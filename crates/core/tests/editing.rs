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
