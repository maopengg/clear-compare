use clear_contracts::workbench::{Action, ActionResult, ScanSettings};
use clear_core::workbench::Workspace;
use std::{fs, path::Path, process::Command, sync::atomic::AtomicBool};
const CANCEL: AtomicBool = AtomicBool::new(false);
fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().into()
}
fn fixture() -> (tempfile::TempDir, Workspace, String) {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    git(dir.path(), &["config", "user.name", "Action Test"]);
    git(dir.path(), &["config", "user.email", "actions@example.com"]);
    git(dir.path(), &["config", "commit.gpgsign", "false"]);
    fs::write(dir.path().join("file.txt"), "initial\n").unwrap();
    git(dir.path(), &["add", "file.txt"]);
    git(dir.path(), &["commit", "-m", "initial"]);
    let mut w = Workspace::default();
    let c = w
        .scan(
            ScanSettings {
                roots: vec![dir.path().display().to_string()],
                exclusions: vec![],
            },
            &CANCEL,
        )
        .unwrap();
    let id = c.repositories[0].id.clone();
    (dir, w, id)
}
fn execute(w: &mut Workspace, id: &str, action: Action) -> ActionResult {
    let s = w.open(id, &CANCEL).unwrap();
    let p = w.prepare(id, &s.revision, action, &CANCEL).unwrap();
    w.apply(&p.id, &CANCEL).unwrap()
}
#[test]
fn stages_unstages_commits_and_switches_using_captured_ids() {
    let (dir, mut w, id) = fixture();
    fs::write(dir.path().join("file.txt"), "modified\n").unwrap();
    let s = w.open(&id, &CANCEL).unwrap();
    let file = s.changes[0].id.clone();
    let r = execute(
        &mut w,
        &id,
        Action::Stage {
            files: vec![file.clone()],
        },
    );
    assert_eq!(r.state.changes[0].index, "M");
    let r = execute(
        &mut w,
        &id,
        Action::Unstage {
            files: vec![file.clone()],
        },
    );
    assert_eq!(r.state.changes[0].index, ".");
    execute(&mut w, &id, Action::Stage { files: vec![file] });
    let r = execute(
        &mut w,
        &id,
        Action::Commit {
            message: "second\n\nDetails".into(),
            amend: false,
        },
    );
    assert!(r.state.changes.is_empty());
    assert_eq!(git(dir.path(), &["log", "-1", "--format=%s"]), "second");
    execute(
        &mut w,
        &id,
        Action::CreateBranch {
            name: "feature/test".into(),
            start: None,
        },
    );
    let r = execute(
        &mut w,
        &id,
        Action::Switch {
            reference: "feature/test".into(),
            detach: false,
        },
    );
    assert_eq!(r.state.repository.branch, "feature/test");
}
#[test]
fn rejects_stale_files_and_replayed_plans_without_discarding_new_edits() {
    let (dir, mut w, id) = fixture();
    fs::write(dir.path().join("file.txt"), "first edit\n").unwrap();
    let s = w.open(&id, &CANCEL).unwrap();
    let p = w
        .prepare(
            &id,
            &s.revision,
            Action::Discard {
                files: vec![s.changes[0].id.clone()],
            },
            &CANCEL,
        )
        .unwrap();
    fs::write(dir.path().join("file.txt"), "new external contents\n").unwrap();
    assert!(w.apply(&p.id, &CANCEL).is_err());
    assert_eq!(
        fs::read_to_string(dir.path().join("file.txt")).unwrap(),
        "new external contents\n"
    );
    assert!(w.apply(&p.id, &CANCEL).is_err());
    let s = w.open(&id, &CANCEL).unwrap();
    assert!(
        w.prepare(
            &id,
            &s.revision,
            Action::Stage {
                files: vec!["foreign-file".into()]
            },
            &CANCEL
        )
        .is_err()
    );
    assert!(
        w.prepare(
            &id,
            &s.revision,
            Action::CreateBranch {
                name: "--force".into(),
                start: None
            },
            &CANCEL
        )
        .is_err()
    );
}
#[test]
fn destructive_actions_preserve_the_actual_deleted_objects_and_worktree() {
    let (dir, mut w, id) = fixture();
    git(dir.path(), &["switch", "-c", "unique"]);
    fs::write(dir.path().join("file.txt"), "unique commit\n").unwrap();
    git(dir.path(), &["commit", "-am", "unique"]);
    let unique = git(dir.path(), &["rev-parse", "HEAD"]);
    git(dir.path(), &["tag", "-a", "annotated", "-m", "tag"]);
    let tag = git(dir.path(), &["rev-parse", "refs/tags/annotated"]);
    git(dir.path(), &["switch", "main"]);
    let r = execute(
        &mut w,
        &id,
        Action::DeleteBranch {
            name: "unique".into(),
            force: true,
        },
    );
    assert_eq!(
        git(
            dir.path(),
            &["rev-parse", r.backup_reference.as_ref().unwrap()]
        ),
        unique
    );
    let r = execute(
        &mut w,
        &id,
        Action::DeleteTag {
            name: "annotated".into(),
        },
    );
    assert_eq!(
        git(
            dir.path(),
            &["rev-parse", r.backup_reference.as_ref().unwrap()]
        ),
        tag
    );
    fs::write(dir.path().join("file.txt"), "unsaved work\n").unwrap();
    let s = w.open(&id, &CANCEL).unwrap();
    let r = execute(
        &mut w,
        &id,
        Action::Discard {
            files: vec![s.changes[0].id.clone()],
        },
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("file.txt")).unwrap(),
        "initial\n"
    );
    assert_eq!(
        git(
            dir.path(),
            &["show", &format!("{}:file.txt", r.backup_reference.unwrap())]
        ),
        "unsaved work"
    );
    fs::write(dir.path().join("file.txt"), "stash work\n").unwrap();
    execute(
        &mut w,
        &id,
        Action::StashSave {
            message: "keep".into(),
            include_untracked: false,
        },
    );
    let stash = git(dir.path(), &["rev-parse", "stash@{0}"]);
    let r = execute(&mut w, &id, Action::StashDrop { index: 0 });
    assert_eq!(
        git(
            dir.path(),
            &["rev-parse", r.backup_reference.as_ref().unwrap()]
        ),
        stash
    );
}
#[test]
fn stash_changes_invalidate_positional_actions_and_hooks_remain_visible() {
    let (dir, mut w, id) = fixture();
    fs::write(dir.path().join("file.txt"), "stash one\n").unwrap();
    execute(
        &mut w,
        &id,
        Action::StashSave {
            message: "one".into(),
            include_untracked: false,
        },
    );
    let s = w.open(&id, &CANCEL).unwrap();
    let p = w
        .prepare(&id, &s.revision, Action::StashDrop { index: 0 }, &CANCEL)
        .unwrap();
    fs::write(dir.path().join("file.txt"), "stash two\n").unwrap();
    git(dir.path(), &["stash", "push", "-m", "two"]);
    assert!(w.apply(&p.id, &CANCEL).is_err());
    assert_eq!(
        git(dir.path(), &["stash", "list", "--format=%s"])
            .lines()
            .count(),
        2
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let hook = dir.path().join(".git/hooks/pre-commit");
        fs::write(
            &hook,
            "#!/bin/sh\necho 'hook validation failed' >&2\nexit 1\n",
        )
        .unwrap();
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(dir.path().join("file.txt"), "new commit\n").unwrap();
        git(dir.path(), &["add", "file.txt"]);
        let s = w.open(&id, &CANCEL).unwrap();
        let p = w
            .prepare(
                &id,
                &s.revision,
                Action::Commit {
                    message: "blocked".into(),
                    amend: false,
                },
                &CANCEL,
            )
            .unwrap();
        let error = w.apply(&p.id, &CANCEL).unwrap_err().to_string();
        assert!(error.contains("hook validation failed"));
        assert_eq!(git(dir.path(), &["log", "-1", "--format=%s"]), "initial");
    }
}
#[test]
fn merge_conflicts_can_be_resolved_and_continued_or_aborted() {
    let (dir, mut w, id) = fixture();
    git(dir.path(), &["switch", "-c", "other"]);
    fs::write(dir.path().join("file.txt"), "other\n").unwrap();
    git(dir.path(), &["commit", "-am", "other"]);
    git(dir.path(), &["switch", "main"]);
    fs::write(dir.path().join("file.txt"), "main\n").unwrap();
    git(dir.path(), &["commit", "-am", "main"]);
    let s = w.open(&id, &CANCEL).unwrap();
    let p = w
        .prepare(
            &id,
            &s.revision,
            Action::Merge {
                reference: "other".into(),
            },
            &CANCEL,
        )
        .unwrap();
    assert!(w.apply(&p.id, &CANCEL).is_err());
    let s = w.open(&id, &CANCEL).unwrap();
    assert_eq!(s.operation.as_deref(), Some("merge"));
    assert!(
        w.prepare(
            &id,
            &s.revision,
            Action::Continue {
                operation: "merge".into()
            },
            &CANCEL
        )
        .is_err()
    );
    assert!(
        w.prepare(
            &id,
            &s.revision,
            Action::Skip {
                operation: "merge".into()
            },
            &CANCEL
        )
        .is_err()
    );
    execute(
        &mut w,
        &id,
        Action::Abort {
            operation: "merge".into(),
        },
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("file.txt")).unwrap(),
        "main\n"
    );
    let s = w.open(&id, &CANCEL).unwrap();
    let p = w
        .prepare(
            &id,
            &s.revision,
            Action::Merge {
                reference: "other".into(),
            },
            &CANCEL,
        )
        .unwrap();
    assert!(w.apply(&p.id, &CANCEL).is_err());
    fs::write(dir.path().join("file.txt"), "resolved\n").unwrap();
    let s = w.open(&id, &CANCEL).unwrap();
    execute(
        &mut w,
        &id,
        Action::Stage {
            files: vec![s.changes[0].id.clone()],
        },
    );
    let r = execute(
        &mut w,
        &id,
        Action::Continue {
            operation: "merge".into(),
        },
    );
    assert!(r.state.operation.is_none());
    assert_eq!(git(dir.path(), &["show", "HEAD:file.txt"]), "resolved");
}

#[test]
fn fetch_push_and_pull_use_existing_local_remote_configuration() {
    let (dir, mut w, id) = fixture();
    let remote = tempfile::tempdir().unwrap();
    git(remote.path(), &["init", "--bare", "-b", "main"]);
    execute(
        &mut w,
        &id,
        Action::AddRemote {
            name: "origin".into(),
            url: remote.path().display().to_string(),
        },
    );
    execute(
        &mut w,
        &id,
        Action::Push {
            remote: "origin".into(),
            branch: "main".into(),
            set_upstream: true,
            force_with_lease: false,
        },
    );
    assert_eq!(
        git(remote.path(), &["rev-parse", "main"]),
        git(dir.path(), &["rev-parse", "main"])
    );
    let other = tempfile::tempdir().unwrap();
    git(
        other.path(),
        &["clone", remote.path().to_str().unwrap(), "."],
    );
    git(other.path(), &["config", "user.name", "Other Test"]);
    git(other.path(), &["config", "user.email", "other@example.com"]);
    git(other.path(), &["config", "commit.gpgsign", "false"]);
    fs::write(other.path().join("remote.txt"), "remote update\n").unwrap();
    git(other.path(), &["add", "."]);
    git(other.path(), &["commit", "-m", "remote update"]);
    git(other.path(), &["push"]);
    let r = execute(
        &mut w,
        &id,
        Action::Fetch {
            remote: None,
            prune: true,
        },
    );
    assert_eq!(r.state.repository.behind, 1);
    execute(
        &mut w,
        &id,
        Action::Pull {
            remote: "origin".into(),
            branch: "main".into(),
            mode: "ff_only".into(),
        },
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("remote.txt")).unwrap(),
        "remote update\n"
    );
}

#[test]
fn reset_keeps_a_recovery_reference_and_cancellation_never_starts_the_action() {
    let (dir, mut w, id) = fixture();
    let first = git(dir.path(), &["rev-parse", "HEAD"]);
    fs::write(dir.path().join("file.txt"), "second\n").unwrap();
    git(dir.path(), &["commit", "-am", "second"]);
    let second = git(dir.path(), &["rev-parse", "HEAD"]);
    let r = execute(
        &mut w,
        &id,
        Action::Reset {
            reference: first,
            mode: "soft".into(),
        },
    );
    assert_eq!(
        git(
            dir.path(),
            &["rev-parse", r.backup_reference.as_ref().unwrap()]
        ),
        second
    );
    assert_eq!(r.state.changes[0].index, "M");
    let s = w.open(&id, &CANCEL).unwrap();
    let p = w
        .prepare(
            &id,
            &s.revision,
            Action::CreateBranch {
                name: "cancelled".into(),
                start: None,
            },
            &CANCEL,
        )
        .unwrap();
    assert!(w.apply(&p.id, &AtomicBool::new(true)).is_err());
    assert!(git(dir.path(), &["branch", "--list", "cancelled"]).is_empty());
}

#[test]
fn hard_reset_refuses_untracked_data_until_it_can_be_preserved() {
    let (dir, mut w, id) = fixture();
    fs::write(dir.path().join("untracked.txt"), "keep me\n").unwrap();
    let s = w.open(&id, &CANCEL).unwrap();
    assert!(
        w.prepare(
            &id,
            &s.revision,
            Action::Reset {
                reference: "HEAD".into(),
                mode: "hard".into()
            },
            &CANCEL
        )
        .is_err()
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("untracked.txt")).unwrap(),
        "keep me\n"
    );
}

#[test]
fn previous_branch_shorthand_is_expanded_before_execution() {
    let (dir, mut w, id) = fixture();
    git(dir.path(), &["switch", "-c", "feature"]);
    git(dir.path(), &["switch", "main"]);
    let s = w.open(&id, &CANCEL).unwrap();
    let p = w
        .prepare(
            &id,
            &s.revision,
            Action::Switch {
                reference: "@{-1}".into(),
                detach: false,
            },
            &CANCEL,
        )
        .unwrap();
    assert_eq!(p.arguments, vec!["switch", "feature"]);
    assert_eq!(
        w.apply(&p.id, &CANCEL).unwrap().state.repository.branch,
        "feature"
    );
}
