use clear_contracts::{CompareRequest, FileOperation, FilePlanPolicy, Side};
use clear_core::{compare, recovery, sync};
use std::{fs, path::Path, sync::atomic::AtomicBool};
fn fixture() -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    for d in ["l", "r", "backups", "recovery"] {
        fs::create_dir(t.path().join(d)).unwrap();
    }
    t
}
fn capture(root: &Path, content: bool) -> compare::Snapshot {
    compare::compare(
        &CompareRequest {
            left: root.join("l").display().to_string(),
            right: root.join("r").display().to_string(),
            exclusions: "-keep.txt;-./private/".into(),
            content,
        },
        &AtomicBool::new(false),
    )
    .unwrap()
}
#[test]
fn full_file_mirror_previews_all_actions_preserves_exclusions_and_can_be_rolled_back() {
    for side in [Side::Left, Side::Right] {
        let t = fixture();
        let root = t.path();
        let (source, target) = if side == Side::Left {
            ("l", "r")
        } else {
            ("r", "l")
        };
        let source = root.join(source);
        let target = root.join(target);
        fs::create_dir(source.join("sub")).unwrap();
        fs::write(source.join("sub/new"), b"new").unwrap();
        fs::write(source.join("changed"), b"source").unwrap();
        fs::write(source.join("same"), b"same").unwrap();
        fs::write(target.join("changed"), b"target").unwrap();
        fs::write(target.join("same"), b"same").unwrap();
        fs::write(target.join("extra"), b"extra").unwrap();
        fs::write(target.join("keep.txt"), b"excluded").unwrap();
        fs::create_dir(target.join("private")).unwrap();
        fs::write(target.join("private/secret"), b"excluded subtree").unwrap();
        fs::create_dir(target.join("empty")).unwrap();
        let snapshot = capture(root, true);
        let plan = sync::plan_mirror_files(
            &snapshot,
            &root.join("backups"),
            &AtomicBool::new(false),
            side,
        )
        .unwrap();
        assert_eq!(plan.version, 4);
        assert_eq!(plan.policy, FilePlanPolicy::MirrorFiles);
        assert_eq!(plan.actions.len(), 3);
        assert_eq!(
            plan.actions
                .iter()
                .filter(|a| a.operation == FileOperation::Delete)
                .count(),
            1
        );
        assert!(!Path::new(&plan.backup_root).exists());
        assert_eq!(fs::read(target.join("changed")).unwrap(), b"target");
        let result = sync::execute(&snapshot, &plan, &AtomicBool::new(false)).unwrap();
        assert!(
            result.actions.iter().all(|a| a.state == "completed"),
            "{result:?}"
        );
        assert_eq!(fs::read(target.join("changed")).unwrap(), b"source");
        assert_eq!(fs::read(target.join("sub/new")).unwrap(), b"new");
        assert!(!target.join("extra").exists());
        assert_eq!(fs::read(target.join("keep.txt")).unwrap(), b"excluded");
        assert_eq!(
            fs::read(target.join("private/secret")).unwrap(),
            b"excluded subtree"
        );
        assert!(target.join("empty").is_dir());
        let undo = recovery::preview(
            Path::new(&result.journal),
            &root.join("recovery"),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(undo.view.actions.len(), 3);
        let out = recovery::execute(&undo, &AtomicBool::new(false)).unwrap();
        assert!(
            out.actions.iter().all(|a| a.state == "completed"),
            "{out:?}"
        );
        assert_eq!(fs::read(target.join("changed")).unwrap(), b"target");
        assert_eq!(fs::read(target.join("extra")).unwrap(), b"extra");
        assert!(!target.join("sub/new").exists());
        assert_eq!(fs::read(source.join("changed")).unwrap(), b"source");
    }
}
#[test]
fn equal_file_changes_new_entries_and_recreated_source_counterparts_expire_the_whole_plan() {
    for change in [
        "equal",
        "source extra",
        "target extra",
        "counterpart",
        "directory",
    ] {
        let t = fixture();
        let root = t.path();
        fs::write(root.join("l/same"), b"same").unwrap();
        fs::write(root.join("r/same"), b"same").unwrap();
        fs::write(root.join("r/extra"), b"preserve").unwrap();
        let snapshot = capture(root, true);
        let plan = sync::plan_mirror_files(
            &snapshot,
            &root.join("backups"),
            &AtomicBool::new(false),
            Side::Left,
        )
        .unwrap();
        match change {
            "equal" => fs::write(root.join("l/same"), b"changed").unwrap(),
            "source extra" => fs::write(root.join("l/new"), b"new").unwrap(),
            "target extra" => fs::write(root.join("r/new"), b"new").unwrap(),
            "counterpart" => fs::write(root.join("l/extra"), b"recreated").unwrap(),
            _ => fs::create_dir(root.join("l/new-directory")).unwrap(),
        }
        assert!(
            sync::execute(&snapshot, &plan, &AtomicBool::new(false)).is_err(),
            "{change}"
        );
        assert!(!Path::new(&plan.backup_root).exists());
        assert_eq!(fs::read(root.join("r/extra")).unwrap(), b"preserve");
    }
}
#[test]
fn metadata_only_type_conflicts_and_no_file_changes_are_explicit_errors() {
    let t = fixture();
    let root = t.path();
    fs::write(root.join("r/extra"), b"preserve").unwrap();
    let snapshot = capture(root, false);
    assert!(
        sync::plan_mirror_files(
            &snapshot,
            &root.join("backups"),
            &AtomicBool::new(false),
            Side::Left
        )
        .is_err()
    );
    fs::create_dir(root.join("l/extra")).unwrap();
    let snapshot = capture(root, true);
    assert!(
        sync::plan_mirror_files(
            &snapshot,
            &root.join("backups"),
            &AtomicBool::new(false),
            Side::Left
        )
        .is_err()
    );
    fs::remove_file(root.join("r/extra")).unwrap();
    let snapshot = capture(root, true);
    assert!(
        sync::plan_mirror_files(
            &snapshot,
            &root.join("backups"),
            &AtomicBool::new(false),
            Side::Left
        )
        .is_err()
    );
}
#[test]
fn omitted_or_mistyped_actions_are_rejected_before_creating_a_journal() {
    let t = fixture();
    let root = t.path();
    fs::write(root.join("r/a"), b"a").unwrap();
    fs::write(root.join("r/b"), b"b").unwrap();
    let snapshot = capture(root, true);
    let mut plan = sync::plan_mirror_files(
        &snapshot,
        &root.join("backups"),
        &AtomicBool::new(false),
        Side::Left,
    )
    .unwrap();
    plan.actions.pop();
    assert!(sync::execute(&snapshot, &plan, &AtomicBool::new(false)).is_err());
    assert!(!Path::new(&plan.backup_root).exists());
    assert_eq!(fs::read(root.join("r/a")).unwrap(), b"a");
    assert_eq!(fs::read(root.join("r/b")).unwrap(), b"b");
    plan.actions[0].operation = FileOperation::Copy;
    assert!(sync::execute(&snapshot, &plan, &AtomicBool::new(false)).is_err());
    assert!(!Path::new(&plan.backup_root).exists());
}
#[test]
fn excluded_changes_do_not_expire_mirror_and_cancellation_prevents_mutation() {
    let t = fixture();
    let root = t.path();
    fs::write(root.join("r/extra"), b"preserve").unwrap();
    let snapshot = capture(root, true);
    let plan = sync::plan_mirror_files(
        &snapshot,
        &root.join("backups"),
        &AtomicBool::new(false),
        Side::Left,
    )
    .unwrap();
    assert!(sync::execute(&snapshot, &plan, &AtomicBool::new(true)).is_err());
    assert!(!Path::new(&plan.backup_root).exists());
    fs::write(root.join("r/keep.txt"), b"new excluded").unwrap();
    let out = sync::execute(&snapshot, &plan, &AtomicBool::new(false)).unwrap();
    assert_eq!(out.actions[0].state, "completed");
    assert_eq!(fs::read(root.join("r/keep.txt")).unwrap(), b"new excluded");
}
#[cfg(unix)]
#[test]
fn links_anywhere_in_the_included_scope_block_mirror_instead_of_being_treated_as_absent() {
    let t = fixture();
    let root = t.path();
    fs::write(root.join("r/extra"), b"preserve").unwrap();
    std::os::unix::fs::symlink(root.join("r/extra"), root.join("l/link")).unwrap();
    let snapshot = capture(root, true);
    assert!(
        sync::plan_mirror_files(
            &snapshot,
            &root.join("backups"),
            &AtomicBool::new(false),
            Side::Left
        )
        .is_err()
    );
    assert_eq!(fs::read(root.join("r/extra")).unwrap(), b"preserve");
}
