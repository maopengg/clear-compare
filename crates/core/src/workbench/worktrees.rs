//! Captured worktree registrations and identity-checked lifecycle plans.
use super::*;
use crate::sync::durable::Object;
#[derive(Clone)]
pub(super) struct Entry {
    pub view: WorktreeEntry,
    path: PathBuf,
    identity: Option<Object>,
}
pub(super) struct Destination {
    repository: String,
    path: PathBuf,
    identity: Option<Object>,
}
pub(super) struct Guard {
    snapshot: String,
    target: Option<(PathBuf, Option<Object>)>,
    parent: Option<(PathBuf, Option<Object>)>,
    remove: bool,
    prune: bool,
    recovery_heads: Vec<String>,
}
fn invalid(text: &str) -> Error {
    Error::Invalid(text.into())
}
fn text(path: &Path) -> Result<String> {
    runner::path_arg(path)
}
fn raw(repo: &Captured, cancel: &AtomicBool) -> Result<Vec<u8>> {
    runner::run(
        &repo.root,
        &["worktree", "list", "--porcelain", "-z"],
        cancel,
    )
}
fn native_path(bytes: &[u8]) -> Result<PathBuf> {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Ok(PathBuf::from(std::ffi::OsString::from_vec(bytes.to_vec())))
    }
    #[cfg(not(unix))]
    {
        Ok(PathBuf::from(
            std::str::from_utf8(bytes).map_err(|_| invalid("Worktree path is not Unicode"))?,
        ))
    }
}
fn parse(repo: &Captured, raw: &[u8]) -> Result<Vec<Entry>> {
    let mut result = Vec::new();
    let fields = raw.split(|b| *b == 0).collect::<Vec<_>>();
    for record in fields
        .split(|field| field.is_empty())
        .filter(|r| !r.is_empty())
    {
        let Some(first) = record[0].strip_prefix(b"worktree ") else {
            return Err(invalid("Invalid worktree listing"));
        };
        let mut path = native_path(first)?;
        if !path.is_absolute() {
            return Err(invalid("Worktree path must be absolute"));
        }
        let field = |prefix: &[u8]| {
            record
                .iter()
                .find_map(|f| f.strip_prefix(prefix))
                .map(|v| String::from_utf8_lossy(v).into_owned())
        };
        let flag = |name: &[u8]| record.contains(&name);
        let reason = |name: &str| {
            field(format!("{name} ").as_bytes()).or_else(|| flag(name.as_bytes()).then(String::new))
        };
        let canonical = fs::symlink_metadata(&path)
            .ok()
            .filter(|m| m.is_dir())
            .and_then(|_| fs::canonicalize(&path).ok());
        let available = canonical.is_some();
        if let Some(canonical) = canonical {
            path = canonical;
        }
        let mut bytes = repo.view.id.as_bytes().to_vec();
        for field in record {
            bytes.push(0);
            bytes.extend_from_slice(field);
        }
        result.push(Entry {
            view: WorktreeEntry {
                id: id(&bytes),
                path: path.to_string_lossy().into(),
                head: field(b"HEAD "),
                branch: field(b"branch "),
                detached: flag(b"detached"),
                bare: flag(b"bare"),
                main: result.is_empty(),
                current: path == repo.root,
                available,
                locked: reason("locked"),
                prunable: reason("prunable"),
            },
            identity: if available {
                Object::observe(&path, Path::new(""))?
            } else {
                None
            },
            path,
        });
        if result.len() > 4096 {
            return Err(invalid("Worktree list exceeds 4096 entries"));
        }
    }
    Ok(result)
}
fn clean(path: &Path, cancel: &AtomicBool) -> Result<()> {
    let output = runner::run(
        path,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--ignored=matching",
            "--ignore-submodules=none",
        ],
        cancel,
    )?;
    if !output.is_empty() {
        return Err(invalid(
            "Worktree contains changes, untracked or ignored files; preserve them before removal",
        ));
    }
    Ok(())
}
impl Guard {
    pub(super) fn reserve_destination(&self, journal: &Path, cancel: &AtomicBool) -> Result<()> {
        if self.parent.is_none() {
            return Ok(());
        }
        check_cancel(cancel)?;
        let (target, _) = self
            .target
            .as_ref()
            .ok_or_else(|| invalid("Missing worktree destination"))?;
        fs::create_dir(target)?;
        let pin = Object::observe(target, Path::new(""))?;
        super::remotes::write(
            &journal.join("reserved.json"),
            &serde_json::to_vec(
                &serde_json::json!({"path":crate::native_path::encode(target),"identity":pin}),
            )?,
        )?;
        check_cancel(cancel)?;
        if Object::observe(target, Path::new(""))? != pin {
            return Err(invalid("Reserved worktree directory changed"));
        }
        if let Some((parent, pin)) = &self.parent
            && (fs::canonicalize(parent)? != *parent
                || Object::observe(parent, Path::new(""))? != *pin)
        {
            return Err(invalid("Worktree parent changed during reservation"));
        }
        Ok(())
    }
    pub(super) fn removed_repository(&self) -> Option<String> {
        self.remove
            .then(|| self.target.as_ref().map(|(p, _)| path_id(p)))
            .flatten()
    }
    pub(super) fn check(
        &self,
        workspace: &Workspace,
        repo: &Captured,
        cancel: &AtomicBool,
    ) -> Result<()> {
        let listing = raw(repo, cancel)?;
        if id(&listing) != self.snapshot {
            return Err(invalid(
                "Worktree registrations changed; refresh and review again",
            ));
        }
        if let Some((path, pin)) = &self.parent
            && (fs::canonicalize(path)? != *path || Object::observe(path, Path::new(""))? != *pin)
        {
            return Err(invalid(
                "Worktree destination parent changed; choose it again",
            ));
        }
        if let Some((path, pin)) = &self.target {
            if Object::observe(path, Path::new(""))? != *pin {
                return Err(invalid(
                    "Worktree destination or identity changed after review",
                ));
            }
            if self.remove {
                if workspace.open_sources_in(&path_id(path)) {
                    return Err(invalid(
                        "Close this worktree's editor tabs before removing it",
                    ));
                }
                if inspect(path, cancel)?.common != repo.common {
                    return Err(invalid("Worktree repository mapping changed"));
                }
                clean(path, cancel)?;
            }
        }
        if self.prune {
            for entry in parse(repo, &listing)? {
                if entry.view.prunable.is_some() && workspace.open_sources_in(&path_id(&entry.path))
                {
                    return Err(invalid(
                        "Close editor tabs for missing worktrees before pruning",
                    ));
                }
            }
        }
        Ok(())
    }
    pub(super) fn journal(
        &self,
        repo: &Captured,
        token: &str,
        args: &[String],
        cancel: &AtomicBool,
    ) -> Result<PathBuf> {
        let directory = repo.common.join("clear-compare-worktree-operations");
        super::remotes::directory(&directory)?;
        let root = directory.join(token);
        super::remotes::directory(&root)?;
        super::remotes::write(&root.join("registrations.bin"), &raw(repo, cancel)?)?;
        super::remotes::write(
            &root.join("before.json"),
            &serde_json::to_vec_pretty(
                &serde_json::json!({"version":1,"arguments":args,"registration_snapshot":self.snapshot,"registrations_file":"registrations.bin","remove":self.remove,"prune":self.prune,"recovery_heads":self.recovery_heads,"recovery_prefix":format!("refs/clear-compare/worktrees/{token}/")}),
            )?,
        )?;
        if !self.recovery_heads.is_empty() {
            let mut transaction = String::from("start\n");
            for (i, head) in self.recovery_heads.iter().enumerate() {
                transaction.push_str(&format!(
                    "create refs/clear-compare/worktrees/{token}/{i} {head}\n"
                ));
            }
            transaction.push_str("prepare\ncommit\n");
            runner::mutate_input(
                &repo.root,
                &["update-ref".into(), "--stdin".into()],
                transaction.as_bytes(),
                cancel,
            )?;
        }
        Ok(root)
    }
}
impl Workspace {
    pub fn worktrees(&mut self, repository: &str, cancel: &AtomicBool) -> Result<WorktreeList> {
        let state = self.open(repository, cancel)?;
        let repo = self
            .repositories
            .get(repository)
            .ok_or_else(|| invalid("Unknown repository"))?;
        let entries = parse(repo, &raw(repo, cancel)?)?;
        self.worktree_entries.retain(|_, (r, _)| r != repository);
        for entry in &entries {
            self.worktree_entries
                .insert(entry.view.id.clone(), (repository.into(), entry.clone()));
        }
        Ok(WorktreeList {
            state,
            catalog: self.catalog.clone(),
            entries: entries.into_iter().map(|e| e.view).collect(),
        })
    }
    pub fn worktree_destination(
        &mut self,
        repository: &str,
        parent: &str,
        cancel: &AtomicBool,
    ) -> Result<WorktreeDestination> {
        self.open(repository, cancel)?;
        let parent = canonical_root(parent)?;
        if self
            .repositories
            .values()
            .any(|r| parent.starts_with(&r.git) || parent.starts_with(&r.common))
        {
            return Err(invalid("Worktrees cannot be created inside Git metadata"));
        }
        let identity = Object::observe(&parent, Path::new(""))?;
        let id = crate::unique_id();
        if self.worktree_destinations.len() >= 32 {
            self.worktree_destinations.clear();
        }
        self.worktree_destinations.insert(
            id.clone(),
            Destination {
                repository: repository.into(),
                path: parent.clone(),
                identity,
            },
        );
        Ok(WorktreeDestination {
            id,
            path: parent.to_string_lossy().into(),
        })
    }
    fn worktree_entry(&self, repository: &str, id: &str) -> Result<Entry> {
        self.worktree_entries
            .get(id)
            .filter(|(r, _)| r == repository)
            .map(|(_, e)| e.clone())
            .ok_or_else(|| invalid("Worktree selection expired; refresh the list"))
    }
    pub fn worktree_path(
        &mut self,
        repository: &str,
        id: &str,
        cancel: &AtomicBool,
    ) -> Result<PathBuf> {
        self.open(repository, cancel)?;
        let selected = self.worktree_entry(repository, id)?;
        let repo = self
            .repositories
            .get(repository)
            .ok_or_else(|| invalid("Unknown repository"))?;
        if !parse(repo, &raw(repo, cancel)?)?
            .iter()
            .any(|e| e.view.id == selected.view.id)
            || !selected.view.available
        {
            return Err(invalid(
                "Worktree changed or is unavailable; refresh the list",
            ));
        }
        if fs::canonicalize(&selected.path)? != selected.path
            || Object::observe(&selected.path, Path::new(""))? != selected.identity
        {
            return Err(invalid("Worktree path changed"));
        }
        Ok(selected.path)
    }
    pub fn open_worktree(
        &mut self,
        repository: &str,
        id: &str,
        cancel: &AtomicBool,
    ) -> Result<WorktreeOpen> {
        let path = self.worktree_path(repository, id, cancel)?;
        let parent_common = self
            .repositories
            .get(repository)
            .ok_or_else(|| invalid("Unknown repository"))?
            .common
            .clone();
        let captured = inspect(&path, cancel)?;
        if captured.common != parent_common {
            return Err(invalid("Worktree repository mapping changed"));
        }
        let new_id = captured.view.id.clone();
        let add_root = !self
            .catalog
            .settings
            .roots
            .iter()
            .any(|r| path.starts_with(r));
        if add_root && self.catalog.settings.roots.len() >= 64 {
            return Err(invalid(
                "Scan root limit reached; add a shared parent directory first",
            ));
        }
        if !self.catalog.repositories.iter().any(|r| r.id == new_id) {
            if self.catalog.repositories.len() >= 4096 {
                return Err(invalid("Workspace repository limit reached"));
            }
            self.catalog.repositories.push(captured.view.clone());
        }
        if add_root {
            self.catalog.settings.roots.push(text(&path)?);
        }
        self.ignored.remove(&new_id);
        self.repositories.entry(new_id.clone()).or_insert(captured);
        let state = self.open(&new_id, cancel)?;
        Ok(WorktreeOpen {
            state,
            catalog: self.catalog.clone(),
        })
    }
    pub(super) fn prepare_worktree(
        &self,
        repository: &str,
        state: &RepositoryState,
        action: &Action,
        cancel: &AtomicBool,
    ) -> Result<(Vec<String>, Guard, Option<String>)> {
        let repo = self
            .repositories
            .get(repository)
            .ok_or_else(|| invalid("Unknown repository"))?;
        let snapshot = id(&raw(repo, cancel)?);
        let mut guard = Guard {
            snapshot,
            target: None,
            parent: None,
            remove: false,
            prune: false,
            recovery_heads: Vec::new(),
        };
        let mut backup = None;
        let args = match action {
            Action::CreateWorktree {
                destination_id,
                folder,
                reference_id,
                new_branch,
                detach,
            } => {
                if folder.is_empty()
                    || folder.len() > 255
                    || matches!(folder.as_str(), "." | "..")
                    || folder.eq_ignore_ascii_case(".git")
                    || folder.chars().any(|c| {
                        c.is_control()
                            || matches!(c, '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*')
                    })
                    || folder.ends_with([' ', '.'])
                {
                    return Err(invalid("Enter a plain new worktree folder name"));
                }
                #[cfg(windows)]
                if !crate::native_path::windows_file_name(folder) {
                    return Err(invalid("Invalid Windows worktree folder name"));
                }
                let destination = self
                    .worktree_destinations
                    .get(destination_id)
                    .filter(|d| d.repository == repository)
                    .ok_or_else(|| invalid("Choose a worktree parent directory first"))?;
                let target = destination.path.join(folder);
                if fs::symlink_metadata(&target).is_ok() {
                    return Err(invalid("Worktree destination already exists"));
                }
                let reference = reference_id
                    .as_ref()
                    .map(|id| {
                        state
                            .references
                            .iter()
                            .find(|r| &r.id == id)
                            .ok_or_else(|| invalid("Reference selection expired"))
                    })
                    .transpose()?;
                let start = reference.map(|r| r.full_name.as_str()).unwrap_or("HEAD");
                let frozen = runner::text(
                    &repo.root,
                    &[
                        "rev-parse",
                        "--verify",
                        "--end-of-options",
                        &format!("{start}^{{commit}}"),
                    ],
                    cancel,
                )?;
                let mut args = vec!["worktree".into(), "add".into()];
                if let Some(branch) = new_branch {
                    if *detach {
                        return Err(invalid("Choose either a new branch or detached mode"));
                    }
                    if branch.is_empty()
                        || branch.len() > 1024
                        || branch.starts_with('-')
                        || branch.chars().any(char::is_control)
                    {
                        return Err(invalid("Invalid worktree branch name"));
                    }
                    let checked = runner::text(
                        &repo.root,
                        &["check-ref-format", "--branch", branch],
                        cancel,
                    )?;
                    if state
                        .references
                        .iter()
                        .any(|r| r.full_name == format!("refs/heads/{checked}"))
                    {
                        return Err(invalid("Branch already exists"));
                    }
                    args.extend(["-b".into(), checked]);
                } else if *detach {
                    args.push("--detach".into());
                }
                let revision = if new_branch.is_some() || *detach {
                    frozen
                } else {
                    let selected = reference.ok_or_else(|| {
                        invalid("Select an existing local branch or create a new branch")
                    })?;
                    if selected.kind != "local" {
                        return Err(invalid("Existing branch mode requires a local branch"));
                    }
                    selected.name.clone()
                };
                args.extend(["--".into(), text(&target)?, revision]);
                guard.target = Some((target, None));
                guard.parent = Some((destination.path.clone(), destination.identity.clone()));
                args
            }
            Action::RemoveWorktree { id } | Action::SetWorktreeLock { id, .. } => {
                let entry = self.worktree_entry(repository, id)?;
                if !parse(repo, &raw(repo, cancel)?)?
                    .iter()
                    .any(|e| e.view.id == entry.view.id)
                {
                    return Err(invalid("Worktree selection changed; refresh the list"));
                }
                if entry.view.main || entry.view.current {
                    return Err(invalid(
                        "Operate on a different linked worktree; the main/current worktree is protected",
                    ));
                }
                if !entry.view.available {
                    return Err(invalid(
                        "Worktree is missing; review stale registration pruning instead",
                    ));
                }
                guard.target = Some((entry.path.clone(), entry.identity.clone()));
                match action {
                    Action::RemoveWorktree { .. } => {
                        if entry.view.locked.is_some() {
                            return Err(invalid("Unlock the worktree before removal"));
                        }
                        guard.remove = true;
                        backup = entry.view.head.clone();
                        vec![
                            "worktree".into(),
                            "remove".into(),
                            "--".into(),
                            text(&entry.path)?,
                        ]
                    }
                    Action::SetWorktreeLock { reason, .. } => {
                        let mut args = vec!["worktree".into()];
                        if let Some(reason) = reason {
                            if reason.len() > 2048 || reason.chars().any(char::is_control) {
                                return Err(invalid("Invalid worktree lock reason"));
                            }
                            if entry.view.locked.is_some() {
                                return Err(invalid("Worktree is already locked"));
                            }
                            args.extend(["lock".into(), "--reason".into(), reason.clone()]);
                        } else {
                            if entry.view.locked.is_none() {
                                return Err(invalid("Worktree is not locked"));
                            }
                            args.push("unlock".into());
                        }
                        args.extend(["--".into(), text(&entry.path)?]);
                        args
                    }
                    _ => unreachable!(),
                }
            }
            Action::PruneWorktrees => {
                if !parse(repo, &raw(repo, cancel)?)?
                    .iter()
                    .any(|e| e.view.prunable.is_some() && e.view.locked.is_none())
                {
                    return Err(invalid("No unlocked stale worktree registrations to prune"));
                }
                guard.recovery_heads = parse(repo, &raw(repo, cancel)?)?
                    .into_iter()
                    .filter(|e| e.view.prunable.is_some() && e.view.locked.is_none())
                    .filter_map(|e| e.view.head)
                    .filter(|h| {
                        matches!(h.len(), 40 | 64)
                            && h.bytes().all(|b| b.is_ascii_hexdigit())
                            && !h.bytes().all(|b| b == b'0')
                    })
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
                guard.prune = true;
                vec![
                    "worktree".into(),
                    "prune".into(),
                    "--expire".into(),
                    "now".into(),
                    "--verbose".into(),
                ]
            }
            _ => return Err(invalid("Not a worktree action")),
        };
        guard.check(self, repo, cancel)?;
        Ok((args, guard, backup))
    }
}
