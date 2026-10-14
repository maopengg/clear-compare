use clear_contracts::{CompareRequest, Status};
use clear_core::{compare, text};
use std::{fs, sync::atomic::AtomicBool};

#[test]
fn matching_file_metadata_is_not_claimed_as_verified_content() {
    let left = tempfile::tempdir().unwrap();
    let right = tempfile::tempdir().unwrap();
    fs::write(left.path().join("a"), "same").unwrap();
    fs::copy(left.path().join("a"), right.path().join("a")).unwrap();
    let mut request = CompareRequest {
        left: left.path().to_string_lossy().into_owned(),
        right: right.path().to_string_lossy().into_owned(),
        exclusions: String::new(),
        content: false,
    };
    let s = compare::compare(&request, &AtomicBool::new(false)).unwrap();
    assert_ne!(s.report.rows[0].status, Status::Same);
    request.content = true;
    let s = compare::compare(&request, &AtomicBool::new(false)).unwrap();
    assert_eq!(s.report.rows[0].status, Status::Same);
    assert_eq!(s.report.summary.same, 1);
}

#[test]
fn changed_same_length_files_type_conflicts_and_missing_files_are_distinct() {
    let left = tempfile::tempdir().unwrap();
    let right = tempfile::tempdir().unwrap();
    fs::write(left.path().join("a"), "one").unwrap();
    fs::write(right.path().join("a"), "two").unwrap();
    fs::write(left.path().join("b"), "file").unwrap();
    fs::create_dir(right.path().join("b")).unwrap();
    fs::write(right.path().join("c"), "target only").unwrap();
    let request = CompareRequest {
        left: left.path().to_string_lossy().into_owned(),
        right: right.path().to_string_lossy().into_owned(),
        exclusions: String::new(),
        content: true,
    };
    let s = compare::compare(&request, &AtomicBool::new(false)).unwrap();
    assert_eq!(s.report.rows[0].status, Status::Changed);
    assert_eq!(s.report.rows[1].status, Status::TypeConflict);
    assert_eq!(s.report.rows[2].status, Status::RightOnly);
    assert_eq!(s.report.summary.errors, 1);
    let d = text::preview(&s, &s.report.rows[0].id).unwrap();
    assert!(d.lines.iter().any(|l| l.tag == "delete" && l.text == "one"));
    assert!(d.lines.iter().any(|l| l.tag == "insert" && l.text == "two"));
}

#[test]
fn text_preview_rejects_binary_invalid_encoding_and_large_inputs() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("a");
    fs::write(&path, [0, 1, 2]).unwrap();
    assert!(text::read_utf8(&path).is_err());
    fs::write(&path, [255, 254, 128]).unwrap();
    assert!(text::read_utf8(&path).is_err());
    fs::write(&path, vec![b'a'; 2 * 1024 * 1024 + 1]).unwrap();
    assert!(text::read_utf8(&path).is_err());
    fs::write(&path, "\u{feff}中文\r\n").unwrap();
    assert_eq!(text::read_utf8(&path).unwrap(), "中文\r\n");
}
