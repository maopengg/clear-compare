//! Typed Git operations with one-use plans, revision checks and recovery references.
use super::*;
use clear_contracts::workbench::{Action, ActionPlan, ActionResult};
pub(super) struct Prepared {
    view: ActionPlan,
    args: Vec<String>,
    network: bool,
    backup_worktree: bool,
    backup_target: Option<String>,
    input: Option<Vec<u8>>,
    selection: Option<super::working::SelectionGuard>,
    guard_unsaved: bool,
    followup: Option<(Vec<String>, bool)>,
    checkout_expectation: Option<(String, String)>,
    preference_rename: Option<(String, String)>,
}
fn invalid(message: &str) -> Error {
    Error::Invalid(message.into())
}
fn atom(value: &str) -> Result<String> {
    if value.is_empty()
        || value.len() > 2048
        || value.starts_with('-')
        || value.chars().any(char::is_control)
    {
        return Err(invalid("Invalid Git name or reference"));
    }
    Ok(value.into())
}
fn branch(root: &Path, name: &str, cancel: &AtomicBool) -> Result<String> {
    let name = atom(name)?;
    // Git expands @{-n}; freeze the expanded name in the reviewed command.
    runner::text(root, &["check-ref-format", "--branch", &name], cancel)
}
fn commit(root: &Path, reference: &str, cancel: &AtomicBool) -> Result<String> {
    let reference = atom(reference)?;
    runner::text(
        root,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{reference}^{{commit}}"),
        ],
        cancel,
    )
}
fn message(value: &str) -> Result<String> {
    if value.trim().is_empty() || value.len() > 8000 || value.contains('\0') {
        return Err(invalid("Git message must contain 1–8000 bytes"));
    }
    Ok(value.into())
}
fn remote(state: &RepositoryState, name: &str) -> Result<String> {
    let name = atom(name)?;
    if !state.remotes.iter().any(|r| r.name == name) {
        return Err(invalid("Unknown remote; refresh repository"));
    }
    Ok(name)
}
fn stash(repo: &Path, index: usize, cancel: &AtomicBool) -> Result<String> {
    if index > 10_000 {
        return Err(invalid("Invalid stash index"));
    }
    commit(repo, &format!("stash@{{{index}}}"), cancel)
}
fn continuation(state: &RepositoryState, operation: &str, flag: &str) -> Result<Vec<String>> {
    if state.operation.as_deref() != Some(operation) {
        return Err(invalid("Repository operation changed; refresh first"));
    }
    let command = match operation {
        "merge" if flag != "--skip" => "merge",
        "rebase" => "rebase",
        "cherry_pick" => "cherry-pick",
        "revert" => "revert",
        _ => return Err(invalid("This operation cannot be skipped")),
    };
    if flag == "--continue" && state.repository.conflicts != 0 {
        return Err(invalid("Resolve and stage conflicts before continuing"));
    }
    Ok(vec![command.into(), flag.into()])
}
impl Workspace {
    pub fn prepare(
        &mut self,
        repository_id: &str,
        revision: &str,
        action: Action,
        cancel: &AtomicBool,
    ) -> Result<ActionPlan> {
        self.pending = None;
        let guard_unsaved = matches!(
            &action,
            Action::CreateAndSwitch { .. }
                | Action::CheckoutRemote { .. }
                | Action::SwitchAndUpdate { .. }
                | Action::SwitchAndRebase { .. }
                | Action::UpdateCurrent { .. }
                | Action::StageSelection { .. }
                | Action::Stage { .. }
                | Action::Unstage { .. }
                | Action::Discard { .. }
                | Action::Commit { .. }
                | Action::Switch { .. }
                | Action::Merge { .. }
                | Action::Rebase { .. }
                | Action::CherryPick { .. }
                | Action::Revert { .. }
                | Action::Reset { .. }
                | Action::Continue { .. }
                | Action::Abort { .. }
                | Action::Skip { .. }
                | Action::Pull { .. }
                | Action::StashUse {
                    operation: StashOperation::Apply | StashOperation::Pop | StashOperation::Branch,
                    ..
                }
                | Action::StashSave { .. }
                | Action::StashApply { .. }
                | Action::StashBranch { .. }
        );
        if guard_unsaved && self.dirty_repository(repository_id) {
            return Err(invalid(
                "Save or close unsaved source buffers in this repository before changing its working tree or index",
            ));
        }
        let state = self.open(repository_id, cancel)?;
        if state.revision != revision {
            return Err(invalid(
                "Repository changed; refresh before preparing the action",
            ));
        }
        let repo = self
            .repositories
            .get(repository_id)
            .ok_or_else(|| invalid("Unknown repository"))?;
        if crate::sync::durable::Object::observe(&repo.root, Path::new(""))? != repo.root_identity
            || crate::sync::durable::Object::observe(&repo.git, Path::new(""))? != repo.git_identity
        {
            return Err(invalid("Repository identity changed; scan again"));
        }
        let mut dangerous = false;
        let mut network = false;
        let mut backup_worktree = false;
        let mut backup = false;
        let mut backup_target = None;
        let paths = |ids: &[String]| -> Result<Vec<String>> {
            if ids.is_empty() || ids.len() > 10_000 {
                return Err(invalid("Select between 1 and 10000 changed files"));
            }
            let mut paths = BTreeSet::new();
            for id in ids {
                for path in repo
                    .files
                    .get(id)
                    .ok_or_else(|| invalid("File selection expired; refresh repository"))?
                {
                    paths.insert(
                        path.to_str()
                            .ok_or_else(|| {
                                invalid("This Git write operation requires a Unicode filename")
                            })?
                            .to_string(),
                    );
                }
            }
            Ok(paths.into_iter().collect())
        };
        let mut input = None;
        let mut selection = None;
        let mut followup = None;
        let mut checkout_expectation = None;
        let mut preference_rename = None;
        let args = match action {
            Action::CreateAndSwitch { name, start } => {
                let mut args = vec![
                    "switch".into(),
                    "--create".into(),
                    branch(&repo.root, &name, cancel)?,
                ];
                if let Some(start) = start {
                    args.push(commit(&repo.root, &start, cancel)?);
                }
                args
            }
            Action::CheckoutRemote { reference, name } => {
                let reference = super::branches::reference(&state, &reference)?;
                if reference.kind != "remote" {
                    return Err(invalid("Select a remote branch"));
                }
                super::branches::checkout(
                    &repo.root,
                    &state,
                    &reference.full_name,
                    Some(&name),
                    cancel,
                )?
                .args
            }
            Action::SwitchAndUpdate {
                reference,
                name,
                mode,
            } => {
                let target = super::branches::checkout(
                    &repo.root,
                    &state,
                    &reference,
                    name.as_deref(),
                    cancel,
                )?;
                let upstream = if let Some(remote) = target.remote.clone() {
                    remote
                } else {
                    super::branches::upstream(&repo.root, &state, &target.name, cancel)?
                };
                followup = Some((super::branches::pull(upstream.0, upstream.1, &mode)?, true));
                checkout_expectation = Some((target.name, target.tip.clone()));
                dangerous = mode != "ff_only";
                backup = dangerous;
                backup_target = Some(target.tip);
                target.args
            }
            Action::SwitchAndRebase { reference, name } => {
                let target = super::branches::checkout(
                    &repo.root,
                    &state,
                    &reference,
                    name.as_deref(),
                    cancel,
                )?;
                let original = state
                    .repository
                    .head
                    .clone()
                    .ok_or_else(|| invalid("Current branch has no commit to rebase onto"))?;
                if target.name == state.repository.branch {
                    return Err(invalid("Choose a different branch for checkout and rebase"));
                }
                followup = Some((vec!["rebase".into(), original], false));
                checkout_expectation = Some((target.name, target.tip.clone()));
                dangerous = true;
                backup = true;
                backup_target = Some(target.tip);
                target.args
            }
            Action::UpdateCurrent { mode } => {
                let current = state
                    .references
                    .iter()
                    .find(|r| r.current && r.kind == "local")
                    .ok_or_else(|| invalid("Updating requires a local branch with an upstream"))?;
                let upstream =
                    super::branches::upstream(&repo.root, &state, &current.name, cancel)?;
                network = true;
                dangerous = mode != "ff_only";
                backup = dangerous;
                super::branches::pull(upstream.0, upstream.1, &mode)?
            }
            Action::PushBranch {
                branch: name,
                remote: name_remote,
                target,
                set_upstream,
                force_with_lease,
            } => {
                let name = branch(&repo.root, &name, cancel)?;
                if !state
                    .references
                    .iter()
                    .any(|r| r.kind == "local" && r.name == name)
                {
                    return Err(invalid("Choose an existing local branch to push"));
                }
                let remote = remote(&state, &name_remote)?;
                let target = super::branches::target_name(&repo.root, &target, cancel)?;
                network = true;
                dangerous = force_with_lease;
                let mut args = vec!["push".into()];
                if set_upstream {
                    args.push("--set-upstream".into());
                }
                if force_with_lease {
                    let expected = state
                        .references
                        .iter()
                        .find(|r| r.full_name == format!("refs/remotes/{remote}/{target}"))
                        .map(|r| r.target.as_str())
                        .unwrap_or("");
                    if !expected.is_empty() {
                        backup = true;
                        backup_target = Some(expected.into());
                    }
                    args.push(format!("--force-with-lease=refs/heads/{target}:{expected}"));
                }
                args.extend([remote, format!("refs/heads/{name}:refs/heads/{target}")]);
                args
            }
            Action::StageSelection {
                diff_id,
                hunks,
                lines,
            } => {
                let (patch, guard, old_blob) = self.prepare_selection(
                    repository_id,
                    &state.revision,
                    &diff_id,
                    &hunks,
                    &lines,
                    cancel,
                )?;
                input = Some(patch);
                selection = Some(guard);
                backup = old_blob.is_some() || state.repository.head.is_some();
                backup_target = old_blob;
                let mut args = vec![
                    "apply".into(),
                    "--cached".into(),
                    "--recount".into(),
                    "--unidiff-zero".into(),
                    "--whitespace=nowarn".into(),
                ];
                if self.selection_reversed(&diff_id) {
                    args.push("--reverse".into());
                }
                args
            }
            Action::Stage { files } => {
                let mut a = vec!["add".into(), "--".into()];
                a.extend(paths(&files)?);
                a
            }
            Action::Unstage { files } => {
                let mut a = if state.repository.head.is_some() {
                    vec!["restore".into(), "--staged".into(), "--".into()]
                } else {
                    vec![
                        "rm".into(),
                        "--cached".into(),
                        "--ignore-unmatch".into(),
                        "--".into(),
                    ]
                };
                a.extend(paths(&files)?);
                a
            }
            Action::Discard { files } => {
                for id in &files {
                    if state
                        .changes
                        .iter()
                        .find(|c| c.id == *id)
                        .is_some_and(|c| c.untracked || c.conflicted)
                    {
                        return Err(invalid(
                            "Discard accepts tracked, non-conflicted files only",
                        ));
                    }
                }
                dangerous = true;
                backup = true;
                backup_worktree = true;
                let mut a = vec!["restore".into(), "--worktree".into(), "--".into()];
                a.extend(paths(&files)?);
                a
            }
            Action::Commit {
                message: msg,
                amend,
            } => {
                if state.operation.as_deref() == Some("rebase") {
                    return Err(invalid("Use Continue to complete a paused rebase"));
                }
                backup = amend;
                dangerous = amend;
                let mut a = vec!["commit".into(), "-m".into(), message(&msg)?];
                if amend {
                    a.push("--amend".into());
                }
                a
            }
            Action::CreateBranch { name, start } => {
                let mut a = vec!["branch".into(), branch(&repo.root, &name, cancel)?];
                if let Some(start) = start {
                    a.push(commit(&repo.root, &start, cancel)?);
                }
                a
            }
            Action::Switch { reference, detach } => {
                if detach {
                    vec![
                        "switch".into(),
                        "--detach".into(),
                        commit(&repo.root, &reference, cancel)?,
                    ]
                } else {
                    let name = branch(&repo.root, &reference, cancel)?;
                    if !state
                        .references
                        .iter()
                        .any(|r| r.kind == "local" && r.name == name)
                    {
                        return Err(invalid(
                            "Select an existing local branch; remote branches use explicit tracking checkout",
                        ));
                    }
                    vec!["switch".into(), "--no-guess".into(), name]
                }
            }
            Action::RenameBranch { name, new_name } => {
                let name = branch(&repo.root, &name, cancel)?;
                let new_name = branch(&repo.root, &new_name, cancel)?;
                preference_rename = Some((
                    format!("refs/heads/{name}"),
                    format!("refs/heads/{new_name}"),
                ));
                vec!["branch".into(), "-m".into(), name, new_name]
            }
            Action::DeleteBranch { name, force } => {
                dangerous = force;
                backup = force;
                if force {
                    backup_target = Some(commit(
                        &repo.root,
                        &format!("refs/heads/{}", branch(&repo.root, &name, cancel)?),
                        cancel,
                    )?);
                }
                vec![
                    "branch".into(),
                    if force { "-D" } else { "-d" }.into(),
                    branch(&repo.root, &name, cancel)?,
                ]
            }
            Action::SetUpstream {
                branch: name,
                upstream,
            } => {
                let name = branch(&repo.root, &name, cancel)?;
                if let Some(upstream) = upstream {
                    vec![
                        "branch".into(),
                        format!("--set-upstream-to={}", atom(&upstream)?),
                        name,
                    ]
                } else {
                    vec!["branch".into(), "--unset-upstream".into(), name]
                }
            }
            Action::Merge { reference } => vec![
                "merge".into(),
                "--no-edit".into(),
                commit(&repo.root, &reference, cancel)?,
            ],
            Action::Rebase { reference } => {
                dangerous = true;
                backup = true;
                vec!["rebase".into(), commit(&repo.root, &reference, cancel)?]
            }
            Action::CherryPick { reference } => vec![
                "cherry-pick".into(),
                commit(&repo.root, &reference, cancel)?,
            ],
            Action::Revert { reference } => vec![
                "revert".into(),
                "--no-edit".into(),
                commit(&repo.root, &reference, cancel)?,
            ],
            Action::Reset { reference, mode } => {
                let flag = match mode.as_str() {
                    "soft" => "--soft",
                    "mixed" => "--mixed",
                    "hard" => {
                        if state.changes.iter().any(|c| c.untracked || c.conflicted) {
                            return Err(invalid(
                                "Hard reset requires untracked files and conflicts to be handled first; their contents cannot be retained in this recovery snapshot",
                            ));
                        }
                        backup_worktree = true;
                        "--hard"
                    }
                    _ => return Err(invalid("Unknown reset mode")),
                };
                dangerous = true;
                backup = true;
                vec![
                    "reset".into(),
                    flag.into(),
                    commit(&repo.root, &reference, cancel)?,
                ]
            }
            Action::Continue { operation } => continuation(&state, &operation, "--continue")?,
            Action::Abort { operation } => {
                dangerous = true;
                backup = true;
                backup_worktree = state.repository.conflicts == 0;
                continuation(&state, &operation, "--abort")?
            }
            Action::Skip { operation } => continuation(&state, &operation, "--skip")?,
            Action::Fetch {
                remote: name,
                prune,
            } => {
                network = true;
                let mut a = vec!["fetch".into()];
                if prune {
                    a.push("--prune".into());
                }
                if let Some(name) = name {
                    a.push(remote(&state, &name)?);
                } else {
                    a.push("--all".into());
                }
                a
            }
            Action::Pull {
                remote: name,
                branch: name_branch,
                mode,
            } => {
                network = true;
                let flag = match mode.as_str() {
                    "ff_only" => "--ff-only",
                    "merge" => "--no-rebase",
                    "rebase" => "--rebase",
                    _ => return Err(invalid("Unknown pull mode")),
                };
                vec![
                    "pull".into(),
                    flag.into(),
                    remote(&state, &name)?,
                    branch(&repo.root, &name_branch, cancel)?,
                ]
            }
            Action::Push {
                remote: name,
                branch: name_branch,
                set_upstream,
                force_with_lease,
            } => {
                network = true;
                dangerous = force_with_lease;
                let mut a = vec!["push".into()];
                if set_upstream {
                    a.push("--set-upstream".into());
                }
                if force_with_lease {
                    a.push("--force-with-lease".into());
                }
                a.push(remote(&state, &name)?);
                a.push(branch(&repo.root, &name_branch, cancel)?);
                a
            }
            Action::StashUse {
                id,
                operation,
                name,
                restore_index,
            } => {
                let entry = self.stash_selection(repository_id, &id, cancel)?;
                if state.repository.kind == "bare" {
                    return Err(invalid("Stash mutations require a working tree"));
                }
                dangerous = true;
                backup = true;
                backup_target = Some(entry.object.clone());
                let selector = format!("stash@{{{}}}", entry.position);
                match operation {
                    StashOperation::Apply | StashOperation::Pop => {
                        let pop = matches!(operation, StashOperation::Pop);
                        let mut args = vec![
                            "stash".into(),
                            if pop { "pop".into() } else { "apply".into() },
                        ];
                        if restore_index {
                            args.push("--index".into());
                        }
                        args.push(if pop { selector } else { entry.object });
                        args
                    }
                    StashOperation::Drop => vec!["stash".into(), "drop".into(), selector],
                    StashOperation::Branch => vec![
                        "stash".into(),
                        "branch".into(),
                        branch(
                            &repo.root,
                            &name.ok_or_else(|| invalid("Enter a new branch name"))?,
                            cancel,
                        )?,
                        selector,
                    ],
                }
            }
            Action::StashSave {
                message: msg,
                include_untracked,
            } => {
                let mut a = vec!["stash".into(), "push".into(), "-m".into(), message(&msg)?];
                if include_untracked {
                    a.push("--include-untracked".into());
                }
                a
            }
            Action::StashApply { index, pop } => {
                let sha = stash(&repo.root, index, cancel)?;
                if pop {
                    vec!["stash".into(), "pop".into(), format!("stash@{{{index}}}")]
                } else {
                    vec!["stash".into(), "apply".into(), sha]
                }
            }
            Action::StashDrop { index } => {
                backup_target = Some(stash(&repo.root, index, cancel)?);
                dangerous = true;
                backup = true;
                vec!["stash".into(), "drop".into(), format!("stash@{{{index}}}")]
            }
            Action::StashBranch { name, index } => {
                let _ = stash(&repo.root, index, cancel)?;
                vec![
                    "stash".into(),
                    "branch".into(),
                    branch(&repo.root, &name, cancel)?,
                    format!("stash@{{{index}}}"),
                ]
            }
            Action::CreateTag {
                name,
                reference,
                message: msg,
            } => {
                let name = branch(&repo.root, &name, cancel)?;
                let target = commit(&repo.root, &reference, cancel)?;
                if let Some(msg) = msg {
                    vec![
                        "tag".into(),
                        "-a".into(),
                        name,
                        "-m".into(),
                        message(&msg)?,
                        target,
                    ]
                } else {
                    vec!["tag".into(), name, target]
                }
            }
            Action::DeleteTag { name } => {
                dangerous = true;
                backup = true;
                let name = branch(&repo.root, &name, cancel)?;
                backup_target = Some(runner::text(
                    &repo.root,
                    &[
                        "rev-parse",
                        "--verify",
                        "--end-of-options",
                        &format!("refs/tags/{name}"),
                    ],
                    cancel,
                )?);
                vec![
                    "tag".into(),
                    "-d".into(),
                    branch(&repo.root, &name, cancel)?,
                ]
            }
            Action::PushTag {
                remote: name,
                name: tag,
            } => {
                network = true;
                let tag = branch(&repo.root, &tag, cancel)?;
                vec![
                    "push".into(),
                    remote(&state, &name)?,
                    format!("refs/tags/{tag}:refs/tags/{tag}"),
                ]
            }
            Action::AddRemote { name, url } => {
                vec!["remote".into(), "add".into(), atom(&name)?, atom(&url)?]
            }
            Action::SetRemote { name, url, push } => {
                let mut a = vec!["remote".into(), "set-url".into()];
                if push {
                    a.push("--push".into());
                }
                a.push(remote(&state, &name)?);
                a.push(atom(&url)?);
                a
            }
            Action::RemoveRemote { name } => {
                dangerous = true;
                vec!["remote".into(), "remove".into(), remote(&state, &name)?]
            }
        };
        if args.iter().map(String::len).sum::<usize>() > 200_000 {
            return Err(invalid("Git action exceeds argument limit"));
        }
        let mut displayed_args = args.clone();
        if let Some((next, _)) = &followup {
            displayed_args.push("→ git".into());
            displayed_args.extend(next.iter().cloned());
        }
        if displayed_args.iter().map(String::len).sum::<usize>() > 200_000 {
            return Err(invalid("Combined Git action exceeds argument limit"));
        }
        let token = crate::unique_id();
        let view = ActionPlan {
            id: token.clone(),
            repository_id: repository_id.into(),
            revision: state.revision,
            arguments: displayed_args.iter().map(|a| runner::redact(a)).collect(),
            dangerous,
            backup_reference: if backup {
                Some(format!("refs/clear-compare/backups/{token}"))
            } else {
                None
            },
        };
        self.pending = Some(Prepared {
            view: view.clone(),
            args,
            network,
            backup_worktree,
            backup_target,
            input,
            selection,
            guard_unsaved,
            followup,
            checkout_expectation,
            preference_rename,
        });
        Ok(view)
    }
    pub fn apply(&mut self, plan_id: &str, cancel: &AtomicBool) -> Result<ActionResult> {
        let plan = self
            .pending
            .take()
            .ok_or_else(|| invalid("Git action plan expired"))?;
        if plan.view.id != plan_id {
            return Err(invalid("Git action plan identity differs"));
        }
        if plan.guard_unsaved && self.dirty_repository(&plan.view.repository_id) {
            return Err(invalid(
                "Save or close unsaved source buffers before applying this Git operation",
            ));
        }
        let state = self.open(&plan.view.repository_id, cancel)?;
        if state.revision != plan.view.revision {
            return Err(invalid(
                "Repository changed after review; refresh and prepare again",
            ));
        }
        let repo = self
            .repositories
            .get(&plan.view.repository_id)
            .ok_or_else(|| invalid("Unknown repository"))?;
        if crate::sync::durable::Object::observe(&repo.root, Path::new(""))? != repo.root_identity
            || crate::sync::durable::Object::observe(&repo.git, Path::new(""))? != repo.git_identity
        {
            return Err(invalid("Repository identity changed; scan again"));
        }
        if let Some(selection) = &plan.selection {
            selection.check(&repo.root, cancel)?;
        }
        if let Some(reference) = &plan.view.backup_reference {
            let target = if plan.backup_worktree {
                let created = runner::text(
                    &repo.root,
                    &["stash", "create", "Clear Compare safety snapshot"],
                    cancel,
                )?;
                if created.is_empty() {
                    state.repository.head.clone()
                } else {
                    Some(created)
                }
            } else {
                plan.backup_target
                    .clone()
                    .or_else(|| state.repository.head.clone())
            };
            if let Some(target) = target {
                runner::mutate(
                    &repo.root,
                    &["update-ref".into(), reference.clone(), target],
                    cancel,
                    false,
                )?;
            }
        }
        let output = if let Some(input) = &plan.input {
            runner::mutate_input(&repo.root, &plan.args, input, cancel)?
        } else {
            runner::mutate(&repo.root, &plan.args, cancel, plan.network)?
        };
        let mut output = runner::redact(&String::from_utf8_lossy(&output));
        if let Some((next, network)) = &plan.followup {
            let next_result = (|| -> Result<Vec<u8>> {
                if let Some((name, tip)) = &plan.checkout_expectation
                    && (runner::text(
                        &repo.root,
                        &["symbolic-ref", "--short", "-q", "HEAD"],
                        cancel,
                    )? != *name
                        || runner::text(&repo.root, &["rev-parse", "--verify", "HEAD"], cancel)?
                            != *tip)
                {
                    return Err(invalid(
                        "Branch or HEAD changed after checkout; follow-up was not run",
                    ));
                }
                runner::mutate(&repo.root, next, cancel, *network)
            })();
            match next_result {
                Ok(bytes) => {
                    output.push('\n');
                    output.push_str(&runner::redact(&String::from_utf8_lossy(&bytes)));
                }
                Err(error) => {
                    return Err(invalid(&format!(
                        "Checkout completed; follow-up failed and the current Git state was retained: {error}"
                    )));
                }
            }
        }
        if let Some((old, new)) = &plan.preference_rename {
            self.rename_reference_preference(&plan.view.repository_id, old, new);
        }
        let state = self.open(&plan.view.repository_id, cancel)?;
        Ok(ActionResult {
            output,
            backup_reference: plan.view.backup_reference,
            state,
        })
    }
}
