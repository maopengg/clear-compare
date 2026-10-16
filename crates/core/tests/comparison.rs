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
    assert!(text::read_text(&path).is_err());
    fs::write(&path, [255, 254, 128]).unwrap();
    assert!(text::read_text(&path).is_err());
    fs::write(
        &path,
        vec![b'a'; clear_core::editing::MAX_DOCUMENT_BYTES + 1],
    )
    .unwrap();
    assert!(text::read_text(&path).is_err());
    fs::write(&path, "\u{feff}中文\r\n").unwrap();
    assert_eq!(text::read_text(&path).unwrap(), "中文\r\n");
}

#[test]
fn modification_times_do_not_affect_file_or_directory_status() {
    use std::time::{Duration, UNIX_EPOCH};
    let left = tempfile::tempdir().unwrap();
    let right = tempfile::tempdir().unwrap();
    for (root, seconds) in [(left.path(), 1000), (right.path(), 9000)] {
        fs::create_dir(root.join("images")).unwrap();
        for (name, bytes) in [
            ("same.jpg", b"same".as_slice()),
            ("different.jpg", b"left".as_slice()),
        ] {
            let path = root.join("images").join(name);
            let bytes = if root == right.path() && name == "different.jpg" {
                b"rght".as_slice()
            } else {
                bytes
            };
            fs::write(&path, bytes).unwrap();
            fs::File::open(path)
                .unwrap()
                .set_times(
                    fs::FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(seconds)),
                )
                .unwrap();
        }
    }
    fs::write(left.path().join("size.txt"), "a").unwrap();
    fs::write(right.path().join("size.txt"), "longer").unwrap();
    let mut request = CompareRequest {
        left: left.path().to_string_lossy().into_owned(),
        right: right.path().to_string_lossy().into_owned(),
        exclusions: String::new(),
        content: false,
    };
    let quick = compare::compare(&request, &AtomicBool::new(false)).unwrap();
    for path in ["images", "images/same.jpg", "images/different.jpg"] {
        let row = quick.report.rows.iter().find(|r| r.path == path).unwrap();
        assert_eq!(row.status, Status::MetadataMatch, "{path}");
    }
    assert_eq!(quick.report.summary.changed, 1);
    assert_eq!(quick.report.summary.unverified, 2);
    request.content = true;
    let verified = compare::compare(&request, &AtomicBool::new(false)).unwrap();
    let same = verified
        .report
        .rows
        .iter()
        .find(|r| r.path == "images/same.jpg")
        .unwrap();
    assert_eq!(same.status, Status::Same);
    let changed = verified
        .report
        .rows
        .iter()
        .find(|r| r.path == "images/different.jpg")
        .unwrap();
    assert_eq!(changed.status, Status::Changed);
    assert_eq!(verified.report.summary.same, 1);
    assert_eq!(verified.report.summary.changed, 2);
}
