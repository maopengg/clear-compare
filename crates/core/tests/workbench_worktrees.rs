use clear_contracts::workbench::*;
use clear_core::workbench::Workspace;
use std::{fs, path::Path, process::Command, sync::atomic::AtomicBool};
static CANCEL: AtomicBool = AtomicBool::new(false);
fn git(root: &Path, args: &[&str]) -> String {
    let r = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        r.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    String::from_utf8_lossy(&r.stdout).trim().into()
}
fn fixture() -> (tempfile::TempDir, Workspace, String) {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("main");
    fs::create_dir(&root).unwrap();
    git(&root, &["init", "-b", "main"]);
    git(&root, &["config", "user.name", "Worktree Test"]);
    git(&root, &["config", "user.email", "worktrees@example.com"]);
    git(&root, &["config", "commit.gpgsign", "false"]);
    fs::write(root.join("tracked.txt"), "original\n").unwrap();
    fs::write(root.join(".gitignore"), "ignored.txt\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "initial"]);
    let mut w = Workspace::default();
    let c = w
        .scan(
            ScanSettings {
                roots: vec![root.display().to_string()],
                exclusions: vec![],
            },
            &CANCEL,
        )
        .unwrap();
    let id = c.repositories[0].id.clone();
    (t, w, id)
}
fn plan(w: &mut Workspace, id: &str, a: Action) -> ActionPlan {
    let s = w.open(id, &CANCEL).unwrap();
    w.prepare(id, &s.revision, a, &CANCEL).unwrap()
}
fn apply(w: &mut Workspace, id: &str, a: Action) -> ActionResult {
    let p = plan(w, id, a);
    assert!(p.dangerous);
    w.apply(&p.id, &CANCEL).unwrap()
}
fn create(
    w: &mut Workspace,
    id: &str,
    parent: &Path,
    folder: &str,
    branch: Option<&str>,
    detach: bool,
) -> ActionResult {
    let parent = fs::canonicalize(parent).unwrap();
    let d = w
        .worktree_destination(id, parent.to_str().unwrap(), &CANCEL)
        .unwrap();
    apply(
        w,
        id,
        Action::CreateWorktree {
            destination_id: d.id,
            folder: folder.into(),
            reference_id: None,
            new_branch: branch.map(String::from),
            detach,
        },
    )
}
fn linked(w: &mut Workspace, id: &str) -> WorktreeEntry {
    w.worktrees(id, &CANCEL)
        .unwrap()
        .entries
        .into_iter()
        .find(|e| !e.main)
        .unwrap()
}
#[test]
fn create_open_lock_unlock_remove_preserves_commit_and_journal() {
    let (t, mut w, id) = fixture();
    let result = create(
        &mut w,
        &id,
        t.path(),
        "linked space",
        Some("feature"),
        false,
    );
    let journal = result.recovery_path.unwrap();
    assert!(Path::new(&journal).join("before.json").exists());
    assert_eq!(
        fs::read_to_string(Path::new(&journal).join("result.json")).unwrap(),
        "{\"success\":true}"
    );
    let e = linked(&mut w, &id);
    assert_eq!(e.branch.as_deref(), Some("refs/heads/feature"));
    assert!(e.available);
    assert!(!e.current);
    let opened = w.open_worktree(&id, &e.id, &CANCEL).unwrap();
    assert_eq!(
        opened.state.repository.common_id,
        result.state.repository.common_id
    );
    assert_eq!(opened.state.repository.kind, "worktree");
    assert!(
        opened
            .catalog
            .repositories
            .iter()
            .any(|r| r.id == opened.state.repository.id)
    );
    apply(
        &mut w,
        &id,
        Action::SetWorktreeLock {
            id: e.id.clone(),
            reason: Some("keep this checkout".into()),
        },
    );
    let e = linked(&mut w, &id);
    assert_eq!(e.locked.as_deref(), Some("keep this checkout"));
    let s = w.open(&id, &CANCEL).unwrap();
    assert!(
        w.prepare(
            &id,
            &s.revision,
            Action::RemoveWorktree { id: e.id.clone() },
            &CANCEL
        )
        .is_err()
    );
    apply(
        &mut w,
        &id,
        Action::SetWorktreeLock {
            id: e.id,
            reason: None,
        },
    );
    let e = linked(&mut w, &id);
    let p = plan(&mut w, &id, Action::RemoveWorktree { id: e.id });
    let recovery = p.backup_reference.clone().unwrap();
    let result = w.apply(&p.id, &CANCEL).unwrap();
    assert!(!t.path().join("linked space").exists());
    assert_eq!(
        git(&t.path().join("main"), &["rev-parse", &recovery]),
        e.head.unwrap()
    );
    assert!(result.recovery_path.is_some());
    assert!(w.apply(&p.id, &CANCEL).is_err());
    assert_eq!(w.worktrees(&id, &CANCEL).unwrap().entries.len(), 1);
}
#[test]
fn creation_modes_and_reference_ids_are_frozen() {
    let (t, mut w, id) = fixture();
    create(&mut w, &id, t.path(), "detached", None, true);
    let e = linked(&mut w, &id);
    assert!(e.detached);
    assert!(e.branch.is_none());
    git(&t.path().join("main"), &["branch", "existing"]);
    let s = w.open(&id, &CANCEL).unwrap();
    let r = s
        .references
        .iter()
        .find(|r| r.name == "existing")
        .unwrap()
        .id
        .clone();
    let d = w
        .worktree_destination(&id, t.path().to_str().unwrap(), &CANCEL)
        .unwrap();
    apply(
        &mut w,
        &id,
        Action::CreateWorktree {
            destination_id: d.id,
            folder: "existing".into(),
            reference_id: Some(r),
            new_branch: None,
            detach: false,
        },
    );
    assert_eq!(
        git(&t.path().join("existing"), &["symbolic-ref", "HEAD"]),
        "refs/heads/existing"
    );
    let s = w.open(&id, &CANCEL).unwrap();
    let d = w
        .worktree_destination(&id, t.path().to_str().unwrap(), &CANCEL)
        .unwrap();
    assert!(
        w.prepare(
            &id,
            &s.revision,
            Action::CreateWorktree {
                destination_id: d.id,
                folder: "invalid".into(),
                reference_id: Some("forged".into()),
                new_branch: None,
                detach: true
            },
            &CANCEL
        )
        .is_err()
    );
}
#[test]
fn removal_refuses_changes_untracked_ignored_and_open_tabs_and_rechecks_after_review() {
    let (t, mut w, id) = fixture();
    create(&mut w, &id, t.path(), "linked", None, true);
    let root = t.path().join("linked");
    for (name, bytes) in [
        ("tracked.txt", "changed"),
        ("untracked.txt", "new"),
        ("ignored.txt", "private"),
    ] {
        fs::write(root.join(name), bytes).unwrap();
        let e = linked(&mut w, &id);
        let s = w.open(&id, &CANCEL).unwrap();
        assert!(
            w.prepare(
                &id,
                &s.revision,
                Action::RemoveWorktree { id: e.id },
                &CANCEL
            )
            .is_err()
        );
        if name == "tracked.txt" {
            fs::write(root.join(name), "original\n").unwrap()
        } else {
            fs::remove_file(root.join(name)).unwrap()
        }
    }
    let e = linked(&mut w, &id);
    let opened = w.open_worktree(&id, &e.id, &CANCEL).unwrap();
    let other = opened.state.repository.id;
    let f = w
        .file_tree(&other, None, 0, &CANCEL)
        .unwrap()
        .files
        .into_iter()
        .find(|f| f.name == "tracked.txt")
        .unwrap();
    let doc = w.source_open(&other, &f.id, None, &CANCEL).unwrap();
    let s = w.open(&id, &CANCEL).unwrap();
    assert!(
        w.prepare(
            &id,
            &s.revision,
            Action::RemoveWorktree { id: e.id.clone() },
            &CANCEL
        )
        .is_err()
    );
    w.source_close(&doc.document.id, false).unwrap();
    let p = plan(&mut w, &id, Action::RemoveWorktree { id: e.id });
    fs::write(root.join("ignored.txt"), "arrived later").unwrap();
    assert!(w.apply(&p.id, &CANCEL).is_err());
    assert_eq!(
        fs::read_to_string(root.join("ignored.txt")).unwrap(),
        "arrived later"
    );
}
#[test]
fn prune_only_missing_unlocked_registrations_and_stale_plans_are_rejected() {
    let (t, mut w, id) = fixture();
    create(&mut w, &id, t.path(), "missing", None, true);
    create(&mut w, &id, t.path(), "locked", None, true);
    git(
        &t.path().join("main"),
        &[
            "worktree",
            "lock",
            t.path().join("locked").to_str().unwrap(),
        ],
    );
    fs::remove_dir_all(t.path().join("missing")).unwrap();
    fs::remove_dir_all(t.path().join("locked")).unwrap();
    let list = w.worktrees(&id, &CANCEL).unwrap();
    assert!(list.entries.iter().any(|e| e.prunable.is_some()));
    let p = plan(&mut w, &id, Action::PruneWorktrees);
    git(
        &t.path().join("main"),
        &[
            "worktree",
            "unlock",
            t.path().join("locked").to_str().unwrap(),
        ],
    );
    assert!(w.apply(&p.id, &CANCEL).is_err());
    git(
        &t.path().join("main"),
        &[
            "worktree",
            "lock",
            t.path().join("locked").to_str().unwrap(),
        ],
    );
    apply(&mut w, &id, Action::PruneWorktrees);
    let list = w.worktrees(&id, &CANCEL).unwrap();
    assert_eq!(list.entries.len(), 2);
    assert!(list.entries.iter().any(|e| e.locked.is_some()));
    assert!(!list.entries.iter().any(|e| e.path.ends_with("missing")));
}
#[test]
fn destinations_existing_targets_and_current_worktree_are_protected() {
    let (t, mut w, id) = fixture();
    let d = w
        .worktree_destination(&id, t.path().to_str().unwrap(), &CANCEL)
        .unwrap();
    for folder in ["../escape", ".git", "nested/path", ""] {
        let s = w.open(&id, &CANCEL).unwrap();
        assert!(
            w.prepare(
                &id,
                &s.revision,
                Action::CreateWorktree {
                    destination_id: d.id.clone(),
                    folder: folder.into(),
                    reference_id: None,
                    new_branch: None,
                    detach: true
                },
                &CANCEL
            )
            .is_err()
        );
    }
    let p = plan(
        &mut w,
        &id,
        Action::CreateWorktree {
            destination_id: d.id,
            folder: "new".into(),
            reference_id: None,
            new_branch: None,
            detach: true,
        },
    );
    fs::create_dir(t.path().join("new")).unwrap();
    fs::write(t.path().join("new/keep"), "keep").unwrap();
    assert!(w.apply(&p.id, &CANCEL).is_err());
    assert!(t.path().join("new/keep").exists());
    let main = w.worktrees(&id, &CANCEL).unwrap().entries[0].clone();
    let s = w.open(&id, &CANCEL).unwrap();
    assert!(
        w.prepare(
            &id,
            &s.revision,
            Action::RemoveWorktree {
                id: main.id.clone()
            },
            &CANCEL
        )
        .is_err()
    );
    assert!(w.open_worktree("foreign", &main.id, &CANCEL).is_err());
    assert!(w.worktrees(&id, &AtomicBool::new(true)).is_err());
}

#[test]
fn pruning_preserves_detached_commit_objects_and_open_worktrees_survive_reload_and_rescan() {
    let (t, mut w, id) = fixture();
    create(&mut w, &id, t.path(), "unique", None, true);
    let root = t.path().join("unique");
    git(
        &root,
        &["commit", "--allow-empty", "-m", "only in linked HEAD"],
    );
    let head = git(&root, &["rev-parse", "HEAD"]);
    let e = linked(&mut w, &id);
    let opened = w.open_worktree(&id, &e.id, &CANCEL).unwrap();
    let linked_id = opened.state.repository.id;
    let base = tempfile::tempdir().unwrap();
    w.save(base.path()).unwrap();
    let mut restored = Workspace::load(base.path()).unwrap();
    assert_eq!(
        restored
            .open(&linked_id, &CANCEL)
            .unwrap()
            .repository
            .head
            .as_deref(),
        Some(head.as_str())
    );
    let settings = restored.catalog.settings.clone();
    assert!(
        restored
            .scan(settings, &CANCEL)
            .unwrap()
            .repositories
            .iter()
            .any(|r| r.id == linked_id && r.available)
    );
    fs::remove_dir_all(root).unwrap();
    w.worktrees(&id, &CANCEL).unwrap();
    let p = plan(&mut w, &id, Action::PruneWorktrees);
    w.apply(&p.id, &CANCEL).unwrap();
    assert_eq!(
        git(
            &t.path().join("main"),
            &[
                "rev-parse",
                &format!("refs/clear-compare/worktrees/{}/0", p.id)
            ]
        ),
        head
    );
}
#[cfg(unix)]
#[test]
fn replaced_parents_and_symlink_destinations_are_not_followed() {
    use std::os::unix::fs::symlink;
    let (t, mut w, id) = fixture();
    let parent = t.path().join("parent");
    fs::create_dir(&parent).unwrap();
    let d = w
        .worktree_destination(&id, parent.to_str().unwrap(), &CANCEL)
        .unwrap();
    let p = plan(
        &mut w,
        &id,
        Action::CreateWorktree {
            destination_id: d.id,
            folder: "linked".into(),
            reference_id: None,
            new_branch: None,
            detach: true,
        },
    );
    fs::rename(&parent, t.path().join("old-parent")).unwrap();
    fs::create_dir(&parent).unwrap();
    assert!(w.apply(&p.id, &CANCEL).is_err());
    assert!(!parent.join("linked").exists());
    let d = w
        .worktree_destination(&id, parent.to_str().unwrap(), &CANCEL)
        .unwrap();
    let p = plan(
        &mut w,
        &id,
        Action::CreateWorktree {
            destination_id: d.id,
            folder: "linked".into(),
            reference_id: None,
            new_branch: None,
            detach: true,
        },
    );
    symlink(t.path().join("main"), parent.join("linked")).unwrap();
    assert!(w.apply(&p.id, &CANCEL).is_err());
    assert!(t.path().join("main/tracked.txt").exists());
}

#[test]
fn bare_repository_can_create_and_open_a_linked_worktree() {
    let (t, mut w, _) = fixture();
    let bare = t.path().join("bare.git");
    git(
        t.path(),
        &["clone", "--bare", "main", bare.to_str().unwrap()],
    );
    let c = w
        .scan(
            ScanSettings {
                roots: vec![bare.display().to_string()],
                exclusions: vec![],
            },
            &CANCEL,
        )
        .unwrap();
    let id = c.repositories[0].id.clone();
    assert_eq!(w.open(&id, &CANCEL).unwrap().repository.kind, "bare");
    create(&mut w, &id, t.path(), "from-bare", Some("topic"), false);
    let e = linked(&mut w, &id);
    let opened = w.open_worktree(&id, &e.id, &CANCEL).unwrap();
    assert_eq!(opened.state.repository.branch, "topic");
    assert_eq!(
        fs::read_to_string(t.path().join("from-bare/tracked.txt")).unwrap(),
        "original\n"
    );
}
#[test]
fn occupied_branch_failure_keeps_owned_directory_and_records_actual_failure() {
    let (t, mut w, id) = fixture();
    let state = w.open(&id, &CANCEL).unwrap();
    let reference = state
        .references
        .iter()
        .find(|r| r.current)
        .unwrap()
        .id
        .clone();
    let d = w
        .worktree_destination(&id, t.path().to_str().unwrap(), &CANCEL)
        .unwrap();
    let p = plan(
        &mut w,
        &id,
        Action::CreateWorktree {
            destination_id: d.id,
            folder: "occupied".into(),
            reference_id: Some(reference),
            new_branch: None,
            detach: false,
        },
    );
    let err = w.apply(&p.id, &CANCEL).unwrap_err().to_string();
    assert!(err.contains("Worktree recovery:"));
    let journal = Path::new(&w.repositories[&id].common)
        .join("clear-compare-worktree-operations")
        .join(&p.id);
    assert!(journal.join("reserved.json").exists());
    assert_eq!(
        fs::read_to_string(journal.join("result.json")).unwrap(),
        "{\"success\":false}"
    );
    assert_eq!(fs::read_dir(t.path().join("occupied")).unwrap().count(), 0);
    assert_eq!(
        git(&t.path().join("main"), &["symbolic-ref", "HEAD"]),
        "refs/heads/main"
    );
    assert_eq!(w.worktrees(&id, &CANCEL).unwrap().entries.len(), 1);
}
