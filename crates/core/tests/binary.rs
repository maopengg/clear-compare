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
