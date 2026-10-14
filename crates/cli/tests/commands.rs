//! Process-level contracts: exit codes, JSON output and preview/apply boundaries.
use std::{
    fs,
    path::Path,
    process::{Command, Output},
};
fn run(arguments: &[&std::ffi::OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_clear-compare"))
        .args(arguments)
        .output()
        .unwrap()
}
#[test]
fn text_and_table_commands_report_semantic_exit_codes() {
    let t = tempfile::tempdir().unwrap();
    let left = t.path().join("left");
    let right = t.path().join("right");
    fs::write(&left, "hello\r\n").unwrap();
    fs::write(&right, "hello\n").unwrap();
    let strict = run(&["diff".as_ref(), left.as_os_str(), right.as_os_str()]);
    assert_eq!(strict.status.code(), Some(1));
    let relaxed = run(&[
        "diff".as_ref(),
        left.as_os_str(),
        right.as_os_str(),
        "--ignore-line-endings".as_ref(),
    ]);
    assert!(relaxed.status.success());
    let json: serde_json::Value = serde_json::from_slice(&relaxed.stdout).unwrap();
    assert_eq!(json["lines"][0]["right_text"], "hello\n");
    fs::write(&left, "id,value\na,1\nb,2\n").unwrap();
    fs::write(&right, "id,value\nb,2\na,1.005\n").unwrap();
    let table = run(&[
        "table".as_ref(),
        left.as_os_str(),
        right.as_os_str(),
        "--key-column".as_ref(),
        "0".as_ref(),
        "--numeric-tolerance".as_ref(),
        "0.01".as_ref(),
    ]);
    assert!(
        table.status.success(),
        "{}",
        String::from_utf8_lossy(&table.stderr)
    );
    fs::write(&right, "id,value\na,2\na,3\n").unwrap();
    let invalid = run(&[
        "table".as_ref(),
        left.as_os_str(),
        right.as_os_str(),
        "--key-column".as_ref(),
        "0".as_ref(),
    ]);
    assert_eq!(invalid.status.code(), Some(2));
}
#[test]
fn editor_restore_preview_does_not_write_and_apply_creates_a_reversal_backup() {
    let t = tempfile::tempdir().unwrap();
    fs::create_dir(t.path().join("files")).unwrap();
    let target = t.path().join("files/edited.txt");
    let backups = t.path().join("backups");
    fs::create_dir(&backups).unwrap();
    fs::write(&target, "original\r\n").unwrap();
    let document = clear_core::editing::open_file(&target).unwrap();
    let (_, saved) = clear_core::editing::save(&document, "changed\n", &backups).unwrap();
    let backup = Path::new(&saved.backup);
    let preview = run(&[
        "restore-editor".as_ref(),
        target.as_os_str(),
        backup.as_os_str(),
        "--backup-dir".as_ref(),
        backups.as_os_str(),
    ]);
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    assert_eq!(fs::read_to_string(&target).unwrap(), "changed\r\n");
    let applied = run(&[
        "restore-editor".as_ref(),
        target.as_os_str(),
        backup.as_os_str(),
        "--backup-dir".as_ref(),
        backups.as_os_str(),
        "--apply".as_ref(),
    ]);
    assert!(
        applied.status.success(),
        "{}",
        String::from_utf8_lossy(&applied.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&applied.stdout).unwrap();
    assert_eq!(fs::read_to_string(&target).unwrap(), "original\r\n");
    assert_eq!(
        fs::read_to_string(result["backup"].as_str().unwrap()).unwrap(),
        "changed\r\n"
    );
}
