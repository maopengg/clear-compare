//! Captured gitlinks, recursive module inspection and reviewed updates without force.
use super::*;
use crate::sync::durable::Object;
#[derive(Clone)]
pub(super) struct Entry {
    pub view: SubmoduleEntry,
    parent: PathBuf,
    relative: String,
    root: PathBuf,
    initialized: Option<Captured>,
    fingerprint: String,
}
struct Scan {
    entries: Vec<Entry>,
    fingerprint: String,
    configs: BTreeMap<PathBuf, Vec<u8>>,
    cached: Vec<(PathBuf, Option<String>, String)>,
}
pub(super) struct Guard {
    top: Captured,
    fingerprint: String,
    affected: Vec<Entry>,
    update: bool,
    command_root: PathBuf,
}
fn invalid(s: &str) -> Error {
    Error::Invalid(s.into())
}
fn bytes(path: &Path) -> Result<Vec<u8>> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() && meta.len() <= 4 * 1024 * 1024 => Ok(fs::read(path)?),
        Ok(_) => Err(invalid(
            "Submodule configuration must be a regular file under 4 MiB",
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(e) => Err(e.into()),
    }
}
fn relative(value: &str) -> Result<PathBuf> {
    let p = Path::new(value);
    if value.is_empty()
        || value.len() > 8192
        || value.contains('\0')
        || value.contains('\\')
        || p.components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
        || p.components()
            .any(|c| c.as_os_str().to_string_lossy().eq_ignore_ascii_case(".git"))
    {
        return Err(invalid(
            "Submodule path must be a safe repository-relative path",
        ));
    }
    Ok(p.into())
}
fn child(root: &Path, path: &Path) -> Result<PathBuf> {
    let mut out = root.to_path_buf();
    for part in path.components() {
        out.push(part);
        match fs::symlink_metadata(&out) {
            Ok(m) if !m.is_dir() => {
                return Err(invalid("Submodule path contains a link or non-directory"));
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(out)
}
fn config(root: &Path, file: bool, cancel: &AtomicBool) -> Result<Vec<u8>> {
    let mut args = vec!["config".into()];
    if file {
        args.extend(["--file".into(), ".gitmodules".into()]);
    }
    args.extend([
        "--null".into(),
        "--get-regexp".into(),
        "^submodule\\..*\\.(path|url)$".into(),
    ]);
    // No-match status 1 is a valid empty configuration.
    runner::optional(root, &args, &[], cancel)
}
fn split(bytes: &[u8], at: u8) -> Option<(&[u8], &[u8])> {
    let index = bytes.iter().position(|b| *b == at)?;
    Some((&bytes[..index], &bytes[index + 1..]))
}
fn values(raw: &[u8], strict: bool) -> Result<BTreeMap<String, String>> {
    let mut values = BTreeMap::new();
    for field in raw.split(|b| *b == 0).filter(|v| !v.is_empty()) {
        let (key, value) =
            split(field, b'\n').ok_or_else(|| invalid("Malformed submodule configuration"))?;
        let key =
            std::str::from_utf8(key).map_err(|_| invalid("Submodule names must be Unicode"))?;
        let value = std::str::from_utf8(value)
            .map_err(|_| invalid("Submodule settings must be Unicode"))?;
        if key.chars().any(char::is_control)
            || value.contains('\0')
            || (values.insert(key.to_string(), value.to_string()).is_some() && strict)
        {
            return Err(invalid("Duplicate or invalid submodule configuration"));
        }
    }
    Ok(values)
}
fn visit(
    repo: &Captured,
    top: &Path,
    depth: usize,
    scan: &mut Scan,
    seen: &mut BTreeSet<PathBuf>,
    cancel: &AtomicBool,
) -> Result<()> {
    check_cancel(cancel)?;
    if depth > 32 {
        return Err(invalid("Submodule nesting exceeds 32 levels"));
    }
    if !seen.insert(repo.git.clone()) {
        return Err(invalid("Submodule repository cycle detected"));
    }
    let modules = bytes(&repo.root.join(".gitmodules"))?;
    let config_bytes = bytes(&repo.common.join("config"))?;
    if scan.configs.values().map(Vec::len).sum::<usize>() + config_bytes.len() + 4 * 1024 * 1024
        > 32 * 1024 * 1024
    {
        return Err(invalid("Submodule configuration snapshot exceeds 32 MiB"));
    }
    scan.configs
        .insert(repo.common.join("config"), config_bytes.clone());
    let worktree_config = bytes(&repo.git.join("config.worktree"))?;
    if !worktree_config.is_empty() {
        scan.configs
            .insert(repo.git.join("config.worktree"), worktree_config.clone());
    }
    let declared = if modules.is_empty() {
        BTreeMap::new()
    } else {
        values(&config(&repo.root, true, cancel)?, true)?
    };
    let configured = values(&config(&repo.root, false, cancel)?, false)?;
    let raw = runner::run(&repo.root, &["ls-files", "--stage", "-z"], cancel)?;
    let mut links = BTreeMap::<String, (Option<String>, bool)>::new();
    for field in raw.split(|b| *b == 0).filter(|v| !v.is_empty()) {
        let (header, path) = split(field, b'\t').ok_or_else(|| invalid("Malformed index entry"))?;
        let header = std::str::from_utf8(header).map_err(|_| invalid("Malformed gitlink"))?;
        let parts = header.split_whitespace().collect::<Vec<_>>();
        if parts.len() != 3 {
            return Err(invalid("Malformed index stage"));
        }
        if parts[0] != "160000" {
            continue;
        }
        let path = std::str::from_utf8(path)
            .map_err(|_| invalid("Submodule path must be Unicode"))?
            .to_string();
        let link = links.entry(path).or_default();
        if parts[2] == "0" {
            link.0 = Some(parts[1].into())
        } else {
            link.1 = true;
        }
    }
    let mut names = BTreeMap::new();
    for (key, path) in &declared {
        if let Some(name) = key
            .strip_prefix("submodule.")
            .and_then(|k| k.strip_suffix(".path"))
        {
            relative(path)?;
            if names.insert(path.clone(), name.to_string()).is_some() {
                return Err(invalid("Multiple submodule names use the same path"));
            }
        }
    }
    let paths = names
        .keys()
        .chain(links.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    for path in paths {
        check_cancel(cancel)?;
        if scan.entries.len() >= 4096 {
            return Err(invalid("Submodule list exceeds 4096 entries"));
        }
        let rel = relative(&path)?;
        let name = names.get(&path).cloned().unwrap_or_default();
        let (expected, conflicted) = links.get(&path).cloned().unwrap_or_default();
        let mut issue = None;
        let root = match child(&repo.root, &rel) {
            Ok(root) => root,
            Err(e) => {
                issue = Some(e.to_string());
                repo.root.join(&rel)
            }
        };
        let identity = if issue.is_none() {
            Object::observe(&root, Path::new(""))?
        } else {
            None
        };
        let declared_url = declared
            .get(&format!("submodule.{name}.url"))
            .cloned()
            .unwrap_or_default();
        let configured_url = configured.get(&format!("submodule.{name}.url")).cloned();
        let mut initialized = None;
        let mut head = None;
        let mut changes = 0;
        let mut actual_url = None;
        let mut revision = String::new();
        if issue.is_none() && fs::symlink_metadata(root.join(".git")).is_ok() {
            if fs::symlink_metadata(root.join(".git"))?
                .file_type()
                .is_symlink()
            {
                issue = Some("Submodule Git metadata cannot be a symbolic link".into());
            } else {
                let inspected =
                    inspect(&fs::canonicalize(&root)?, cancel).and_then(|mut captured| {
                        if captured.git == repo.git || captured.view.kind == "bare" {
                            return Err(invalid("Submodule repository mapping is invalid"));
                        }
                        let state = status::state(&mut captured, cancel)?;
                        let nested_links =
                            runner::run(&captured.root, &["ls-files", "--stage", "-z"], cancel)?;
                        let nested = nested_links
                            .split(|b| *b == 0)
                            .filter_map(|line| {
                                line.strip_prefix(b"160000 ")
                                    .and_then(|l| split(l, b'\t'))
                                    .map(|(_, p)| String::from_utf8_lossy(p).to_string())
                            })
                            .collect::<BTreeSet<_>>();
                        changes = state
                            .changes
                            .iter()
                            .filter(|c| !nested.contains(&c.path))
                            .count();
                        let ignored = runner::run(
                            &captured.root,
                            &[
                                "ls-files",
                                "--others",
                                "--ignored",
                                "--exclude-standard",
                                "--directory",
                                "-z",
                            ],
                            cancel,
                        )?;
                        changes += ignored.split(|b| *b == 0).filter(|v| !v.is_empty()).count();
                        if state.operation.is_some() {
                            changes += 1;
                        }
                        head = state.repository.head.clone();
                        revision = state.revision;
                        actual_url = state
                            .remotes
                            .iter()
                            .find(|r| r.name == "origin")
                            .or(state.remotes.first())
                            .map(|r| r.fetch.clone());
                        Ok(captured)
                    });
                match inspected {
                    Ok(c) => initialized = Some(c),
                    Err(e) => issue = Some(e.to_string()),
                }
            }
        } else if issue.is_none() && root.exists() && fs::read_dir(&root)?.next().is_some() {
            issue = Some("Uninitialized submodule directory contains files".into());
        }
        let status = if issue.is_some() {
            "blocked"
        } else if conflicted {
            "conflicted"
        } else if expected.is_none() || name.is_empty() {
            "missing_configuration"
        } else if initialized.is_none() {
            "uninitialized"
        } else if changes > 0 {
            "dirty"
        } else if head != expected {
            "different_commit"
        } else {
            "current"
        };
        let display = root
            .strip_prefix(top)
            .map_err(|_| invalid("Submodule escaped project root"))?
            .to_string_lossy()
            .into_owned();
        let fingerprint = id(&serde_json::to_vec(
            &serde_json::json!({"root":crate::native_path::encode(&root),"relative":path,"expected":expected,"conflicted":conflicted,"modules":id(&modules),"config":id(&config_bytes),"worktree_config":id(&worktree_config),"identity":identity,"git_identity":initialized.as_ref().map(|c|&c.git_identity),"head":head,"revision":revision,"changes":changes,"issue":issue}),
        )?);
        let view = SubmoduleEntry {
            id: id(format!("{}:{fingerprint}", repo.view.id).as_bytes()),
            name,
            path: display,
            depth,
            expected,
            head,
            declared_url: runner::redact(&declared_url),
            configured_url: configured_url.as_ref().map(|u| runner::redact(u)),
            actual_url,
            registered: configured_url.is_some(),
            initialized: initialized.is_some(),
            status: status.into(),
            changes,
            issue,
        };
        scan.entries.push(Entry {
            view,
            parent: repo.root.clone(),
            relative: path,
            root,
            initialized: initialized.clone(),
            fingerprint,
        });
        if let Some(initialized) = initialized {
            visit(&initialized, top, depth + 1, scan, seen, cancel)?;
        }
    }
    seen.remove(&repo.git);
    Ok(())
}
fn administrative(git: &Path, worktree: &Path, tail: &[&str]) -> Result<Vec<String>> {
    let mut args = vec![
        "--git-dir".into(),
        runner::path_arg(git)?,
        "--work-tree".into(),
        runner::path_arg(worktree)?,
    ];
    args.extend(tail.iter().map(|v| v.to_string()));
    Ok(args)
}
fn cached(repo: &Captured, scan: &mut Scan, cancel: &AtomicBool) -> Result<()> {
    let base = repo.common.join("modules");
    let mut pending = vec![(base, 0usize)];
    let mut visited = 0usize;
    while let Some((path, depth)) = pending.pop() {
        check_cancel(cancel)?;
        if depth > 64 || visited >= 8192 {
            return Err(invalid("Cached submodule metadata traversal limit reached"));
        }
        visited += 1;
        let meta = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e.into()),
        };
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err(invalid(
                "Cached submodule metadata contains a link or invalid directory",
            ));
        }
        if path.join("config").is_file() || path.join("HEAD").is_file() {
            let config = bytes(&path.join("config"))?;
            let raw_head = bytes(&path.join("HEAD"))?;
            if scan.configs.values().map(Vec::len).sum::<usize>() + config.len() + raw_head.len()
                > 32 * 1024 * 1024
            {
                return Err(invalid("Submodule configuration snapshot exceeds 32 MiB"));
            }
            scan.configs.insert(path.join("config"), config);
            scan.configs.insert(path.join("HEAD"), raw_head.clone());
            let head = if !raw_head.is_empty() {
                let raw = runner::optional(
                    &repo.root,
                    &administrative(
                        &path,
                        &repo.root,
                        &["rev-parse", "--verify", "--quiet", "HEAD"],
                    )?,
                    &[],
                    cancel,
                )?;
                let text =
                    String::from_utf8(raw).map_err(|_| invalid("Invalid cached submodule HEAD"))?;
                let text = text.trim();
                (!text.is_empty()).then(|| text.to_string())
            } else {
                None
            };
            let identity = Object::observe(&path, Path::new(""))?;
            scan.cached.push((
                path.clone(),
                head,
                id(&serde_json::to_vec(&(identity, id(&raw_head)))?),
            ));
            if scan.cached.len() > 4096 {
                return Err(invalid("Cached submodule count exceeds 4096"));
            }
            pending.push((path.join("modules"), depth + 1));
        } else {
            for entry in fs::read_dir(&path)? {
                let entry = entry?;
                let kind = entry.file_type()?;
                if kind.is_symlink() {
                    return Err(invalid(
                        "Cached submodule container contains a symbolic link",
                    ));
                }
                if kind.is_dir() {
                    if pending.len() >= 8192 {
                        return Err(invalid(
                            "Cached submodule directory queue exceeds its limit",
                        ));
                    }
                    pending.push((entry.path(), depth + 1));
                }
            }
        }
    }
    scan.cached.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(())
}
fn capture(repo: &Captured, cancel: &AtomicBool) -> Result<Scan> {
    if repo.view.kind == "bare" {
        return Err(invalid("Submodule management requires a working tree"));
    }
    let mut scan = Scan {
        entries: vec![],
        fingerprint: String::new(),
        configs: BTreeMap::new(),
        cached: Vec::new(),
    };
    visit(repo, &repo.root, 0, &mut scan, &mut BTreeSet::new(), cancel)?;
    cached(repo, &mut scan, cancel)?;
    scan.fingerprint = id(&serde_json::to_vec(&(
        scan.entries
            .iter()
            .map(|e| &e.fingerprint)
            .collect::<Vec<_>>(),
        &scan.cached,
        scan.configs
            .iter()
            .map(|(p, b)| (crate::native_path::encode(p), id(b)))
            .collect::<Vec<_>>(),
    ))?);
    Ok(scan)
}
impl Guard {
    pub(super) fn command_root(&self) -> &Path {
        &self.command_root
    }
    pub(super) fn check(&self, w: &Workspace, cancel: &AtomicBool) -> Result<()> {
        let scan = capture(&self.top, cancel)?;
        if scan.fingerprint != self.fingerprint {
            return Err(invalid("Submodule state changed; refresh and review again"));
        }
        if self.update {
            for entry in &self.affected {
                if w.dirty_repository(&path_id(
                    entry
                        .initialized
                        .as_ref()
                        .map(|c| c.root.as_path())
                        .unwrap_or(&entry.root),
                )) {
                    return Err(invalid(
                        "Save unsaved submodule editor buffers before updating",
                    ));
                }
                if entry.view.changes > 0 {
                    return Err(invalid(
                        "Submodule has changes, untracked or ignored files; preserve them before updating",
                    ));
                }
            }
        }
        Ok(())
    }
    pub(super) fn journal(&self, token: &str, cancel: &AtomicBool) -> Result<PathBuf> {
        let scan = capture(&self.top, cancel)?;
        if scan.fingerprint != self.fingerprint {
            return Err(invalid("Submodule state changed before backup"));
        }
        let parent = self.top.common.join("clear-compare-submodule-operations");
        super::remotes::directory(&parent)?;
        let root = parent.join(token);
        super::remotes::directory(&root)?;
        let mut configs = vec![];
        for (n, (path, bytes)) in scan.configs.iter().enumerate() {
            let file = format!("config-{n}");
            super::remotes::write(&root.join(&file), bytes)?;
            configs.push(serde_json::json!({"path":crate::native_path::encode(path),"file":file,"sha256":id(bytes)}));
        }
        let mut recovery = vec![];
        let mut updates = Vec::new();
        if self.update {
            for (n, (git, head, _)) in scan.cached.iter().enumerate() {
                if let Some(head) = head {
                    let reference = format!("refs/clear-compare/submodules/{token}/cached-{n}");
                    updates.push((git.clone(), reference.clone(), head.clone()));
                    recovery.push(serde_json::json!({"git":crate::native_path::encode(git),"head":head,"reference":reference}));
                }
            }
            for (n, entry) in self.affected.iter().enumerate() {
                if let (Some(repo), Some(head)) = (&entry.initialized, &entry.view.head) {
                    let reference = format!("refs/clear-compare/submodules/{token}/{n}");
                    updates.push((repo.git.clone(), reference.clone(), head.clone()));
                    recovery.push(serde_json::json!({"path":crate::native_path::encode(&repo.root),"head":head,"reference":reference}));
                }
            }
        }
        super::remotes::write(
            &root.join("before.json"),
            &serde_json::to_vec_pretty(
                &serde_json::json!({"version":1,"snapshot":self.fingerprint,"configs":configs,"references":recovery,"modules":self.affected.iter().map(|e|&e.view).collect::<Vec<_>>()}),
            )?,
        )?;
        for (git, reference, head) in updates {
            runner::mutate(
                &self.top.root,
                &administrative(&git, &self.top.root, &["update-ref", &reference, &head])?,
                cancel,
                false,
            )?;
        }
        Ok(root)
    }
}
impl Workspace {
    pub fn submodules(&mut self, repository: &str, cancel: &AtomicBool) -> Result<SubmoduleList> {
        let state = self.open(repository, cancel)?;
        let repo = self
            .repositories
            .get(repository)
            .ok_or_else(|| invalid("Unknown repository"))?;
        let scan = capture(repo, cancel)?;
        self.submodule_entries.retain(|_, (id, _)| id != repository);
        for entry in &scan.entries {
            self.submodule_entries
                .insert(entry.view.id.clone(), (repository.into(), entry.clone()));
        }
        Ok(SubmoduleList {
            state,
            entries: scan.entries.into_iter().map(|e| e.view).collect(),
        })
    }
    fn submodule_entry(&self, repository: &str, id: &str) -> Result<Entry> {
        self.submodule_entries
            .get(id)
            .filter(|(r, _)| r == repository)
            .map(|(_, e)| e.clone())
            .ok_or_else(|| invalid("Submodule selection expired; refresh the list"))
    }
    pub fn open_submodule(
        &mut self,
        repository: &str,
        id: &str,
        cancel: &AtomicBool,
    ) -> Result<WorktreeOpen> {
        self.open(repository, cancel)?;
        let entry = self.submodule_entry(repository, id)?;
        let repo = self
            .repositories
            .get(repository)
            .ok_or_else(|| invalid("Unknown repository"))?;
        if !capture(repo, cancel)?
            .entries
            .iter()
            .any(|e| e.view.id == entry.view.id)
            || entry.initialized.is_none()
        {
            return Err(invalid(
                "Submodule changed or is uninitialized; refresh the list",
            ));
        }
        let captured = inspect(&fs::canonicalize(&entry.root)?, cancel)?;
        let new_id = captured.view.id.clone();
        let add_root = !self
            .catalog
            .settings
            .roots
            .iter()
            .any(|r| captured.root.starts_with(r));
        if add_root && self.catalog.settings.roots.len() >= 64 {
            return Err(invalid("Scan root limit reached"));
        }
        if !self.catalog.repositories.iter().any(|r| r.id == new_id) {
            if self.catalog.repositories.len() >= 4096 {
                return Err(invalid("Repository limit reached"));
            }
            self.catalog.repositories.push(captured.view.clone());
        }
        if add_root {
            self.catalog.settings.roots.push(captured.view.path.clone());
        }
        self.ignored.remove(&new_id);
        self.repositories.entry(new_id.clone()).or_insert(captured);
        let state = self.open(&new_id, cancel)?;
        Ok(WorktreeOpen {
            state,
            catalog: self.catalog.clone(),
        })
    }
    pub(super) fn prepare_submodules(
        &self,
        repository: &str,
        action: &Action,
        cancel: &AtomicBool,
    ) -> Result<(Vec<String>, Guard, bool)> {
        let repo = self
            .repositories
            .get(repository)
            .ok_or_else(|| invalid("Unknown repository"))?;
        let (selected, update, recursive) = match action {
            Action::InitializeSubmodules { id } => (id, false, false),
            Action::UpdateSubmodules { id, recursive } => (id, true, *recursive),
            _ => return Err(invalid("Not a submodule action")),
        };
        let chosen = selected
            .as_ref()
            .map(|id| self.submodule_entry(repository, id))
            .transpose()?;
        let scan = capture(repo, cancel)?;
        if let Some(chosen) = &chosen
            && !scan.entries.iter().any(|e| e.view.id == chosen.view.id)
        {
            return Err(invalid("Submodule selection changed; refresh the list"));
        }
        let affected = scan
            .entries
            .iter()
            .filter(|e| {
                if let Some(chosen) = &chosen {
                    e.root == chosen.root || (recursive && e.root.starts_with(&chosen.root))
                } else {
                    recursive || e.view.depth == 0
                }
            })
            .cloned()
            .collect::<Vec<_>>();
        if affected.is_empty() {
            return Err(invalid("No submodules selected"));
        }
        if affected.iter().any(|e| {
            e.view.issue.is_some()
                || matches!(
                    e.view.status.as_str(),
                    "conflicted" | "missing_configuration"
                )
        }) {
            return Err(invalid(
                "Resolve invalid or conflicted submodule configuration before this action",
            ));
        }
        let command_root = chosen
            .as_ref()
            .map(|e| e.parent.clone())
            .unwrap_or_else(|| repo.root.clone());
        let mut args = vec!["submodule".into()];
        if update {
            args.extend([
                "update".into(),
                "--init".into(),
                "--checkout".into(),
                "--jobs=1".into(),
            ]);
            if recursive {
                args.push("--recursive".into());
            }
        } else {
            args.push("init".into());
        }
        args.push("--".into());
        if let Some(chosen) = chosen {
            args.push(format!(":(literal){}", chosen.relative));
        }
        let guard = Guard {
            top: repo.clone(),
            fingerprint: scan.fingerprint,
            affected,
            update,
            command_root,
        };
        guard.check(self, cancel)?;
        Ok((args, guard, update))
    }
}
