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

#[test]
fn generated_conflicts_block_save_and_history_cannot_reintroduce_them_after_save() {
    use clear_contracts::{
        SaveOptions,
        edit_history::{History, conflict_tag},
    };
    let t = tempfile::tempdir().unwrap();
    for name in ["files", "backups"] {
        fs::create_dir(t.path().join(name)).unwrap();
    }
    let path = t.path().join("files/a");
    fs::write(&path, "base\n").unwrap();
    let document = editing::open_file(&path).unwrap();
    let result = clear_core::merge::three_way("base\n", "left\n", "right\n").unwrap();
    let token = result.conflict_token.unwrap();
    let tokens = vec![token.clone()];
    let mut history = History::default();
    history.reset("base\n");
    history.record(&result.text).unwrap();
    assert!(
        editing::save_merge_aware(
            &document,
            history.current(),
            &t.path().join("backups"),
            &SaveOptions::default(),
            &tokens
        )
        .is_err()
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "base\n");
    assert!(
        fs::read_dir(t.path().join("backups"))
            .unwrap()
            .next()
            .is_none()
    );
    let stripped = result.text.replace(&conflict_tag(&token), "");
    assert!(
        editing::save_merge_aware(
            &document,
            &stripped,
            &t.path().join("backups"),
            &SaveOptions::default(),
            &tokens
        )
        .is_err()
    );
    history.record("resolved\n").unwrap();
    let (saved, _) = editing::save_merge_aware(
        &document,
        history.current(),
        &t.path().join("backups"),
        &SaveOptions::default(),
        &tokens,
    )
    .unwrap();
    history.undo();
    assert!(
        editing::save_merge_aware(
            &saved,
            history.current(),
            &t.path().join("backups"),
            &SaveOptions::default(),
            &tokens
        )
        .is_err()
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "resolved\n");
    assert!(
        editing::save_merge_aware(
            &saved,
            "heading\n=======\n",
            &t.path().join("backups"),
            &SaveOptions::default(),
            &tokens
        )
        .is_ok()
    );
}

#[test]
fn decoded_utf16_expansion_can_be_saved_within_the_encoded_file_limit() {
    let t = tempfile::tempdir().unwrap();
    for name in ["files", "backups"] {
        fs::create_dir(t.path().join(name)).unwrap();
    }
    let text = "中".repeat(1_500_000);
    let mut bytes = vec![255, 254];
    for unit in text.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    let path = t.path().join("files/a");
    fs::write(&path, &bytes).unwrap();
    let doc = editing::open_file(&path).unwrap();
    assert!(doc.view.text.len() > editing::MAX_DOCUMENT_BYTES);
    let (_, saved) = editing::save(&doc, &text, &t.path().join("backups")).unwrap();
    assert_eq!(fs::read(&path).unwrap(), bytes);
    assert_eq!(fs::read(saved.backup).unwrap(), bytes);
}

#[test]
fn selected_text_hunk_saves_only_that_block_with_original_target_bytes_backed_up() {
    use clear_contracts::text_merge;
    use std::collections::BTreeSet;
    let root = tempfile::tempdir().unwrap();
    for d in ["left", "right", "backup"] {
        fs::create_dir(root.path().join(d)).unwrap();
    }
    fs::write(
        root.path().join("left/sample.txt"),
        "head\r\n左🙂\r\nmiddle\r\nleftB\r\n",
    )
    .unwrap();
    let original = b"head\r\nrightA\r\nmiddle\r\nrightB\r\n";
    fs::write(root.path().join("right/sample.txt"), original).unwrap();
    let snapshot = compare::compare(
        &CompareRequest {
            left: root.path().join("left").to_string_lossy().into_owned(),
            right: root.path().join("right").to_string_lossy().into_owned(),
            content: true,
            exclusions: String::new(),
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    let id = &snapshot.report.rows[0].id;
    let diff = clear_core::text::preview(&snapshot, id).unwrap();
    let document = editing::open(&snapshot, id, Side::Right).unwrap();
    let preview = text_merge::preview(
        &diff,
        &BTreeSet::from([0]),
        &Side::Right,
        Some(&document.view.line_ending),
    )
    .unwrap();
    let (_, result) = editing::save(
        &document,
        &text_merge::text(&preview.diff, &Side::Right),
        &root.path().join("backup"),
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(root.path().join("right/sample.txt")).unwrap(),
        "head\r\n左🙂\r\nmiddle\r\nrightB\r\n"
    );
    assert_eq!(fs::read(result.backup).unwrap(), original);
    assert_eq!(
        fs::read_to_string(root.path().join("left/sample.txt")).unwrap(),
        "head\r\n左🙂\r\nmiddle\r\nleftB\r\n"
    );
}
