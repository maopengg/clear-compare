use clear_contracts::{CompareRequest, FileOperation, FilePlanPolicy, Side, SyncResult};
use clear_core::{compare, recovery, sync};
use std::{fs, path::Path, sync::atomic::AtomicBool};
fn fixture() -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    for d in ["l", "r", "b", "recover"] {
        fs::create_dir(t.path().join(d)).unwrap();
    }
    t
}
fn capture(root: &Path) -> compare::Snapshot {
    compare::compare(
        &CompareRequest {
            left: root.join("l").display().to_string(),
            right: root.join("r").display().to_string(),
            exclusions: "-keep.txt".into(),
            content: true,
        },
        &AtomicBool::new(false),
    )
    .unwrap()
}
fn id(snapshot: &compare::Snapshot, path: &str) -> String {
    snapshot
        .report
        .rows
        .iter()
        .find(|r| r.path == path)
        .unwrap()
        .id
        .clone()
}
fn apply_delete(root: &Path, path: &str, side: Side) -> SyncResult {
    let snapshot = capture(root);
    let plan = sync::plan_delete(
        &snapshot,
        &[id(&snapshot, path)],
        &root.join("b"),
        &AtomicBool::new(false),
        side,
    )
    .unwrap();
    let result = sync::execute(&snapshot, &plan, &AtomicBool::new(false)).unwrap();
    assert!(
        result.actions.iter().all(|a| a.state == "completed"),
        "{result:?}"
    );
    result
}
#[test]
fn reviewed_deletion_preserves_other_side_exclusions_and_directories_then_restores_exact_bytes() {
    let t = fixture();
    let root = t.path();
    fs::create_dir(root.join("r/sub")).unwrap();
    fs::write(root.join("l/a"), "left\n").unwrap();
    fs::write(root.join("r/a"), "right\n").unwrap();
    fs::write(root.join("r/sub/empty"), []).unwrap();
    fs::write(root.join("r/keep.txt"), "keep").unwrap();
    let snapshot = capture(root);
    let plan = sync::plan_delete(
        &snapshot,
        &[id(&snapshot, "a"), id(&snapshot, "sub/empty")],
        &root.join("b"),
        &AtomicBool::new(false),
        Side::Right,
    )
    .unwrap();
    assert_eq!(plan.version, 2);
    assert_eq!(
        plan.policy,
        FilePlanPolicy::DeleteSelected {
            target_side: Side::Right
        }
    );
    assert_eq!(plan.actions[0].operation, FileOperation::Delete);
    assert!(plan.actions[0].source_digest.is_none());
    assert!(!Path::new(&plan.backup_root).exists());
    assert_eq!(fs::read_to_string(root.join("r/a")).unwrap(), "right\n");
    let result = sync::execute(&snapshot, &plan, &AtomicBool::new(false)).unwrap();
    assert!(result.actions.iter().all(|a| a.state == "completed"));
    assert!(!root.join("r/a").exists());
    assert!(!root.join("r/sub/empty").exists());
    assert!(root.join("r/sub").is_dir());
    assert_eq!(fs::read_to_string(root.join("l/a")).unwrap(), "left\n");
    assert_eq!(fs::read_to_string(root.join("r/keep.txt")).unwrap(), "keep");
    assert_eq!(
        fs::read(Path::new(&plan.backup_root).join("files/a")).unwrap(),
        b"right\n"
    );
    drop(snapshot);
    let rollback = recovery::preview(
        Path::new(&result.journal),
        &root.join("recover"),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(
        rollback
            .view
            .actions
            .iter()
            .all(|a| a.operation == "restore_deleted")
    );
    fs::remove_dir(root.join("r/sub")).unwrap();
    let restored = recovery::execute(&rollback, &AtomicBool::new(false)).unwrap();
    assert!(
        restored.actions.iter().all(|a| a.state == "completed"),
        "{restored:?}"
    );
    assert_eq!(fs::read(root.join("r/a")).unwrap(), b"right\n");
    assert!(fs::read(root.join("r/sub/empty")).unwrap().is_empty());
}
#[test]
fn explicit_side_selection_and_duplicate_ids_cannot_remove_the_opposite_file() {
    let t = fixture();
    let root = t.path();
    fs::write(root.join("l/a"), "left").unwrap();
    fs::write(root.join("r/a"), "right").unwrap();
    let snapshot = capture(root);
    let row = id(&snapshot, "a");
    let plan = sync::plan_delete(
        &snapshot,
        &[row.clone(), row],
        &root.join("b"),
        &AtomicBool::new(false),
        Side::Left,
    )
    .unwrap();
    assert_eq!(plan.actions.len(), 1);
    let result = sync::execute(&snapshot, &plan, &AtomicBool::new(false)).unwrap();
    assert_eq!(result.actions[0].state, "completed");
    assert!(!root.join("l/a").exists());
    assert_eq!(fs::read(root.join("r/a")).unwrap(), b"right");
}
#[test]
fn changed_targets_stop_later_actions_and_cancelled_operations_remove_nothing() {
    let t = fixture();
    let root = t.path();
    for p in ["a", "b"] {
        fs::write(root.join("r").join(p), p).unwrap();
    }
    let snapshot = capture(root);
    let plan = sync::plan_delete(
        &snapshot,
        &[id(&snapshot, "a"), id(&snapshot, "b")],
        &root.join("b"),
        &AtomicBool::new(false),
        Side::Right,
    )
    .unwrap();
    fs::write(root.join("r/a"), "external").unwrap();
    let result = sync::execute(&snapshot, &plan, &AtomicBool::new(false)).unwrap();
    assert_eq!(result.actions[0].state, "failed");
    assert_eq!(result.actions[1].state, "pending");
    assert_eq!(fs::read(root.join("r/a")).unwrap(), b"external");
    assert_eq!(fs::read(root.join("r/b")).unwrap(), b"b");
    let snapshot = capture(root);
    let plan = sync::plan_delete(
        &snapshot,
        &[id(&snapshot, "b")],
        &root.join("b"),
        &AtomicBool::new(false),
        Side::Right,
    )
    .unwrap();
    let result = sync::execute(&snapshot, &plan, &AtomicBool::new(true)).unwrap();
    assert_eq!(result.actions[0].state, "cancelled");
    assert!(root.join("r/b").exists());
    assert!(
        sync::plan_delete(
            &snapshot,
            &[id(&snapshot, "b")],
            &root.join("b"),
            &AtomicBool::new(true),
            Side::Right
        )
        .is_err()
    );
}
#[test]
fn stale_snapshots_missing_sides_directories_and_nested_backups_are_rejected() {
    let t = fixture();
    let root = t.path();
    fs::write(root.join("r/a"), "a").unwrap();
    fs::create_dir(root.join("r/dir")).unwrap();
    let snapshot = capture(root);
    assert!(
        sync::plan_delete(
            &snapshot,
            &[],
            &root.join("b"),
            &AtomicBool::new(false),
            Side::Right
        )
        .is_err()
    );
    assert!(
        sync::plan_delete(
            &snapshot,
            &[id(&snapshot, "a")],
            &root.join("b"),
            &AtomicBool::new(false),
            Side::Left
        )
        .is_err()
    );
    assert!(
        sync::plan_delete(
            &snapshot,
            &[id(&snapshot, "dir")],
            &root.join("b"),
            &AtomicBool::new(false),
            Side::Right
        )
        .is_err()
    );
    assert!(
        sync::plan_delete(
            &snapshot,
            &[id(&snapshot, "a")],
            &root.join("r"),
            &AtomicBool::new(false),
            Side::Right
        )
        .is_err()
    );
    fs::write(root.join("r/a"), "changed").unwrap();
    assert!(
        sync::plan_delete(
            &snapshot,
            &[id(&snapshot, "a")],
            &root.join("b"),
            &AtomicBool::new(false),
            Side::Right
        )
        .is_err()
    );
}
#[test]
fn recreated_paths_are_never_overwritten_by_deletion_recovery() {
    let t = fixture();
    let root = t.path();
    fs::write(root.join("r/a"), "old").unwrap();
    let result = apply_delete(root, "a", Side::Right);
    let rollback = recovery::preview(
        Path::new(&result.journal),
        &root.join("recover"),
        &AtomicBool::new(false),
    )
    .unwrap();
    fs::write(root.join("r/a"), "external").unwrap();
    assert!(
        recovery::preview(
            Path::new(&result.journal),
            &root.join("recover"),
            &AtomicBool::new(false)
        )
        .is_err()
    );
    let result = recovery::execute(&rollback, &AtomicBool::new(false)).unwrap();
    assert_eq!(result.actions[0].state, "failed");
    assert_eq!(fs::read(root.join("r/a")).unwrap(), b"external");
}
#[test]
fn tampered_backups_fail_before_restoration() {
    let t = fixture();
    let root = t.path();
    fs::write(root.join("r/a"), "old").unwrap();
    let result = apply_delete(root, "a", Side::Right);
    let rollback = recovery::preview(
        Path::new(&result.journal),
        &root.join("recover"),
        &AtomicBool::new(false),
    )
    .unwrap();
    fs::write(
        Path::new(&result.plan.backup_root).join("files/a"),
        "corrupt",
    )
    .unwrap();
    assert!(
        recovery::preview(
            Path::new(&result.journal),
            &root.join("recover"),
            &AtomicBool::new(false)
        )
        .is_err()
    );
    let result = recovery::execute(&rollback, &AtomicBool::new(false)).unwrap();
    assert_eq!(result.actions[0].state, "failed");
    assert!(!root.join("r/a").exists());
}
#[test]
fn interrupted_deletions_reconcile_absence_and_ignore_actions_that_never_removed_a_file() {
    for state in ["running", "failed"] {
        let t = fixture();
        let root = t.path();
        fs::write(root.join("r/a"), "old").unwrap();
        let mut result = apply_delete(root, "a", Side::Right);
        result.actions[0].state = state.into();
        fs::write(&result.journal, serde_json::to_vec(&result).unwrap()).unwrap();
        let rollback = recovery::preview(
            Path::new(&result.journal),
            &root.join("recover"),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(rollback.view.actions[0].operation, "restore_deleted");
        assert_eq!(
            recovery::execute(&rollback, &AtomicBool::new(false))
                .unwrap()
                .actions[0]
                .state,
            "completed"
        );
        result.actions[0].state = state.into();
        fs::write(&result.journal, serde_json::to_vec(&result).unwrap()).unwrap();
        assert!(
            recovery::preview(
                Path::new(&result.journal),
                &root.join("recover"),
                &AtomicBool::new(false)
            )
            .is_err()
        );
        assert_eq!(fs::read(root.join("r/a")).unwrap(), b"old");
    }
}
#[test]
fn version_one_copy_journals_remain_recoverable() {
    let t = fixture();
    let root = t.path();
    fs::write(root.join("l/a"), "new").unwrap();
    fs::write(root.join("r/a"), "old").unwrap();
    let snapshot = capture(root);
    let plan = sync::plan(
        &snapshot,
        &[id(&snapshot, "a")],
        &root.join("b"),
        &AtomicBool::new(false),
    )
    .unwrap();
    let result = sync::execute(&snapshot, &plan, &AtomicBool::new(false)).unwrap();
    let mut value = serde_json::to_value(&result).unwrap();
    value.as_object_mut().unwrap().remove("intent_digest");
    value.as_object_mut().unwrap().remove("attempts");
    let p = value["plan"].as_object_mut().unwrap();
    for field in ["version", "policy", "target"] {
        p.remove(field);
    }
    for a in p["actions"].as_array_mut().unwrap() {
        let a = a.as_object_mut().unwrap();
        a.remove("operation");
        a.remove("target_permissions");
        let existed = a.remove("target_existed").unwrap();
        a.insert("overwrite".into(), existed);
    }
    fs::write(&result.journal, serde_json::to_vec(&value).unwrap()).unwrap();
    let rollback = recovery::preview(
        Path::new(&result.journal),
        &root.join("recover"),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(
        recovery::execute(&rollback, &AtomicBool::new(false))
            .unwrap()
            .actions[0]
            .state,
        "completed"
    );
    assert_eq!(fs::read(root.join("r/a")).unwrap(), b"old");
}
#[test]
fn policy_tampering_and_oversized_journals_fail_before_any_file_is_removed() {
    let t = fixture();
    let root = t.path();
    fs::write(root.join("r/a"), "old").unwrap();
    let snapshot = capture(root);
    let mut plan = sync::plan_delete(
        &snapshot,
        &[id(&snapshot, "a")],
        &root.join("b"),
        &AtomicBool::new(false),
        Side::Right,
    )
    .unwrap();
    let original = plan.clone();
    plan.policy = FilePlanPolicy::SelectedCopy;
    assert!(sync::execute(&snapshot, &plan, &AtomicBool::new(false)).is_err());
    assert!(!Path::new(&plan.backup_root).exists());
    let mut plan = original.clone();
    plan.actions[0].path = "x".repeat(32 * 1024 * 1024);
    assert!(sync::execute(&snapshot, &plan, &AtomicBool::new(false)).is_err());
    assert!(!Path::new(&plan.backup_root).exists());
    assert_eq!(fs::read(root.join("r/a")).unwrap(), b"old");
    let mut plan = original;
    plan.version = 99;
    assert!(sync::execute(&snapshot, &plan, &AtomicBool::new(false)).is_err());
}
#[cfg(unix)]
#[test]
fn links_are_not_followed_and_original_permission_bits_are_restored() {
    use std::os::unix::fs::PermissionsExt;
    let t = fixture();
    let root = t.path();
    fs::write(root.join("r/a"), "old").unwrap();
    fs::set_permissions(root.join("r/a"), fs::Permissions::from_mode(0o750)).unwrap();
    let result = apply_delete(root, "a", Side::Right);
    let rollback = recovery::preview(
        Path::new(&result.journal),
        &root.join("recover"),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(
        recovery::execute(&rollback, &AtomicBool::new(false))
            .unwrap()
            .actions[0]
            .state,
        "completed"
    );
    assert_eq!(
        fs::metadata(root.join("r/a")).unwrap().permissions().mode() & 0o7777,
        0o750
    );
    let snapshot = capture(root);
    let plan = sync::plan_delete(
        &snapshot,
        &[id(&snapshot, "a")],
        &root.join("b"),
        &AtomicBool::new(false),
        Side::Right,
    )
    .unwrap();
    fs::rename(root.join("r/a"), root.join("outside")).unwrap();
    std::os::unix::fs::symlink(root.join("outside"), root.join("r/a")).unwrap();
    let result = sync::execute(&snapshot, &plan, &AtomicBool::new(false)).unwrap();
    assert_eq!(result.actions[0].state, "failed");
    assert_eq!(fs::read(root.join("outside")).unwrap(), b"old");
    assert!(
        fs::symlink_metadata(root.join("r/a"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}
#[cfg(target_os = "linux")]
#[test]
fn deleted_non_unicode_names_round_trip_without_display_path_reconstruction() {
    use std::os::unix::ffi::OsStringExt;
    let t = fixture();
    let root = t.path();
    let name = std::ffi::OsString::from_vec(vec![b'a', 255]);
    fs::write(root.join("r").join(&name), "native").unwrap();
    let snapshot = capture(root);
    let row = snapshot
        .paths
        .iter()
        .find(|(_, p)| p.as_os_str() == name)
        .unwrap()
        .0
        .clone();
    let plan = sync::plan_delete(
        &snapshot,
        &[row],
        &root.join("b"),
        &AtomicBool::new(false),
        Side::Right,
    )
    .unwrap();
    let result = sync::execute(&snapshot, &plan, &AtomicBool::new(false)).unwrap();
    let rollback = recovery::preview(
        Path::new(&result.journal),
        &root.join("recover"),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(
        recovery::execute(&rollback, &AtomicBool::new(false))
            .unwrap()
            .actions[0]
            .state,
        "completed"
    );
    assert_eq!(fs::read(root.join("r").join(name)).unwrap(), b"native");
}

#[test]
fn recovery_labels_are_derived_from_native_paths_instead_of_journal_display_strings() {
    let t = fixture();
    let root = t.path();
    fs::write(root.join("r/a"), "old").unwrap();
    let mut result = apply_delete(root, "a", Side::Right);
    result.plan.actions[0].path = "misleading label".into();
    result.actions[0].path = "misleading label".into();
    fs::write(&result.journal, serde_json::to_vec(&result).unwrap()).unwrap();
    let rollback = recovery::preview(
        Path::new(&result.journal),
        &root.join("recover"),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(rollback.view.actions[0].path, "a");
    assert_eq!(
        recovery::execute(&rollback, &AtomicBool::new(false))
            .unwrap()
            .actions[0]
            .state,
        "completed"
    );
    assert_eq!(fs::read(root.join("r/a")).unwrap(), b"old");
}
#[cfg(unix)]
#[test]
fn changed_permission_bits_block_deletion_or_restoration_of_a_tampered_backup() {
    use std::os::unix::fs::PermissionsExt;
    let t = fixture();
    let root = t.path();
    fs::write(root.join("r/a"), "old").unwrap();
    let snapshot = capture(root);
    let plan = sync::plan_delete(
        &snapshot,
        &[id(&snapshot, "a")],
        &root.join("b"),
        &AtomicBool::new(false),
        Side::Right,
    )
    .unwrap();
    fs::set_permissions(root.join("r/a"), fs::Permissions::from_mode(0o750)).unwrap();
    let result = sync::execute(&snapshot, &plan, &AtomicBool::new(false)).unwrap();
    assert_eq!(result.actions[0].state, "failed");
    assert_eq!(fs::read(root.join("r/a")).unwrap(), b"old");
    let result = apply_delete(root, "a", Side::Right);
    let rollback = recovery::preview(
        Path::new(&result.journal),
        &root.join("recover"),
        &AtomicBool::new(false),
    )
    .unwrap();
    fs::set_permissions(
        Path::new(&result.plan.backup_root).join("files/a"),
        fs::Permissions::from_mode(0o777),
    )
    .unwrap();
    assert!(
        recovery::preview(
            Path::new(&result.journal),
            &root.join("recover"),
            &AtomicBool::new(false)
        )
        .is_err()
    );
    assert_eq!(
        recovery::execute(&rollback, &AtomicBool::new(false))
            .unwrap()
            .actions[0]
            .state,
        "failed"
    );
    assert!(!root.join("r/a").exists());
}

#[test]
fn malformed_native_roots_and_empty_or_traversing_journal_paths_are_rejected() {
    let t = fixture();
    let root = t.path();
    fs::write(root.join("r/a"), "old").unwrap();
    fs::write(root.join("sentinel"), "keep").unwrap();
    let result = apply_delete(root, "a", Side::Right);
    for path in ["", "../sentinel", "."] {
        let mut modified = result.clone();
        modified.plan.actions[0].relative_native =
            Some(clear_core::native_path::encode(Path::new(path)));
        fs::write(&modified.journal, serde_json::to_vec(&modified).unwrap()).unwrap();
        assert!(
            recovery::preview(
                Path::new(&modified.journal),
                &root.join("recover"),
                &AtomicBool::new(false)
            )
            .is_err()
        );
    }
    let mut modified = result;
    modified.plan.target_native = Some(clear_core::native_path::encode(&root.join("sentinel")));
    fs::write(&modified.journal, serde_json::to_vec(&modified).unwrap()).unwrap();
    assert!(
        recovery::preview(
            Path::new(&modified.journal),
            &root.join("recover"),
            &AtomicBool::new(false)
        )
        .is_err()
    );
    assert_eq!(fs::read(root.join("sentinel")).unwrap(), b"keep");
    assert!(!root.join("r/a").exists());
}
