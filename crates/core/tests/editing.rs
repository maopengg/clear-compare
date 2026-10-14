use clear_contracts::{CompareRequest, Encoding, Side};
use clear_core::{compare, editing};
use std::{fs, sync::atomic::AtomicBool};

#[test]
fn utf16_edit_retains_encoding_endings_and_backup_and_rejects_external_changes() {
    let root = tempfile::tempdir().unwrap();
    for d in ["l", "r", "b"] {
        fs::create_dir(root.path().join(d)).unwrap();
    }
    let mut bytes = vec![0xff, 0xfe];
    for c in "中文\r\nold\r\n".encode_utf16() {
        bytes.extend_from_slice(&c.to_le_bytes());
    }
    fs::write(root.path().join("l/a.txt"), &bytes).unwrap();
    let request = CompareRequest {
        left: root.path().join("l").to_string_lossy().into_owned(),
        right: root.path().join("r").to_string_lossy().into_owned(),
        exclusions: String::new(),
        content: true,
    };
    let s = compare::compare(&request, &AtomicBool::new(false)).unwrap();
    let d = editing::open(&s, &s.report.rows[0].id, Side::Left).unwrap();
    assert_eq!(d.view.encoding, Encoding::Utf16Le);
    let (updated, result) = editing::save(&d, "中文\nnew\n", &root.path().join("b")).unwrap();
    assert_eq!(updated.view.text, "中文\r\nnew\r\n");
    assert_eq!(fs::read(&result.backup).unwrap(), bytes);
    let manifest = std::path::Path::new(&result.backup)
        .parent()
        .unwrap()
        .join("manifest.json");
    let metadata: serde_json::Value = serde_json::from_slice(&fs::read(manifest).unwrap()).unwrap();
    assert_eq!(metadata["state"], "completed");
    assert_eq!(metadata["saved_digest"], updated.view.digest);
    fs::write(root.path().join("l/a.txt"), "external").unwrap();
    assert!(editing::save(&updated, "overwrite", &root.path().join("b")).is_err());
    assert_eq!(
        fs::read_to_string(root.path().join("l/a.txt")).unwrap(),
        "external"
    );
}

#[test]
fn malformed_and_binary_text_are_rejected() {
    assert!(editing::decode(&[0xff, 0xfe, 0x01]).is_err());
    assert!(editing::decode(&[0, 1, 2]).is_err());
    assert!(editing::decode(&[255, 128]).is_err());
    assert_eq!(editing::decode(b"\xef\xbb\xbfhello").unwrap().0, "hello");
}

#[test]
fn explicit_format_conversion_and_reviewed_restore_preserve_original_bytes() {
    use clear_contracts::{LineEnding, SaveOptions};
    let t = tempfile::tempdir().unwrap();
    for name in ["l", "r", "b"] {
        fs::create_dir(t.path().join(name)).unwrap();
    }
    let original = b"mixed\r\nlines\nend\r";
    fs::write(t.path().join("l/a"), original).unwrap();
    let req = CompareRequest {
        left: t.path().join("l").to_string_lossy().into(),
        right: t.path().join("r").to_string_lossy().into(),
        exclusions: String::new(),
        content: true,
    };
    let s = compare::compare(&req, &AtomicBool::new(false)).unwrap();
    let d = editing::open(&s, &s.report.rows[0].id, Side::Left).unwrap();
    assert!(editing::save(&d, "new", &t.path().join("b")).is_err());
    let options = SaveOptions {
        encoding: Some(Encoding::Utf16Be),
        line_ending: Some(LineEnding::CrLf),
        bom: None,
    };
    let (saved, result) =
        editing::save_with_options(&d, "中文\nnew\n", &t.path().join("b"), &options).unwrap();
    assert_eq!(saved.view.encoding, Encoding::Utf16Be);
    assert!(saved.view.bom);
    assert_eq!(saved.view.text, "中文\r\nnew\r\n");
    let plan = editing::preview_restore(&saved, std::path::Path::new(&result.backup)).unwrap();
    let before = fs::read(t.path().join("l/a")).unwrap();
    let (restored, result) = editing::apply_restore(&plan, &t.path().join("b")).unwrap();
    assert_eq!(fs::read(t.path().join("l/a")).unwrap(), original);
    assert_eq!(fs::read(&result.backup).unwrap(), before);
    assert_eq!(restored.view.line_ending, LineEnding::Mixed);
    let reopened = editing::open_file(&t.path().join("l/a")).unwrap();
    let redo = editing::preview_restore(&reopened, std::path::Path::new(&result.backup)).unwrap();
    fs::write(t.path().join("l/a"), "external").unwrap();
    assert!(editing::apply_restore(&redo, &t.path().join("b")).is_err());
    assert_eq!(
        fs::read_to_string(t.path().join("l/a")).unwrap(),
        "external"
    );
}

#[test]
fn corrupted_backup_and_bomless_utf16_requests_are_rejected() {
    use clear_contracts::SaveOptions;
    let t = tempfile::tempdir().unwrap();
    for name in ["l", "r", "b"] {
        fs::create_dir(t.path().join(name)).unwrap();
    }
    fs::write(t.path().join("l/a"), "original").unwrap();
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
    let d = editing::open(&s, &s.report.rows[0].id, Side::Left).unwrap();
    assert!(
        editing::save_with_options(
            &d,
            "new",
            &t.path().join("b"),
            &SaveOptions {
                encoding: Some(Encoding::Utf16Le),
                bom: Some(false),
                ..Default::default()
            }
        )
        .is_err()
    );
    let (saved, result) = editing::save(&d, "new", &t.path().join("b")).unwrap();
    fs::write(&result.backup, "corrupt").unwrap();
    assert!(editing::preview_restore(&saved, std::path::Path::new(&result.backup)).is_err());
}
