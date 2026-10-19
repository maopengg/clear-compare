//! Remote validation, local-only configuration recovery and read-only probes.
use super::*;
use std::{fs::OpenOptions, time::Instant};
fn invalid(message: &str) -> Error {
    Error::Invalid(message.into())
}
pub(super) fn name(root: &Path, value: &str, cancel: &AtomicBool) -> Result<String> {
    if value.is_empty()
        || value.len() > 512
        || value.starts_with('-')
        || value.chars().any(char::is_control)
    {
        return Err(invalid("Invalid remote name"));
    }
    runner::text(
        root,
        &[
            "check-ref-format",
            &format!("refs/remotes/{value}/validation"),
        ],
        cancel,
    )?;
    Ok(value.into())
}
pub(super) fn editable(
    root: &Path,
    remote: &str,
    push: bool,
    cancel: &AtomicBool,
) -> Result<Vec<String>> {
    let key = format!("remote.{remote}.{}", if push { "pushurl" } else { "url" });
    let effective = runner::optional(
        root,
        &[
            "config".into(),
            "--null".into(),
            "--get-all".into(),
            key.clone(),
        ],
        &[],
        cancel,
    )?;
    let local = runner::optional(
        root,
        &[
            "config".into(),
            "--no-includes".into(),
            "--local".into(),
            "--null".into(),
            "--get-all".into(),
            key,
        ],
        &[],
        cancel,
    )?;
    if effective != local {
        return Err(invalid(
            "Remote URLs inherited from another Git config must be edited in their originating config; no change was made",
        ));
    }
    if local.is_empty() {
        return Ok(vec![]);
    }
    let mut values = local.split(|b| *b == 0).collect::<Vec<_>>();
    if values.last() == Some(&b"".as_slice()) {
        values.pop();
    }
    values
        .into_iter()
        .map(|v| {
            String::from_utf8(v.to_vec()).map_err(|_| invalid("Remote URL is not valid UTF-8"))
        })
        .collect()
}
fn directory(path: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    match builder.create(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }
    if !fs::symlink_metadata(path)?.is_dir() {
        return Err(invalid(
            "Configuration backup directory is not a real directory",
        ));
    }
    #[cfg(unix)]
    {
        fs::File::open(path)?.sync_all()?;
        if let Some(parent) = path.parent() {
            fs::File::open(parent)?.sync_all()?;
        }
    }
    Ok(())
}
fn write(path: &Path, data: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(data)?;
    file.sync_all()?;
    if fs::read(path)? != data {
        return Err(invalid("Configuration backup verification failed"));
    }
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        fs::File::open(parent)?.sync_all()?;
    }
    Ok(())
}
pub(super) fn backup(
    repo: &Captured,
    remote: &str,
    token: &str,
    state: &RepositoryState,
    all_tracking: bool,
    cancel: &AtomicBool,
) -> Result<PathBuf> {
    check_cancel(cancel)?;
    let config = repo.common.join("config");
    let meta = fs::symlink_metadata(&config)?;
    if !meta.is_file() || meta.len() > 4 * 1024 * 1024 {
        return Err(invalid(
            "Remote edits require a regular repository config under 4 MiB",
        ));
    }
    let bytes = fs::read(&config)?;
    let root = repo.common.join("clear-compare-remote-backups");
    directory(&root)?;
    let path = root.join(token);
    directory(&path)?;
    write(&path.join("config"), &bytes)?;
    let references = state
        .references
        .iter()
        .filter(|r| {
            r.kind == "remote"
                && (all_tracking || r.full_name.starts_with(&format!("refs/remotes/{remote}/")))
        })
        .cloned()
        .collect::<Vec<_>>();
    let recovery=references.iter().enumerate().map(|(i,r)|serde_json::json!({"name":r.full_name,"object":r.target,"recovery":format!("refs/clear-compare/remote-backups/{token}/{i}")})).collect::<Vec<_>>();
    let manifest = serde_json::to_vec_pretty(
        &serde_json::json!({"version":1,"repository":state.repository.id,"remote":remote,"config":"config","config_sha256":id(&bytes),"references":recovery}),
    )?;
    if manifest.len() > 16 * 1024 * 1024 {
        return Err(invalid("Remote recovery manifest exceeds limit"));
    }
    write(&path.join("before.json"), &manifest)?;
    if !references.is_empty() {
        let mut input = String::from("start\n");
        for (i, r) in references.iter().enumerate() {
            input.push_str(&format!(
                "create refs/clear-compare/remote-backups/{token}/{i} {}\n",
                r.target
            ));
        }
        input.push_str("prepare\ncommit\n");
        runner::mutate_input(
            &repo.root,
            &["update-ref".into(), "--stdin".into()],
            input.as_bytes(),
            cancel,
        )?;
    }
    if fs::read(&config)? != bytes {
        return Err(invalid(
            "Repository config changed while preparing backup; no remote edit was run",
        ));
    }
    Ok(path)
}
pub(super) fn finish(path: &Path, success: bool) -> Result<()> {
    write(
        &path.join("result.json"),
        &serde_json::to_vec(&serde_json::json!({"success":success}))?,
    )
}
pub(super) fn unchanged(repo: &Captured, revision: &str, cancel: &AtomicBool) -> Result<()> {
    let state = status::state(&mut repo.clone(), cancel)?;
    if state.revision != revision {
        return Err(invalid(
            "Repository changed while preparing remote recovery; edit was not run",
        ));
    }
    Ok(())
}
impl Workspace {
    pub fn probe_remote(
        &mut self,
        repository: &str,
        remote: &str,
        push: bool,
        cancel: &AtomicBool,
    ) -> Result<RemoteProbe> {
        let started = Instant::now();
        let state = self.open(repository, cancel)?;
        if !state.remotes.iter().any(|r| r.name == remote) {
            return Err(invalid("Unknown remote; refresh repository"));
        }
        let repo = self
            .repositories
            .get(repository)
            .ok_or_else(|| invalid("Unknown repository"))?;
        if crate::sync::durable::Object::observe(&repo.root, Path::new(""))? != repo.root_identity
            || crate::sync::durable::Object::observe(&repo.git, Path::new(""))? != repo.git_identity
        {
            return Err(invalid("Repository identity changed; scan again"));
        }
        let endpoint = if push {
            runner::text(&repo.root, &["remote", "get-url", "--push", remote], cancel)?
        } else {
            remote.into()
        };
        let output = runner::mutate(
            &repo.root,
            &["ls-remote".into(), "--symref".into(), "--".into(), endpoint],
            cancel,
            true,
        )?;
        let text = String::from_utf8_lossy(&output);
        let default_branch = text.lines().find_map(|l| {
            l.strip_prefix("ref: ")
                .and_then(|s| s.strip_suffix("\tHEAD"))
                .map(String::from)
        });
        let references = text
            .lines()
            .filter(|l| {
                l.split_once('\t').is_some_and(|(sha, _)| {
                    matches!(sha.len(), 40 | 64) && sha.bytes().all(|c| c.is_ascii_hexdigit())
                })
            })
            .count();
        Ok(RemoteProbe {
            remote: remote.into(),
            push,
            references,
            default_branch,
            elapsed_ms: started.elapsed().as_millis().to_string(),
        })
    }
}
