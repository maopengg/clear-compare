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

#[test]
fn archive_cli_browses_without_extracting_and_reports_differences() {
    use std::io::Write;
    let t = tempfile::tempdir().unwrap();
    let z = t.path().join("a.zip");
    let directory = t.path().join("folder");
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("a.txt"), "same").unwrap();
    let mut writer = zip::ZipWriter::new(fs::File::create(&z).unwrap());
    writer
        .start_file(
            "a.txt",
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )
        .unwrap();
    writer.write_all(b"same").unwrap();
    writer.finish().unwrap();
    let browse = run(&["archives".as_ref(), z.as_os_str()]);
    assert!(
        browse.status.success(),
        "{}",
        String::from_utf8_lossy(&browse.stderr)
    );
    let equal = run(&["archives".as_ref(), z.as_os_str(), directory.as_os_str()]);
    assert!(equal.status.success());
    fs::write(directory.join("a.txt"), "different").unwrap();
    let changed = run(&["archives".as_ref(), z.as_os_str(), directory.as_os_str()]);
    assert_eq!(changed.status.code(), Some(1));
    assert_eq!(fs::read_dir(t.path()).unwrap().count(), 2);
}

#[test]
fn snapshot_cli_exports_metadata_without_content_and_never_overwrites() {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("folder");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("a"), b"SECRET-ABCDEF").unwrap();
    let file = t.path().join("saved.cctree");
    let args = ["snapshot".as_ref(), root.as_os_str(), file.as_os_str()];
    let result = run(&args);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(!fs::read_to_string(&file).unwrap().contains("SECRET-ABCDEF"));
    assert_eq!(run(&args).status.code(), Some(2));
    assert!(
        run(&["snapshots".as_ref(), file.as_os_str()])
            .status
            .success()
    );
    let equal = run(&["snapshots".as_ref(), file.as_os_str(), root.as_os_str()]);
    assert_eq!(equal.status.code(), Some(1));
    let json: serde_json::Value = serde_json::from_slice(&equal.stdout).unwrap();
    assert_eq!(json["summary"]["unverified"], 1);
    assert_eq!(json["summary"]["same"], 0);
}

#[test]
fn git_cli_compares_local_commits_and_reports_invalid_revisions() {
    let t = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        let result = std::process::Command::new("git")
            .current_dir(t.path())
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=",
                "-c",
                "core.autocrlf=false",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    };
    git(&["init", "-q", "--template="]);
    fs::write(t.path().join("a"), "first").unwrap();
    git(&["add", "a"]);
    git(&["commit", "-qm", "first"]);
    fs::write(t.path().join("a"), "second").unwrap();
    git(&["add", "a"]);
    git(&["commit", "-qm", "second"]);
    let compare = |a: &str, b: &str| {
        run(&[
            "git-compare".as_ref(),
            t.path().as_os_str(),
            a.as_ref(),
            b.as_ref(),
        ])
    };
    assert!(compare("HEAD", "HEAD").status.success());
    let changed = compare("HEAD~1", "HEAD");
    assert_eq!(changed.status.code(), Some(1));
    let data: serde_json::Value = serde_json::from_slice(&changed.stdout).unwrap();
    assert_eq!(data["summary"]["changed"], 1);
    assert_eq!(compare("invalid-ref", "HEAD").status.code(), Some(2));
}

#[test]
fn report_cli_exports_full_native_results_and_keeps_difference_exit_codes() {
    let t = tempfile::tempdir().unwrap();
    let l = t.path().join("left");
    let r = t.path().join("right");
    fs::create_dir(&l).unwrap();
    fs::create_dir(&r).unwrap();
    fs::write(l.join("a"), "left body").unwrap();
    fs::write(r.join("a"), "right body").unwrap();
    let output = t.path().join("report.json");
    let args = [
        "report".as_ref(),
        l.as_os_str(),
        r.as_os_str(),
        output.as_os_str(),
        "--format".as_ref(),
        "json".as_ref(),
        "--differences-only".as_ref(),
    ];
    let result = run(&args);
    assert_eq!(
        result.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let saved = fs::read(&output).unwrap();
    let data: serde_json::Value = serde_json::from_slice(&saved).unwrap();
    assert_eq!(data["source"], "local");
    assert_eq!(data["rows"].as_array().unwrap().len(), 1);
    assert_eq!(data["summary"]["changed"], 1);
    let status: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(status["rows"], 1);
    assert_eq!(status["bytes"], saved.len().to_string());
    assert_eq!(run(&args).status.code(), Some(2));
    assert_eq!(fs::read(&output).unwrap(), saved);
    let html = t.path().join("report.html");
    let result = run(&[
        "report".as_ref(),
        l.as_os_str(),
        r.as_os_str(),
        html.as_os_str(),
        "--english".as_ref(),
    ]);
    assert_eq!(result.status.code(), Some(1));
    let html = fs::read_to_string(html).unwrap();
    assert!(html.contains("Comparison report"));
    assert!(!html.contains("left body"));
}
#[test]
fn git_report_cli_uses_pinned_committed_objects() {
    let repo = tempfile::tempdir().unwrap();
    let output = tempfile::tempdir().unwrap();
    for args in [
        vec!["init", "-q", "--template="],
        vec!["add", "a"],
        vec!["commit", "-qm", "initial"],
    ] {
        if args[0] == "add" {
            fs::write(repo.path().join("a"), "committed body").unwrap();
        }
        let r = std::process::Command::new("git")
            .current_dir(repo.path())
            .args([
                "-c",
                "user.name=Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=",
                "-c",
                "core.autocrlf=false",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    }
    fs::write(repo.path().join("a"), "uncommitted").unwrap();
    let file = output.path().join("git.json");
    let r = run(&[
        "report".as_ref(),
        "HEAD".as_ref(),
        "HEAD".as_ref(),
        file.as_os_str(),
        "--source".as_ref(),
        "git".as_ref(),
        "--repository".as_ref(),
        repo.path().as_os_str(),
        "--format".as_ref(),
        "json".as_ref(),
    ]);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stderr));
    let json: serde_json::Value = serde_json::from_slice(&fs::read(file).unwrap()).unwrap();
    assert_eq!(json["source"], "git");
    assert_eq!(json["policy"]["verification"], "git_object_identity");
    assert_eq!(json["summary"]["same"], 1);
    assert_eq!(
        fs::read_to_string(repo.path().join("a")).unwrap(),
        "uncommitted"
    );
}

#[test]
fn html_table_cli_selects_each_side_and_lists_static_catalogs() {
    let t = tempfile::tempdir().unwrap();
    let html = t.path().join("a.html");
    let csv = t.path().join("b.csv");
    fs::write(&html,"<table><caption>wrong</caption><tr><th>x<tr><td>no</table><table id='data'><caption>Data</caption><tr><th>id<th>value<tr><td>b<td>2<tr><td>a<td>1.005</table>").unwrap();
    fs::write(&csv, "id,value\na,1.000\nb,2\n").unwrap();
    let result = run(&[
        "table".as_ref(),
        html.as_os_str(),
        csv.as_os_str(),
        "--left-html".as_ref(),
        "1".as_ref(),
        "--key-column".as_ref(),
        "0".as_ref(),
        "--numeric-tolerance".as_ref(),
        "0.01".as_ref(),
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(report["summary"]["same"], 2);
    assert_eq!(report["left_tables"][1]["element_id"], "data");
    assert_eq!(report["options"]["left_source"]["table"], 1);
    let catalog = run(&[
        "table".as_ref(),
        html.as_os_str(),
        csv.as_os_str(),
        "--list-tables".as_ref(),
    ]);
    assert!(catalog.status.success());
    let info: serde_json::Value = serde_json::from_slice(&catalog.stdout).unwrap();
    assert_eq!(info["left_tables"].as_array().unwrap().len(), 2);
    assert_eq!(info["left_tables"][1]["caption"], "Data");
    assert!(info["right_tables"].as_array().unwrap().is_empty());
    let missing = run(&[
        "table".as_ref(),
        html.as_os_str(),
        csv.as_os_str(),
        "--left-html".as_ref(),
        "99".as_ref(),
    ]);
    assert_eq!(missing.status.code(), Some(2));
    let direct = run(&["table".as_ref(), html.as_os_str(), html.as_os_str()]);
    assert!(direct.status.success());
}
#[test]
fn utf16_html_and_literal_delimited_overrides_are_explicit() {
    let t = tempfile::tempdir().unwrap();
    let l = t.path().join("left.html");
    let r = t.path().join("right.csv");
    let text = "<table><tr><th>id<th>value<tr><td>a<td>1</table>";
    let bytes = [
        vec![255, 254],
        text.encode_utf16().flat_map(u16::to_le_bytes).collect(),
    ]
    .concat();
    fs::write(&l, bytes).unwrap();
    fs::write(&r, "id,value\na,1\n").unwrap();
    let result = run(&[
        "table".as_ref(),
        l.as_os_str(),
        r.as_os_str(),
        "--key-column".as_ref(),
        "0".as_ref(),
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    fs::write(&l, "id,value\na,1\n").unwrap();
    let literal = run(&[
        "table".as_ref(),
        l.as_os_str(),
        r.as_os_str(),
        "--delimited".as_ref(),
    ]);
    assert!(literal.status.success());
    let auto = run(&["table".as_ref(), l.as_os_str(), r.as_os_str()]);
    assert_eq!(auto.status.code(), Some(2));
}
