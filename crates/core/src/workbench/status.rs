use super::{Captured, id, remote_key, runner};
use crate::{Error, Result, compare::check_cancel};
use clear_contracts::workbench::*;
use std::{fs, path::Path, sync::atomic::AtomicBool};
pub fn remotes(root: &Path, cancel: &AtomicBool) -> Result<Vec<Remote>> {
    let names = runner::text(root, &["remote"], cancel)?;
    let mut result = Vec::new();
    for name in names.lines().take(128) {
        if name.starts_with('-') || name.contains('\0') {
            return Err(Error::Invalid("Invalid remote name".into()));
        }
        let fetch = runner::text(root, &["remote", "get-url", "--all", name], cancel)?;
        let push = runner::text(
            root,
            &["remote", "get-url", "--push", "--all", name],
            cancel,
        )?;
        let fetch_urls = fetch.lines().map(runner::redact).collect::<Vec<_>>();
        let push_urls = push.lines().map(runner::redact).collect::<Vec<_>>();
        let push_local = !runner::optional(
            root,
            &[
                "config".into(),
                "--no-includes".into(),
                "--local".into(),
                "--get-all".into(),
                format!("remote.{name}.pushurl"),
            ],
            &[],
            cancel,
        )?
        .is_empty();
        result.push(Remote {
            fetch_urls: fetch_urls.clone(),
            push_urls: push_urls.clone(),
            push_local,
            name: name.into(),
            fetch: fetch_urls.first().cloned().unwrap_or_default(),
            push: push_urls.first().cloned().unwrap_or_default(),
            match_key: fetch.lines().next().and_then(remote_key),
        });
    }
    Ok(result)
}
fn decode(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}
fn parse(repo: &mut Captured, raw: &[u8]) -> Result<Vec<Change>> {
    repo.files.clear();
    let mut records = raw.split(|b| *b == 0);
    let mut changes = Vec::new();
    repo.view.branch.clear();
    repo.view.head = None;
    repo.view.ahead = 0;
    repo.view.behind = 0;
    while let Some(record) = records.next() {
        if record.is_empty() {
            continue;
        }
        if let Some(value) = record.strip_prefix(b"# branch.head ") {
            repo.view.branch = decode(value);
            continue;
        }
        if let Some(value) = record.strip_prefix(b"# branch.oid ") {
            if value != b"(initial)" {
                repo.view.head = Some(decode(value));
            }
            continue;
        }
        if let Some(value) = record.strip_prefix(b"# branch.ab ") {
            let value = decode(value);
            let mut parts = value.split_whitespace();
            repo.view.ahead = parts
                .next()
                .and_then(|s| s.strip_prefix('+'))
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            repo.view.behind = parts
                .next()
                .and_then(|s| s.strip_prefix('-'))
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);
            continue;
        }
        if record[0] == b'#' {
            continue;
        }
        let (path, original, xy, untracked, conflicted) = match record[0] {
            b'1' | b'2' => {
                let rename = record[0] == b'2';
                let fields = record
                    .splitn(if rename { 10 } else { 9 }, |b| *b == b' ')
                    .collect::<Vec<_>>();
                if fields.len() != if rename { 10 } else { 9 } || fields[1].len() != 2 {
                    return Err(Error::Invalid("Malformed Git status entry".into()));
                }
                let original = if rename {
                    Some(
                        records
                            .next()
                            .ok_or_else(|| Error::Invalid("Missing rename source".into()))?,
                    )
                } else {
                    None
                };
                (*fields.last().unwrap(), original, fields[1], false, false)
            }
            b'u' => {
                let fields = record.splitn(11, |b| *b == b' ').collect::<Vec<_>>();
                if fields.len() != 11 || fields[1].len() != 2 {
                    return Err(Error::Invalid("Malformed conflict entry".into()));
                }
                (fields[10], None, fields[1], false, true)
            }
            b'?' => {
                if record.len() < 3 {
                    return Err(Error::Invalid("Malformed untracked entry".into()));
                }
                (&record[2..], None, b"??".as_slice(), true, false)
            }
            _ => return Err(Error::Invalid("Unsupported Git status entry".into())),
        };
        if changes.len() >= 50_000 {
            return Err(Error::Invalid(
                "Repository has more than 50000 changed entries; narrow untracked scope".into(),
            ));
        }
        let mut paths = vec![native_path(path)?];
        if let Some(original) = original {
            paths.push(native_path(original)?);
        }
        repo.files.insert(id(path), paths);
        changes.push(Change {
            id: id(path),
            path: decode(path),
            original_path: original.map(decode),
            index: decode(&xy[..1]),
            worktree: decode(&xy[1..]),
            untracked,
            conflicted,
        });
    }
    repo.view.changed = changes.len();
    repo.view.conflicts = changes.iter().filter(|c| c.conflicted).count();
    Ok(changes)
}
pub(super) fn native_path(bytes: &[u8]) -> Result<std::path::PathBuf> {
    #[cfg(unix)]
    let path = {
        use std::os::unix::ffi::OsStringExt;
        std::path::PathBuf::from(std::ffi::OsString::from_vec(bytes.to_vec()))
    };
    #[cfg(not(unix))]
    let path =
        std::path::PathBuf::from(std::str::from_utf8(bytes).map_err(|_| {
            Error::Invalid("Git path is not valid Unicode on this platform".into())
        })?);
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err(Error::Invalid("Unsafe Git path".into()));
    }
    Ok(path)
}
pub fn summary(repo: &mut Captured, cancel: &AtomicBool) -> Result<Vec<Change>> {
    check_cancel(cancel)?;
    if repo.view.kind == "bare" {
        repo.view.head = runner::text(&repo.root, &["rev-parse", "--verify", "HEAD"], cancel).ok();
        repo.view.branch = runner::text(
            &repo.root,
            &["symbolic-ref", "--short", "-q", "HEAD"],
            cancel,
        )
        .unwrap_or_else(|_| "(detached)".into());
        return Ok(Vec::new());
    }
    let raw = runner::run(
        &repo.root,
        &[
            "status",
            "--porcelain=v2",
            "-z",
            "--branch",
            "--untracked-files=normal",
        ],
        cancel,
    )?;
    parse(repo, &raw)
}
pub fn state(repo: &mut Captured, cancel: &AtomicBool) -> Result<RepositoryState> {
    let changes = summary(repo, cancel)?;
    let refs = runner::run(
        &repo.root,
        &[
            "for-each-ref",
            "--format=%(refname)%00%(objectname)%00%(upstream:short)%00%(upstream:track)%00%(HEAD)",
            "refs/heads",
            "refs/remotes",
            "refs/tags",
        ],
        cancel,
    )?;
    let mut references = Vec::new();
    for line in refs.split(|b| *b == b'\n').filter(|l| !l.is_empty()) {
        let fields = line.split(|b| *b == 0).collect::<Vec<_>>();
        if fields.len() != 5 {
            return Err(Error::Invalid("Malformed reference output".into()));
        }
        let full_name = decode(fields[0]);
        let (kind, name) = if let Some(n) = full_name.strip_prefix("refs/heads/") {
            ("local", n)
        } else if let Some(n) = full_name.strip_prefix("refs/remotes/") {
            ("remote", n)
        } else {
            (
                "tag",
                full_name.strip_prefix("refs/tags/").unwrap_or(&full_name),
            )
        };
        references.push(Reference {
            id: id(fields[0]),
            name: name.into(),
            full_name,
            target: decode(fields[1]),
            kind: kind.into(),
            upstream: decode(fields[2]),
            tracking: decode(fields[3]),
            current: fields[4] == b"*",
        });
    }
    let remotes = remotes(&repo.root, cancel)?;
    let operation =
        if repo.git.join("rebase-merge").exists() || repo.git.join("rebase-apply").exists() {
            Some("rebase")
        } else if repo.git.join("MERGE_HEAD").exists() {
            Some("merge")
        } else if repo.git.join("CHERRY_PICK_HEAD").exists() {
            Some("cherry_pick")
        } else if repo.git.join("REVERT_HEAD").exists() {
            Some("revert")
        } else {
            None
        }
        .map(String::from);
    let mut revision_data =
        serde_json::to_vec(&(&repo.view.head, &changes, &references, &operation, &remotes))?;
    // Stash selectors are positional: any reflog change invalidates a reviewed operation.
    for name in ["index", "logs/refs/stash", "config"] {
        let base = if name == "index" {
            &repo.git
        } else {
            &repo.common
        };
        match fs::read(base.join(name)) {
            Ok(bytes) => revision_data.extend_from_slice(id(&bytes).as_bytes()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => revision_data.push(0),
            Err(e) => return Err(e.into()),
        }
    }
    for paths in repo.files.values() {
        for path in paths {
            check_cancel(cancel)?;
            match fs::symlink_metadata(repo.root.join(path)) {
                Ok(meta) => revision_data.extend_from_slice(
                    format!(
                        "{:?}:{}:{:?}",
                        meta.modified().ok(),
                        meta.len(),
                        meta.file_type()
                    )
                    .as_bytes(),
                ),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => revision_data.push(0),
                Err(e) => return Err(e.into()),
            }
        }
    }
    if let Ok(meta) = fs::metadata(repo.git.join("index")) {
        revision_data
            .extend_from_slice(format!("{:?}:{}", meta.modified().ok(), meta.len()).as_bytes());
    }
    repo.view.match_key = remotes
        .iter()
        .find(|r| r.name == "origin")
        .or(remotes.first())
        .and_then(|r| r.match_key.clone());
    repo.view.error = None;
    repo.view.available = true;
    Ok(RepositoryState {
        reference_settings: ReferenceSettings::default(),
        repository: repo.view.clone(),
        revision: id(&revision_data),
        operation,
        changes,
        references,
        remotes,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn porcelain_preserves_spaces_renames_and_conflicts() {
        let mut r = Captured {
            root: "/example".into(),
            git: "/example/.git".into(),
            common: "/example/.git".into(),
            files: Default::default(),
            root_identity: None,
            git_identity: None,
            view: Repository {
                id: "r".into(),
                name: "r".into(),
                path: "/example".into(),
                kind: "repository".into(),
                common_id: "r".into(),
                match_key: None,
                branch: String::new(),
                head: None,
                changed: 0,
                conflicts: 0,
                ahead: 0,
                behind: 0,
                favorite: false,
                last_opened: String::new(),
                available: true,
                error: None,
            },
        };
        let v=parse(&mut r,b"# branch.head main\0# branch.ab +2 -3\02 R. N... 100644 100644 100644 a b R100 new name\0old name\0u UU N... 100644 100644 100644 100644 a b c conflict\0? fresh file\0").unwrap();
        assert_eq!(r.view.branch, "main");
        assert_eq!((r.view.ahead, r.view.behind), (2, 3));
        assert_eq!(v[0].path, "new name");
        assert_eq!(v[0].original_path.as_deref(), Some("old name"));
        assert!(v[1].conflicted);
        assert!(v[2].untracked);
    }
}
