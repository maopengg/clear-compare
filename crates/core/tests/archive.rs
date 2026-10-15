use clear_contracts::{ArchiveRequest, Status, TextOptions};
use clear_core::archive;
use std::{fs, io::Write, path::Path, sync::atomic::AtomicBool};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};
fn zip(path: &Path, files: &[(&str, &str)]) {
    let mut writer = ZipWriter::new(fs::File::create(path).unwrap());
    for (name, text) in files {
        writer
            .start_file(
                *name,
                SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
            )
            .unwrap();
        writer.write_all(text.as_bytes()).unwrap();
    }
    writer.finish().unwrap();
}
#[test]
fn zip_and_directory_compare_verified_contents_and_implicit_directories() {
    let t = tempfile::tempdir().unwrap();
    let z = t.path().join("source.zip");
    zip(
        &z,
        &[
            ("sub/a.txt", "same"),
            ("different.txt", "zip"),
            ("README.md", "ignored"),
        ],
    );
    let dir = t.path().join("folder");
    fs::create_dir_all(dir.join("sub")).unwrap();
    fs::write(dir.join("sub/a.txt"), "same").unwrap();
    fs::write(dir.join("different.txt"), "dir").unwrap();
    fs::write(dir.join("README.md"), "other").unwrap();
    let request = ArchiveRequest {
        left: z.to_string_lossy().into(),
        right: dir.to_string_lossy().into(),
        exclusions: "-README.md".into(),
        content: true,
    };
    let snapshot = archive::compare(&request, &AtomicBool::new(false)).unwrap();
    assert_eq!(snapshot.report.summary.same, 1);
    assert_eq!(snapshot.report.summary.changed, 1);
    assert!(snapshot.report.rows.iter().all(|r| r.path != "README.md"));
    assert_eq!(
        snapshot
            .report
            .rows
            .iter()
            .find(|r| r.path == "sub")
            .unwrap()
            .status,
        Status::Same
    );
    let changed = snapshot
        .report
        .rows
        .iter()
        .find(|r| r.path == "different.txt")
        .unwrap();
    let diff = snapshot
        .text_diff(
            &changed.id,
            &TextOptions::default(),
            &AtomicBool::new(false),
        )
        .unwrap();
    assert!(
        diff.lines
            .iter()
            .any(|l| l.tag == "delete" && l.text == "zip")
    );
    let metadata = archive::compare(
        &ArchiveRequest {
            content: false,
            ..request
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(metadata.report.summary.same, 0);
    assert!(metadata.report.summary.unverified > 0);
}
#[test]
fn browsing_is_read_only_and_changed_archives_invalidate_preview() {
    let t = tempfile::tempdir().unwrap();
    let z = t.path().join("source.zip");
    zip(&z, &[("a", "original")]);
    let snapshot = archive::compare(
        &ArchiveRequest {
            left: z.to_string_lossy().into(),
            right: String::new(),
            exclusions: String::new(),
            content: true,
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(snapshot.report.summary.left_only, 1);
    assert_eq!(snapshot.page(0, "A", true).unwrap().total, 1);
    let id = snapshot.report.rows[0].id.clone();
    zip(&z, &[("a", "modified")]);
    assert!(
        snapshot
            .text_diff(&id, &TextOptions::default(), &AtomicBool::new(false))
            .is_err()
    );
    assert_eq!(fs::read_dir(t.path()).unwrap().count(), 1);
}
#[test]
fn traversal_and_duplicate_names_are_rejected_before_member_reads() {
    let t = tempfile::tempdir().unwrap();
    let z = t.path().join("source.zip");
    zip(&z, &[("xx/escape", "text")]);
    let mut bytes = fs::read(&z).unwrap();
    for index in 0..bytes.len() - 9 {
        if &bytes[index..index + 9] == b"xx/escape" {
            bytes[index] = b'.';
            bytes[index + 1] = b'.';
        }
    }
    fs::write(&z, &bytes).unwrap();
    let request = ArchiveRequest {
        left: z.to_string_lossy().into(),
        right: String::new(),
        exclusions: String::new(),
        content: true,
    };
    assert!(archive::compare(&request, &AtomicBool::new(false)).is_err());
    zip(&z, &[("one", "1"), ("two", "2")]);
    let mut bytes = fs::read(&z).unwrap();
    for index in 0..bytes.len() - 3 {
        if &bytes[index..index + 3] == b"two" {
            bytes[index..index + 3].copy_from_slice(b"one");
        }
    }
    fs::write(&z, bytes).unwrap();
    assert!(archive::compare(&request, &AtomicBool::new(false)).is_err());
}
#[test]
fn expansion_budget_and_cancellation_never_claim_content_equality() {
    let t = tempfile::tempdir().unwrap();
    let z = t.path().join("source.zip");
    zip(&z, &[("large", "0".repeat(2 * 1024 * 1024).as_str())]);
    let request = ArchiveRequest {
        left: z.to_string_lossy().into(),
        right: z.to_string_lossy().into(),
        exclusions: String::new(),
        content: true,
    };
    let snapshot = archive::compare(&request, &AtomicBool::new(false)).unwrap();
    assert_eq!(snapshot.report.summary.same, 0);
    assert!(snapshot.report.summary.errors > 0);
    assert!(archive::compare(&request, &AtomicBool::new(true)).is_err());
}

#[test]
fn corrupted_member_crc_is_an_error_instead_of_a_matching_file() {
    let t = tempfile::tempdir().unwrap();
    let z = t.path().join("source.zip");
    zip(&z, &[("a.txt", "content")]);
    let mut bytes = fs::read(&z).unwrap();
    let start = bytes.windows(4).position(|w| w == b"PK\x01\x02").unwrap();
    bytes[start + 16] ^= 1;
    fs::write(&z, bytes).unwrap();
    let snapshot = archive::compare(
        &ArchiveRequest {
            left: z.to_string_lossy().into(),
            right: z.to_string_lossy().into(),
            exclusions: String::new(),
            content: true,
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(snapshot.report.summary.errors > 0);
    assert_eq!(snapshot.report.summary.same, 0);
}
