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

#[test]
fn synchronization_recovery_cli_defaults_to_preview_and_backs_up_removed_creations() {
    let t = tempfile::tempdir().unwrap();
    for name in ["l", "r", "b", "recover"] {
        fs::create_dir(t.path().join(name)).unwrap();
    }
    fs::write(t.path().join("l/new"), "content").unwrap();
    let sync = run(&[
        "sync".as_ref(),
        t.path().join("l").as_os_str(),
        t.path().join("r").as_os_str(),
        "--backup-dir".as_ref(),
        t.path().join("b").as_os_str(),
        "--apply".as_ref(),
    ]);
    assert!(
        sync.status.success(),
        "{}",
        String::from_utf8_lossy(&sync.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&sync.stdout).unwrap();
    let journal = Path::new(result["journal"].as_str().unwrap());
    let preview = run(&[
        "recover-sync".as_ref(),
        journal.as_os_str(),
        "--backup-dir".as_ref(),
        t.path().join("recover").as_os_str(),
    ]);
    assert!(preview.status.success());
    assert!(t.path().join("r/new").exists());
    let recovered = run(&[
        "recover-sync".as_ref(),
        journal.as_os_str(),
        "--backup-dir".as_ref(),
        t.path().join("recover").as_os_str(),
        "--apply".as_ref(),
    ]);
    assert!(
        recovered.status.success(),
        "{}",
        String::from_utf8_lossy(&recovered.stderr)
    );
    let result: serde_json::Value = serde_json::from_slice(&recovered.stdout).unwrap();
    assert!(!t.path().join("r/new").exists());
    assert_eq!(
        fs::read_to_string(
            Path::new(result["plan"]["backup_root"].as_str().unwrap()).join("files/new")
        )
        .unwrap(),
        "content"
    );
}

#[test]
fn directory_only_differences_produce_a_difference_exit_code() {
    let t = tempfile::tempdir().unwrap();
    for name in ["l/new-empty", "r"] {
        fs::create_dir_all(t.path().join(name)).unwrap();
    }
    let output = run(&[
        "compare".as_ref(),
        t.path().join("l").as_os_str(),
        t.path().join("r").as_os_str(),
    ]);
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn picture_cli_reports_pixel_changes_and_tolerance() {
    fn bmp(red: u8) -> Vec<u8> {
        let mut data = vec![0u8; 58];
        data[..2].copy_from_slice(b"BM");
        data[2..6].copy_from_slice(&58u32.to_le_bytes());
        data[10..14].copy_from_slice(&54u32.to_le_bytes());
        data[14..18].copy_from_slice(&40u32.to_le_bytes());
        data[18..22].copy_from_slice(&1u32.to_le_bytes());
        data[22..26].copy_from_slice(&1u32.to_le_bytes());
        data[26..28].copy_from_slice(&1u16.to_le_bytes());
        data[28..30].copy_from_slice(&24u16.to_le_bytes());
        data[34..38].copy_from_slice(&4u32.to_le_bytes());
        data[56] = red;
        data
    }
    let t = tempfile::tempdir().unwrap();
    let left = t.path().join("left.bmp");
    let right = t.path().join("right.bmp");
    fs::write(&left, bmp(10)).unwrap();
    fs::write(&right, bmp(11)).unwrap();
    let result = run(&["picture".as_ref(), left.as_os_str(), right.as_os_str()]);
    assert_eq!(
        result.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let summary: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(summary["changed_pixels"], "1");
    let tolerant = run(&[
        "picture".as_ref(),
        left.as_os_str(),
        right.as_os_str(),
        "--tolerance".as_ref(),
        "1".as_ref(),
    ]);
    assert!(tolerant.status.success());
}

#[test]
fn folder_merge_cli_preview_is_read_only_and_apply_creates_separate_output() {
    let t = tempfile::tempdir().unwrap();
    for name in ["base", "left", "right", "out", "journals"] {
        fs::create_dir(t.path().join(name)).unwrap();
    }
    for (side, text) in [
        ("base", "a\nb\nc\n"),
        ("left", "A\nb\nc\n"),
        ("right", "a\nb\nC\n"),
    ] {
        fs::write(t.path().join(side).join("a"), text).unwrap();
    }
    let paths = ["base", "left", "right", "out", "journals"].map(|name| t.path().join(name));
    let args = [
        "merge-folders".as_ref(),
        paths[0].as_os_str(),
        paths[1].as_os_str(),
        paths[2].as_os_str(),
        paths[3].as_os_str(),
        "--journal-dir".as_ref(),
        paths[4].as_os_str(),
    ];
    let preview = run(&args);
    assert!(
        preview.status.success(),
        "{}",
        String::from_utf8_lossy(&preview.stderr)
    );
    assert!(fs::read_dir(t.path().join("out")).unwrap().next().is_none());
    let mut apply = args.to_vec();
    apply.push("--apply".as_ref());
    let output = run(&apply);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::read_to_string(t.path().join("out/a")).unwrap(),
        "A\nb\nC\n"
    );
    assert_eq!(
        fs::read_to_string(t.path().join("base/a")).unwrap(),
        "a\nb\nc\n"
    );
}

#[test]
fn two_way_folder_merge_cli_accepts_explicit_version_choice() {
    let t = tempfile::tempdir().unwrap();
    let paths = ["left", "right", "out", "journals"].map(|n| t.path().join(n));
    for p in &paths {
        fs::create_dir(p).unwrap();
    }
    fs::write(paths[0].join("a"), "left").unwrap();
    fs::write(paths[1].join("a"), "right").unwrap();
    let result = run(&[
        "merge-folders".as_ref(),
        "-".as_ref(),
        paths[0].as_os_str(),
        paths[1].as_os_str(),
        paths[2].as_os_str(),
        "--journal-dir".as_ref(),
        paths[3].as_os_str(),
        "--choose".as_ref(),
        "a=right".as_ref(),
        "--apply".as_ref(),
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(fs::read_to_string(paths[2].join("a")).unwrap(), "right");
}
