use clear_contracts::{CompareRequest, Side, Status};
use clear_core::{compare, compare_refresh, sync};
use std::{fs, sync::atomic::AtomicBool};
#[test]
fn quick_copy_refresh_verifies_only_the_pair_and_updates_ancestors_without_changing_ids() {
    let temp = tempfile::tempdir().unwrap();
    let l = temp.path().join("left");
    let r = temp.path().join("right");
    fs::create_dir_all(l.join("src")).unwrap();
    fs::create_dir_all(r.join("src")).unwrap();
    fs::write(l.join("src/a.txt"), "new content").unwrap();
    fs::write(r.join("src/a.txt"), "old").unwrap();
    fs::write(l.join("other.txt"), "source only").unwrap();
    let cancel = AtomicBool::new(false);
    let mut snapshot = compare::compare(
        &CompareRequest {
            left: l.to_string_lossy().into(),
            right: r.to_string_lossy().into(),
            exclusions: String::new(),
            content: false,
        },
        &cancel,
    )
    .unwrap();
    let id = snapshot
        .report
        .rows
        .iter()
        .find(|r| r.path == "src/a.txt")
        .unwrap()
        .id
        .clone();
    let ids = snapshot.paths.clone();
    fs::create_dir(temp.path().join("backups")).unwrap();
    let plan = sync::plan_direction(
        &snapshot,
        std::slice::from_ref(&id),
        &temp.path().join("backups"),
        &cancel,
        Side::Left,
    )
    .unwrap();
    let reply = sync::execute(&snapshot, &plan, &cancel).unwrap();
    assert!(reply.actions.iter().all(|a| a.state == "completed"));
    compare_refresh::file(&mut snapshot, &id, &cancel).unwrap();
    assert_eq!(snapshot.paths, ids);
    assert_eq!(snapshot.report.summary.same, 1);
    assert_eq!(snapshot.report.summary.left_only, 1);
    assert_eq!(
        snapshot
            .report
            .rows
            .iter()
            .find(|r| r.path == "src")
            .unwrap()
            .status,
        Status::Same
    );
    let row = snapshot.report.rows.iter().find(|r| r.id == id).unwrap();
    assert_eq!(
        row.left.as_ref().unwrap().digest,
        row.right.as_ref().unwrap().digest
    );
    assert_eq!(
        fs::read_to_string(l.join("src/a.txt")).unwrap(),
        "new content"
    );
    fs::write(r.join("src/a.txt"), "changed again").unwrap();
    compare_refresh::file(&mut snapshot, &id, &cancel).unwrap();
    assert_eq!(snapshot.report.summary.changed, 1);
    assert!(compare_refresh::file(&mut snapshot, "unknown", &cancel).is_err());
    cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(compare_refresh::file(&mut snapshot, &id, &cancel).is_err());
    assert_eq!(snapshot.report.summary.changed, 1);
}
#[cfg(unix)]
#[test]
fn refresh_rejects_replaced_symlinks_without_modifying_the_snapshot() {
    let temp = tempfile::tempdir().unwrap();
    let l = temp.path().join("l");
    let r = temp.path().join("r");
    fs::create_dir(&l).unwrap();
    fs::create_dir(&r).unwrap();
    fs::write(l.join("a"), "source").unwrap();
    fs::write(r.join("a"), "old").unwrap();
    let cancel = AtomicBool::new(false);
    let mut s = compare::compare(
        &CompareRequest {
            left: l.to_string_lossy().into(),
            right: r.to_string_lossy().into(),
            exclusions: String::new(),
            content: true,
        },
        &cancel,
    )
    .unwrap();
    let id = s.report.rows[0].id.clone();
    let digest = s.report.rows[0].right.as_ref().unwrap().digest.clone();
    fs::remove_file(r.join("a")).unwrap();
    std::os::unix::fs::symlink(l.join("a"), r.join("a")).unwrap();
    assert!(compare_refresh::file(&mut s, &id, &cancel).is_err());
    assert_eq!(s.report.rows[0].right.as_ref().unwrap().digest, digest);
}
