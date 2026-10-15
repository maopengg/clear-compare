use clear_contracts::CompareRequest;
use clear_core::{binary, compare};
use std::{fs, sync::atomic::AtomicBool};
#[test]
fn binary_page_handles_unequal_lengths_and_bounds() {
    let t = tempfile::tempdir().unwrap();
    let l = t.path().join("l");
    let r = t.path().join("r");
    fs::create_dir(&l).unwrap();
    fs::create_dir(&r).unwrap();
    let mut a = vec![0; 4100];
    a[4096] = 65;
    fs::write(l.join("a"), a).unwrap();
    fs::write(r.join("a"), [0, 1]).unwrap();
    let s = compare::compare(
        &CompareRequest {
            left: l.to_string_lossy().into(),
            right: r.to_string_lossy().into(),
            exclusions: String::new(),
            content: true,
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    let id = &s.report.rows[0].id;
    let p = binary::page(&s, id, "0").unwrap();
    assert_eq!(p.lines.len(), 256);
    assert_eq!(p.left_size, "4100");
    assert!(p.lines[0].changed);
    let p = binary::page(&s, id, "4096").unwrap();
    assert_eq!(p.lines.len(), 1);
    assert_eq!(p.lines[0].left_ascii, "A...");
    assert!(p.lines[0].right_hex.is_empty());
    assert!(binary::page(&s, id, "-1").is_err());
    assert!(binary::page(&s, id, "1").is_err());
    assert!(binary::page(&s, id, "invalid").is_err());
}

fn options(side: clear_contracts::Side) -> clear_contracts::BinarySearchOptions {
    clear_contracts::BinarySearchOptions {
        query: "61 61".into(),
        mode: clear_contracts::BinarySearchMode::Hex,
        offset: "0x0".into(),
        limit: 2,
        side,
    }
}
#[test]
fn search_uses_opaque_sides_and_rejects_changed_metadata() {
    use clear_contracts::Side;
    let l = tempfile::tempdir().unwrap();
    let r = tempfile::tempdir().unwrap();
    fs::write(l.path().join("a"), b"aaaa").unwrap();
    let s = compare::compare(
        &CompareRequest {
            left: l.path().display().to_string(),
            right: r.path().display().to_string(),
            exclusions: String::new(),
            content: true,
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    let id = &s.report.rows[0].id;
    let found = binary::search(&s, id, &options(Side::Left), &AtomicBool::new(false)).unwrap();
    assert_eq!(found.positions, ["0", "1"]);
    assert_eq!(found.next_offset.as_deref(), Some("2"));
    assert!(binary::search(&s, id, &options(Side::Right), &AtomicBool::new(false)).is_err());
    assert!(binary::search(&s, "invalid", &options(Side::Left), &AtomicBool::new(false)).is_err());
    let p = binary::page(&s, id, "0").unwrap();
    assert!(p.left_available);
    assert!(!p.right_available);
    assert_eq!(p.lines[0].left_changed, [0, 1, 2, 3]);
    assert!(p.lines[0].right_changed.is_empty());
    fs::write(l.path().join("a"), b"aaaaa").unwrap();
    assert!(binary::search(&s, id, &options(Side::Left), &AtomicBool::new(false)).is_err());
    assert!(binary::page(&s, id, "0").is_err());
}
#[test]
fn paired_bytes_distinguish_missing_bytes_from_zero_and_have_fresh_identity() {
    let t = tempfile::tempdir().unwrap();
    let a = t.path().join("a");
    let b = t.path().join("b");
    fs::write(&a, [0, 1, 0]).unwrap();
    fs::write(&b, [0, 2]).unwrap();
    let p = binary::page_files(&a, &b, "0x0").unwrap();
    assert_eq!(p.lines[0].left_changed, [1, 2]);
    assert_eq!(p.lines[0].right_changed, [1]);
    assert_ne!(p.id, binary::page_files(&a, &b, "0").unwrap().id);
    let p = binary::page_files(&a, &b, "0x1000").unwrap();
    assert!(p.lines.is_empty());
}
#[cfg(unix)]
#[test]
fn direct_binary_operations_do_not_follow_symbolic_links() {
    let t = tempfile::tempdir().unwrap();
    let a = t.path().join("a");
    let link = t.path().join("link");
    fs::write(&a, b"aa").unwrap();
    std::os::unix::fs::symlink(&a, &link).unwrap();
    assert!(
        binary::search_file(
            &link,
            &options(clear_contracts::Side::Left),
            &AtomicBool::new(false)
        )
        .is_err()
    );
    assert!(binary::page_files(&a, &link, "0").is_err());
}
