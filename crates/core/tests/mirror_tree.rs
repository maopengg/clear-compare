use clear_contracts::{CompareRequest, FileOperation, Side, SyncPlan, SyncResult};
use clear_core::{compare, recovery, sync};
use std::{fs, path::Path, sync::atomic::AtomicBool};
fn fixture() -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    for p in ["l", "r", "b", "undo"] {
        fs::create_dir(t.path().join(p)).unwrap();
    }
    t
}
fn capture(root: &Path) -> compare::Snapshot {
    compare::compare(
        &CompareRequest {
            left: root.join("l").display().to_string(),
            right: root.join("r").display().to_string(),
            exclusions: "-keep.txt;-hidden/".into(),
            content: true,
        },
        &AtomicBool::new(false),
    )
    .unwrap()
}
fn plan(root: &Path, snapshot: &compare::Snapshot, side: Side) -> SyncPlan {
    sync::plan_mirror_tree(snapshot, &root.join("b"), &AtomicBool::new(false), side).unwrap()
}
fn apply(snapshot: &compare::Snapshot, plan: &SyncPlan) -> SyncResult {
    let out = sync::execute(snapshot, plan, &AtomicBool::new(false)).unwrap();
    assert!(
        out.actions.iter().all(|a| a.state == "completed"),
        "{out:?}"
    );
    out
}
fn undo(root: &Path, out: &SyncResult) -> recovery::Plan {
    recovery::preview(
        Path::new(&out.journal),
        &root.join("undo"),
        &AtomicBool::new(false),
    )
    .unwrap()
}
#[test]
fn nested_tree_mirror_replicates_empty_directories_and_rolls_back_in_dependency_order() {
    for side in [Side::Left, Side::Right] {
        let t = fixture();
        let root = t.path();
        let (source, target) = if side == Side::Left {
            (root.join("l"), root.join("r"))
        } else {
            (root.join("r"), root.join("l"))
        };
        for p in ["new/sub", "empty/deep"] {
            fs::create_dir_all(source.join(p)).unwrap();
        }
        fs::write(source.join("new/sub/file"), b"new").unwrap();
        fs::write(source.join("change"), b"source").unwrap();
        fs::write(target.join("change"), b"target").unwrap();
        fs::create_dir_all(target.join("old/sub/empty")).unwrap();
        fs::write(target.join("old/sub/file"), b"original").unwrap();
        let snapshot = capture(root);
        let p = plan(root, &snapshot, side);
        assert_eq!(p.version, 5);
        assert_eq!(
            p.actions
                .iter()
                .filter(|a| a.operation == FileOperation::CreateDirectory)
                .count(),
            4
        );
        assert_eq!(
            p.actions
                .iter()
                .filter(|a| a.operation == FileOperation::RemoveDirectory)
                .count(),
            3
        );
        assert!(!Path::new(&p.backup_root).exists());
        let out = apply(&snapshot, &p);
        assert!(!target.join("old").exists());
        assert!(target.join("empty/deep").is_dir());
        assert_eq!(fs::read(target.join("new/sub/file")).unwrap(), b"new");
        let after = capture(root);
        assert_eq!(
            after.report.summary.changed
                + after.report.summary.left_only
                + after.report.summary.right_only
                + after.report.summary.errors,
            0
        );
        let rollback = undo(root, &out);
        assert_eq!(
            rollback.view.actions[0].operation,
            "restore_removed_directory"
        );
        assert_eq!(
            rollback.view.actions.last().unwrap().operation,
            "remove_created_directory"
        );
        let recovered = recovery::execute(&rollback, &AtomicBool::new(false)).unwrap();
        assert!(
            recovered.actions.iter().all(|a| a.state == "completed"),
            "{recovered:?}"
        );
        assert_eq!(fs::read(target.join("old/sub/file")).unwrap(), b"original");
        assert!(target.join("old/sub/empty").is_dir());
        assert_eq!(fs::read(target.join("change")).unwrap(), b"target");
        assert!(!target.join("new").exists());
        assert!(!target.join("empty").exists());
        assert_eq!(fs::read(source.join("new/sub/file")).unwrap(), b"new");
    }
}
#[test]
fn directory_only_changes_have_reviewed_metadata_backups_and_running_state_reconciliation() {
    let t = fixture();
    let root = t.path();
    fs::create_dir_all(root.join("l/new/empty")).unwrap();
    fs::create_dir_all(root.join("r/old/empty")).unwrap();
    let snapshot = capture(root);
    let p = plan(root, &snapshot, Side::Left);
    assert_eq!(p.actions.len(), 4);
    let mut out = apply(&snapshot, &p);
    assert!(!root.join("r/old").exists());
    assert!(root.join("r/new/empty").is_dir());
    assert_eq!(
        fs::read_dir(Path::new(&p.backup_root).join("directory-metadata"))
            .unwrap()
            .count(),
        2
    );
    for action in &mut out.actions {
        action.state = "running".into();
    }
    fs::write(&out.journal, serde_json::to_vec(&out).unwrap()).unwrap();
    let recovered = recovery::execute(&undo(root, &out), &AtomicBool::new(false)).unwrap();
    assert!(
        recovered.actions.iter().all(|a| a.state == "completed"),
        "{recovered:?}"
    );
    assert!(root.join("r/old/empty").is_dir());
    assert!(!root.join("r/new").exists());
}
#[test]
fn excluded_descendants_keep_their_ancestors_without_blocking_other_mirror_actions() {
    let t = fixture();
    let root = t.path();
    fs::create_dir_all(root.join("r/old/hidden")).unwrap();
    fs::create_dir_all(root.join("r/old/removable/empty")).unwrap();
    fs::write(root.join("r/old/hidden/secret"), b"keep subtree").unwrap();
    fs::write(root.join("r/old/keep.txt"), b"keep file").unwrap();
    fs::write(root.join("r/old/remove"), b"remove").unwrap();
    let snapshot = capture(root);
    let p = plan(root, &snapshot, Side::Left);
    assert!(p.warnings.iter().any(|w| w.ends_with("old")));
    let out = apply(&snapshot, &p);
    assert!(root.join("r/old").is_dir());
    assert!(!root.join("r/old/removable").exists());
    assert!(!root.join("r/old/remove").exists());
    assert_eq!(
        fs::read(root.join("r/old/hidden/secret")).unwrap(),
        b"keep subtree"
    );
    assert_eq!(fs::read(root.join("r/old/keep.txt")).unwrap(), b"keep file");
    let recovered = recovery::execute(&undo(root, &out), &AtomicBool::new(false)).unwrap();
    assert!(
        recovered.actions.iter().all(|a| a.state == "completed"),
        "{recovered:?}"
    );
    assert_eq!(fs::read(root.join("r/old/remove")).unwrap(), b"remove");
}
#[test]
fn newly_excluded_children_or_replaced_empty_directories_expire_the_plan_before_any_write() {
    for replacement in [false, true] {
        let t = fixture();
        let root = t.path();
        fs::create_dir(root.join("r/old")).unwrap();
        fs::create_dir(root.join("l/new")).unwrap();
        let snapshot = capture(root);
        let p = plan(root, &snapshot, Side::Left);
        if replacement {
            fs::rename(root.join("r/old"), root.join("parked")).unwrap();
            fs::create_dir(root.join("r/old")).unwrap();
        } else {
            fs::write(root.join("r/old/keep.txt"), b"external").unwrap();
        }
        assert!(sync::execute(&snapshot, &p, &AtomicBool::new(false)).is_err());
        assert!(!Path::new(&p.backup_root).exists());
        assert!(!root.join("r/new").exists());
        assert!(root.join("r/old").is_dir());
    }
}
#[test]
fn dependency_order_and_directory_identity_metadata_cannot_be_forged() {
    let t = fixture();
    let root = t.path();
    fs::create_dir_all(root.join("l/a/b")).unwrap();
    let snapshot = capture(root);
    let mut p = plan(root, &snapshot, Side::Left);
    p.actions.reverse();
    assert!(sync::execute(&snapshot, &p, &AtomicBool::new(false)).is_err());
    assert!(!Path::new(&p.backup_root).exists());
    fs::create_dir(root.join("r/old")).unwrap();
    let snapshot = capture(root);
    let mut p = plan(root, &snapshot, Side::Left);
    p.actions
        .iter_mut()
        .find(|a| a.operation == FileOperation::RemoveDirectory)
        .unwrap()
        .directory_identity = None;
    assert!(sync::execute(&snapshot, &p, &AtomicBool::new(false)).is_err());
    assert!(!Path::new(&p.backup_root).exists());
}
#[test]
fn recovery_refuses_external_created_directory_replacement_and_new_contents() {
    for replace in [true, false] {
        let t = fixture();
        let root = t.path();
        fs::create_dir(root.join("l/new")).unwrap();
        let snapshot = capture(root);
        let out = apply(&snapshot, &plan(root, &snapshot, Side::Left));
        if replace {
            fs::rename(root.join("r/new"), root.join("parked")).unwrap();
            fs::create_dir(root.join("r/new")).unwrap();
        } else {
            fs::write(root.join("r/new/external"), b"external").unwrap();
        }
        assert!(
            recovery::preview(
                Path::new(&out.journal),
                &root.join("undo"),
                &AtomicBool::new(false)
            )
            .is_err()
        );
        assert!(root.join("r/new").is_dir());
    }
}
#[test]
fn recovery_rechecks_directory_contents_and_recreated_paths_after_preview() {
    for changed in ["new contents", "recreated original"] {
        let t = fixture();
        let root = t.path();
        fs::create_dir(root.join("l/new")).unwrap();
        fs::create_dir(root.join("r/old")).unwrap();
        let snapshot = capture(root);
        let out = apply(&snapshot, &plan(root, &snapshot, Side::Left));
        let rollback = undo(root, &out);
        if changed == "new contents" {
            fs::write(root.join("r/new/external"), b"keep").unwrap();
        } else {
            fs::create_dir(root.join("r/old")).unwrap();
        }
        assert!(recovery::execute(&rollback, &AtomicBool::new(false)).is_err());
        assert!(!Path::new(&rollback.view.backup_root).exists());
        assert!(root.join("r/new").is_dir());
    }
}
#[test]
fn missing_ownership_or_tampered_metadata_records_block_automatic_recovery() {
    for ownership in [true, false] {
        let t = fixture();
        let root = t.path();
        if ownership {
            fs::create_dir(root.join("l/new")).unwrap();
        } else {
            fs::create_dir(root.join("r/old")).unwrap();
        }
        let snapshot = capture(root);
        let out = apply(&snapshot, &plan(root, &snapshot, Side::Left));
        let dir = Path::new(&out.plan.backup_root).join(if ownership {
            "directory-created"
        } else {
            "directory-metadata"
        });
        let record = fs::read_dir(dir).unwrap().next().unwrap().unwrap().path();
        if ownership {
            fs::remove_file(record).unwrap();
        } else {
            fs::write(record, b"{}").unwrap();
        }
        assert!(
            recovery::preview(
                Path::new(&out.journal),
                &root.join("undo"),
                &AtomicBool::new(false)
            )
            .is_err()
        );
        if ownership {
            assert!(root.join("r/new").is_dir());
        } else {
            assert!(!root.join("r/old").exists());
        }
    }
}
#[cfg(unix)]
#[test]
fn removed_empty_directory_basic_permissions_are_restored() {
    use std::os::unix::fs::PermissionsExt;
    let t = fixture();
    let root = t.path();
    fs::create_dir(root.join("r/old")).unwrap();
    fs::set_permissions(root.join("r/old"), fs::Permissions::from_mode(0o750)).unwrap();
    let snapshot = capture(root);
    let out = apply(&snapshot, &plan(root, &snapshot, Side::Left));
    let result = recovery::execute(&undo(root, &out), &AtomicBool::new(false)).unwrap();
    assert_eq!(result.actions[0].state, "completed");
    assert_eq!(
        fs::metadata(root.join("r/old"))
            .unwrap()
            .permissions()
            .mode()
            & 0o7777,
        0o750
    );
}

#[test]
fn directory_name_aliases_are_rejected_before_any_write_on_insensitive_filesystems() {
    let t = fixture();
    let root = t.path();
    fs::create_dir(root.join("l/Case")).unwrap();
    fs::create_dir(root.join("r/case")).unwrap();
    let snapshot = capture(root);
    let p = sync::plan_mirror_tree(
        &snapshot,
        &root.join("b"),
        &AtomicBool::new(false),
        Side::Left,
    );
    if root.join("r/Case").exists() {
        assert!(p.is_err());
        assert_eq!(fs::read_dir(root.join("b")).unwrap().count(), 0);
    } else {
        let out = apply(&snapshot, &p.unwrap());
        assert!(root.join("r/Case").is_dir());
        assert!(!root.join("r/case").exists());
        let result = recovery::execute(&undo(root, &out), &AtomicBool::new(false)).unwrap();
        assert!(result.actions.iter().all(|a| a.state == "completed"));
        assert!(root.join("r/case").is_dir());
        assert!(!root.join("r/Case").exists());
    }
}
#[test]
fn cancelled_directory_only_plans_create_no_output_or_journal() {
    let t = fixture();
    let root = t.path();
    fs::create_dir_all(root.join("l/new/deep")).unwrap();
    let snapshot = capture(root);
    let p = plan(root, &snapshot, Side::Left);
    assert!(sync::execute(&snapshot, &p, &AtomicBool::new(true)).is_err());
    assert!(!root.join("r/new").exists());
    assert!(!Path::new(&p.backup_root).exists());
}

#[cfg(target_os = "linux")]
#[test]
fn native_non_unicode_directory_names_round_trip_through_identity_records() {
    use std::os::unix::ffi::OsStringExt;
    let t = fixture();
    let root = t.path();
    let new = std::ffi::OsString::from_vec(vec![b'n', 255]);
    let old = std::ffi::OsString::from_vec(vec![b'o', 254]);
    fs::create_dir(root.join("l").join(&new)).unwrap();
    fs::create_dir(root.join("r").join(&old)).unwrap();
    fs::write(root.join("l").join(&new).join("file"), b"native new").unwrap();
    fs::write(root.join("r").join(&old).join("file"), b"native old").unwrap();
    let snapshot = capture(root);
    let out = apply(&snapshot, &plan(root, &snapshot, Side::Left));
    assert_eq!(
        fs::read(root.join("r").join(&new).join("file")).unwrap(),
        b"native new"
    );
    assert!(!root.join("r").join(&old).exists());
    let recovered = recovery::execute(&undo(root, &out), &AtomicBool::new(false)).unwrap();
    assert!(
        recovered.actions.iter().all(|a| a.state == "completed"),
        "{recovered:?}"
    );
    assert!(!root.join("r").join(&new).exists());
    assert_eq!(
        fs::read(root.join("r").join(&old).join("file")).unwrap(),
        b"native old"
    );
}
