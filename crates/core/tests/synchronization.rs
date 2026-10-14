use clear_contracts::{CompareRequest, Status};
use clear_core::{compare, sync};
use std::{fs, sync::atomic::AtomicBool};

fn request(left: &std::path::Path, right: &std::path::Path) -> CompareRequest {
    CompareRequest {
        left: left.to_string_lossy().into_owned(),
        right: right.to_string_lossy().into_owned(),
        exclusions: "-README.md;-./docs/".into(),
        content: true,
    }
}

#[test]
fn copies_with_verified_backup_preserving_exclusions_and_target_only() {
    let base = tempfile::tempdir().unwrap();
    let left = base.path().join("left");
    let right = base.path().join("right");
    let backups = base.path().join("backups");
    for dir in [&left, &right, &backups] {
        fs::create_dir(dir).unwrap();
    }
    fs::write(left.join("changed.txt"), "new").unwrap();
    fs::write(right.join("changed.txt"), "old").unwrap();
    fs::write(left.join("new.txt"), "created").unwrap();
    fs::write(left.join("README.md"), "source").unwrap();
    fs::write(right.join("README.md"), "preserved").unwrap();
    fs::write(right.join("only.txt"), "keep").unwrap();
    let cancel = AtomicBool::new(false);
    let s = compare::compare(&request(&left, &right), &cancel).unwrap();
    let ids = s
        .report
        .rows
        .iter()
        .filter(|r| matches!(r.status, Status::Changed | Status::LeftOnly))
        .map(|r| r.id.clone())
        .collect::<Vec<_>>();
    let p = sync::plan(&s, &ids, &backups, &cancel).unwrap();
    let result = sync::execute(&s, &p, &cancel).unwrap();
    assert!(
        result.actions.iter().all(|a| a.state == "completed"),
        "{:?}",
        result.actions
    );
    assert_eq!(
        fs::read_to_string(right.join("changed.txt")).unwrap(),
        "new"
    );
    assert_eq!(
        fs::read_to_string(std::path::Path::new(&p.backup_root).join("files/changed.txt")).unwrap(),
        "old"
    );
    assert_eq!(
        fs::read_to_string(right.join("README.md")).unwrap(),
        "preserved"
    );
    assert!(right.join("only.txt").exists());
    assert!(std::path::Path::new(&result.journal).exists());
}

#[test]
fn external_changes_after_preview_are_not_overwritten() {
    let base = tempfile::tempdir().unwrap();
    for name in ["l", "r", "b"] {
        fs::create_dir(base.path().join(name)).unwrap();
    }
    let (l, r, b) = (
        base.path().join("l"),
        base.path().join("r"),
        base.path().join("b"),
    );
    fs::write(l.join("a"), "new").unwrap();
    fs::write(r.join("a"), "old").unwrap();
    let c = AtomicBool::new(false);
    let s = compare::compare(&request(&l, &r), &c).unwrap();
    let p = sync::plan(&s, &[s.report.rows[0].id.clone()], &b, &c).unwrap();
    fs::write(r.join("a"), "external").unwrap();
    let result = sync::execute(&s, &p, &c).unwrap();
    assert_eq!(result.actions[0].state, "failed");
    assert_eq!(fs::read_to_string(r.join("a")).unwrap(), "external");
}

#[test]
fn empty_or_nested_roots_and_cancelled_scans_are_rejected() {
    let b = tempfile::tempdir().unwrap();
    let child = b.path().join("child");
    fs::create_dir(&child).unwrap();
    assert!(compare::compare(&request(b.path(), &child), &AtomicBool::new(false)).is_err());
    let other = tempfile::tempdir().unwrap();
    assert!(compare::compare(&request(b.path(), other.path()), &AtomicBool::new(true)).is_err());
}

#[cfg(unix)]
#[test]
fn destination_symlink_cannot_escape_the_selected_root() {
    use std::os::unix::fs::symlink;
    let b = tempfile::tempdir().unwrap();
    for n in ["l", "r", "backup", "outside"] {
        fs::create_dir(b.path().join(n)).unwrap();
    }
    fs::create_dir(b.path().join("l/sub")).unwrap();
    fs::write(b.path().join("l/sub/file"), "new").unwrap();
    let c = AtomicBool::new(false);
    let s = compare::compare(&request(&b.path().join("l"), &b.path().join("r")), &c).unwrap();
    let id = s
        .report
        .rows
        .iter()
        .find(|r| r.path == "sub/file")
        .unwrap()
        .id
        .clone();
    let p = sync::plan(&s, &[id], &b.path().join("backup"), &c).unwrap();
    symlink(b.path().join("outside"), b.path().join("r/sub")).unwrap();
    let result = sync::execute(&s, &p, &c).unwrap();
    assert_eq!(result.actions[0].state, "failed");
    assert!(!b.path().join("outside/file").exists());
}

#[test]
fn reverse_copy_uses_right_source_and_left_backup() {
    let t = tempfile::tempdir().unwrap();
    for name in ["l", "r", "b"] {
        fs::create_dir(t.path().join(name)).unwrap();
    }
    fs::write(t.path().join("l/a"), "old left").unwrap();
    fs::write(t.path().join("r/a"), "new right").unwrap();
    fs::write(t.path().join("r/new"), "added").unwrap();
    let req = clear_contracts::CompareRequest {
        left: t.path().join("l").to_string_lossy().into(),
        right: t.path().join("r").to_string_lossy().into(),
        exclusions: String::new(),
        content: true,
    };
    let cancel = AtomicBool::new(false);
    let s = compare::compare(&req, &cancel).unwrap();
    let ids = s
        .report
        .rows
        .iter()
        .map(|r| r.id.clone())
        .collect::<Vec<_>>();
    let p = sync::plan_direction(
        &s,
        &ids,
        &t.path().join("b"),
        &cancel,
        clear_contracts::Side::Right,
    )
    .unwrap();
    let result = sync::execute(&s, &p, &cancel).unwrap();
    assert!(result.actions.iter().all(|a| a.state == "completed"));
    assert_eq!(
        fs::read_to_string(t.path().join("l/a")).unwrap(),
        "new right"
    );
    assert_eq!(fs::read_to_string(t.path().join("l/new")).unwrap(), "added");
    assert_eq!(
        fs::read_to_string(std::path::Path::new(&p.backup_root).join("files/a")).unwrap(),
        "old left"
    );
}
