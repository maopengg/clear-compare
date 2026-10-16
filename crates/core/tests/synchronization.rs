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

#[test]
fn single_file_copy_in_large_inventory_has_a_scoped_restart_record() {
    use std::time::Instant;
    let base = tempfile::tempdir().unwrap();
    for name in ["left", "right", "backups", "undo"] {
        fs::create_dir(base.path().join(name)).unwrap();
    }
    let (left, right, backups) = (
        base.path().join("left"),
        base.path().join("right"),
        base.path().join("backups"),
    );
    fs::write(left.join("code.txt"), vec![b'l'; 24_000]).unwrap();
    fs::write(right.join("code.txt"), vec![b'r'; 4_700]).unwrap();
    let cancel = AtomicBool::new(false);
    let mut snapshot = compare::compare(&request(&left, &right), &cancel).unwrap();
    let selected = snapshot.report.rows[0].id.clone();
    // Captured unrelated entries need no on-disk fixture: selected copy must never read them.
    let template = snapshot.report.rows[0].clone();
    for i in 1..105_000 {
        let mut row = template.clone();
        row.id = i.to_string();
        row.path = format!("unrelated/item-{i}.txt");
        snapshot
            .paths
            .insert(row.id.clone(), std::path::PathBuf::from(&row.path));
        snapshot.report.rows.push(row);
    }
    let text_start = Instant::now();
    let text = clear_core::text::preview(&snapshot, &selected).unwrap();
    eprintln!(
        "105000-entry text preview: {:?}, {} diff rows",
        text_start.elapsed(),
        text.lines.len()
    );
    let start = Instant::now();
    let plan = sync::plan_direction(
        &snapshot,
        &[selected],
        &backups,
        &cancel,
        clear_contracts::Side::Right,
    )
    .unwrap();
    let planned = start.elapsed();
    let result = sync::execute(&snapshot, &plan, &cancel).unwrap();
    let elapsed = start.elapsed();
    assert!(
        result.actions.iter().all(|a| a.state == "completed"),
        "{:?}",
        result.actions
    );
    assert_eq!(fs::read(left.join("code.txt")).unwrap(), vec![b'r'; 4_700]);
    assert_eq!(fs::read(right.join("code.txt")).unwrap(), vec![b'r'; 4_700]);
    let intent = std::path::Path::new(&plan.backup_root).join("intent.json");
    let bytes = fs::read(&intent).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    eprintln!(
        "105000-entry selected copy: plan={planned:?}, total={elapsed:?}, intent={} bytes / {} rows",
        bytes.len(),
        value["rows"].as_array().unwrap().len()
    );
    assert_eq!(value["rows"].as_array().unwrap().len(), 1);
    assert!(bytes.len() < 32_000);
    let undo = clear_core::recovery::preview(
        std::path::Path::new(&result.journal),
        &base.path().join("undo"),
        &cancel,
    )
    .unwrap();
    let recovered = clear_core::recovery::execute(&undo, &cancel).unwrap();
    assert!(recovered.actions.iter().all(|a| a.state == "completed"));
    assert_eq!(fs::read(left.join("code.txt")).unwrap(), vec![b'l'; 24_000]);
    assert_eq!(fs::read(right.join("code.txt")).unwrap(), vec![b'r'; 4_700]);
}
