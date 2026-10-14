use clear_contracts::{CompareRequest, Side};
use clear_core::{compare, recovery, sync};
use std::{fs, path::Path, sync::atomic::AtomicBool};
fn copy_fixture(root: &Path) -> (clear_core::compare::Snapshot, clear_contracts::SyncResult) {
    for name in ["l", "r", "b", "recover"] {
        fs::create_dir(root.join(name)).unwrap();
    }
    fs::write(root.join("l/a"), "new").unwrap();
    fs::write(root.join("r/a"), "old").unwrap();
    fs::write(root.join("l/added"), "added").unwrap();
    fs::write(root.join("r/keep"), "keep").unwrap();
    let s = compare::compare(
        &CompareRequest {
            left: root.join("l").to_string_lossy().into(),
            right: root.join("r").to_string_lossy().into(),
            exclusions: String::new(),
            content: true,
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    let ids = s
        .report
        .rows
        .iter()
        .filter(|r| r.path != "keep")
        .map(|r| r.id.clone())
        .collect::<Vec<_>>();
    let p = sync::plan(&s, &ids, &root.join("b"), &AtomicBool::new(false)).unwrap();
    let result = sync::execute(&s, &p, &AtomicBool::new(false)).unwrap();
    (s, result)
}
#[test]
fn journal_reopens_after_snapshot_drop_and_rolls_back_copies_with_backups() {
    let t = tempfile::tempdir().unwrap();
    let (s, result) = copy_fixture(t.path());
    drop(s);
    let plan = recovery::preview(
        Path::new(&result.journal),
        &t.path().join("recover"),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(plan.view.actions.len(), 2);
    assert!(
        plan.view
            .actions
            .iter()
            .any(|a| a.operation == "remove_created")
    );
    let recovered = recovery::execute(&plan, &AtomicBool::new(false)).unwrap();
    assert!(recovered.actions.iter().all(|a| a.state == "completed"));
    assert_eq!(fs::read_to_string(t.path().join("r/a")).unwrap(), "old");
    assert!(!t.path().join("r/added").exists());
    assert_eq!(fs::read_to_string(t.path().join("r/keep")).unwrap(), "keep");
    assert_eq!(
        fs::read_to_string(Path::new(&plan.view.backup_root).join("files/a")).unwrap(),
        "new"
    );
    assert_eq!(
        fs::read_to_string(Path::new(&plan.view.backup_root).join("files/added")).unwrap(),
        "added"
    );
    assert!(recovery::execute(&plan, &AtomicBool::new(false)).is_err());
}
#[test]
fn external_change_after_preview_is_not_overwritten() {
    let t = tempfile::tempdir().unwrap();
    let (_, result) = copy_fixture(t.path());
    let plan = recovery::preview(
        Path::new(&result.journal),
        &t.path().join("recover"),
        &AtomicBool::new(false),
    )
    .unwrap();
    fs::write(t.path().join("r/a"), "external").unwrap();
    let recovered = recovery::execute(&plan, &AtomicBool::new(false)).unwrap();
    assert_eq!(recovered.actions[0].state, "failed");
    assert_eq!(
        fs::read_to_string(t.path().join("r/a")).unwrap(),
        "external"
    );
    assert!(t.path().join("r/added").exists());
}
#[test]
fn reverse_copy_recovery_and_running_state_reconciliation() {
    let t = tempfile::tempdir().unwrap();
    for name in ["l", "r", "b", "recover"] {
        fs::create_dir(t.path().join(name)).unwrap();
    }
    fs::write(t.path().join("l/a"), "left").unwrap();
    fs::write(t.path().join("r/a"), "right").unwrap();
    let s = compare::compare(
        &CompareRequest {
            left: t.path().join("l").to_string_lossy().into(),
            right: t.path().join("r").to_string_lossy().into(),
            exclusions: String::new(),
            content: true,
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    let p = sync::plan_direction(
        &s,
        &[s.report.rows[0].id.clone()],
        &t.path().join("b"),
        &AtomicBool::new(false),
        Side::Right,
    )
    .unwrap();
    let mut result = sync::execute(&s, &p, &AtomicBool::new(false)).unwrap();
    result.actions[0].state = "running".into();
    fs::write(&result.journal, serde_json::to_vec(&result).unwrap()).unwrap();
    let plan = recovery::preview(
        Path::new(&result.journal),
        &t.path().join("recover"),
        &AtomicBool::new(false),
    )
    .unwrap();
    recovery::execute(&plan, &AtomicBool::new(false)).unwrap();
    assert_eq!(fs::read_to_string(t.path().join("l/a")).unwrap(), "left");
}
#[cfg(target_os = "linux")]
#[test]
fn non_unicode_entry_paths_round_trip_losslessly() {
    use std::os::unix::ffi::OsStringExt;
    let t = tempfile::tempdir().unwrap();
    for name in ["l", "r", "b", "recover"] {
        fs::create_dir(t.path().join(name)).unwrap();
    }
    let name = std::ffi::OsString::from_vec(vec![b'a', 255]);
    fs::write(t.path().join("l").join(&name), "new").unwrap();
    fs::write(t.path().join("r").join(&name), "old").unwrap();
    let s = compare::compare(
        &CompareRequest {
            left: t.path().join("l").to_string_lossy().into(),
            right: t.path().join("r").to_string_lossy().into(),
            exclusions: String::new(),
            content: true,
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    let p = sync::plan(
        &s,
        &[s.report.rows[0].id.clone()],
        &t.path().join("b"),
        &AtomicBool::new(false),
    )
    .unwrap();
    let result = sync::execute(&s, &p, &AtomicBool::new(false)).unwrap();
    let plan = recovery::preview(
        Path::new(&result.journal),
        &t.path().join("recover"),
        &AtomicBool::new(false),
    )
    .unwrap();
    recovery::execute(&plan, &AtomicBool::new(false)).unwrap();
    assert_eq!(
        fs::read_to_string(t.path().join("r").join(name)).unwrap(),
        "old"
    );
}

#[test]
fn changed_created_file_and_tampered_original_backup_are_not_replaced() {
    let t = tempfile::tempdir().unwrap();
    let (_, result) = copy_fixture(t.path());
    fs::write(t.path().join("r/added"), "external new").unwrap();
    assert!(
        recovery::preview(
            Path::new(&result.journal),
            &t.path().join("recover"),
            &AtomicBool::new(false)
        )
        .is_err()
    );
    assert_eq!(fs::read_to_string(t.path().join("r/a")).unwrap(), "new");
    fs::write(t.path().join("r/added"), "added").unwrap();
    let plan = recovery::preview(
        Path::new(&result.journal),
        &t.path().join("recover"),
        &AtomicBool::new(false),
    )
    .unwrap();
    fs::write(
        Path::new(&result.plan.backup_root).join("files/a"),
        "tampered",
    )
    .unwrap();
    let recovered = recovery::execute(&plan, &AtomicBool::new(false)).unwrap();
    assert_eq!(recovered.actions[0].state, "failed");
    assert_eq!(fs::read_to_string(t.path().join("r/a")).unwrap(), "new");
    assert_eq!(
        fs::read_to_string(t.path().join("r/added")).unwrap(),
        "added"
    );
}

#[test]
fn older_journals_without_native_paths_return_an_explicit_recovery_error() {
    let t = tempfile::tempdir().unwrap();
    let (_, result) = copy_fixture(t.path());
    let mut value = serde_json::to_value(&result).unwrap();
    let plan = value["plan"].as_object_mut().unwrap();
    plan.remove("target_native");
    plan.remove("source_side");
    for action in plan["actions"].as_array_mut().unwrap() {
        action.as_object_mut().unwrap().remove("relative_native");
    }
    fs::write(&result.journal, serde_json::to_vec(&value).unwrap()).unwrap();
    let error = match recovery::preview(
        Path::new(&result.journal),
        &t.path().join("recover"),
        &AtomicBool::new(false),
    ) {
        Ok(_) => panic!("legacy recovery should be rejected"),
        Err(e) => e,
    };
    assert!(
        error
            .to_string()
            .contains("older journal lacks durable native paths")
    );
}
