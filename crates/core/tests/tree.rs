use clear_contracts::{CompareRequest, FolderQuery, FolderSort, Status};
use clear_core::{compare, tree};
use std::{fs, path::Path, sync::atomic::AtomicBool};
fn scan(root: &Path, content: bool) -> compare::Snapshot {
    compare::compare(
        &CompareRequest {
            left: root.join("l").to_string_lossy().into(),
            right: root.join("r").to_string_lossy().into(),
            exclusions: String::new(),
            content,
        },
        &AtomicBool::new(false),
    )
    .unwrap()
}
fn query() -> FolderQuery {
    FolderQuery {
        parent_id: None,
        recursive: true,
        search: String::new(),
        differences_only: false,
        sort: FolderSort::Path,
        descending: false,
    }
}
#[test]
fn global_filter_finds_matches_beyond_the_first_page() {
    let t = tempfile::tempdir().unwrap();
    for name in ["l", "r"] {
        fs::create_dir(t.path().join(name)).unwrap();
    }
    for i in 0..620 {
        let name = format!("file-{i:04}.txt");
        fs::write(t.path().join("l").join(&name), "same").unwrap();
        fs::write(t.path().join("r").join(name), "same").unwrap();
    }
    fs::write(t.path().join("l/zz-needle.txt"), "left").unwrap();
    fs::write(t.path().join("r/zz-needle.txt"), "right").unwrap();
    let s = scan(t.path(), true);
    let first = tree::query(&s, &query(), 0).unwrap();
    assert_eq!(first.page.rows.len(), 500);
    assert_eq!(first.page.total, 621);
    let filtered = tree::query(
        &s,
        &FolderQuery {
            search: "NEEDLE".into(),
            differences_only: true,
            ..query()
        },
        0,
    )
    .unwrap();
    assert_eq!(filtered.page.total, 1);
    assert_eq!(filtered.page.rows[0].path, "zz-needle.txt");
    let last = tree::query(&s, &query(), 500).unwrap();
    assert_eq!(last.page.rows.len(), 121);
}
#[test]
fn directory_status_includes_descendants_and_breadcrumb_scope_is_native() {
    let t = tempfile::tempdir().unwrap();
    for name in ["l/sub/nested", "r/sub/nested", "l/unchanged", "r/unchanged"] {
        fs::create_dir_all(t.path().join(name)).unwrap();
    }
    fs::write(t.path().join("l/sub/nested/a"), "left").unwrap();
    fs::write(t.path().join("r/sub/nested/a"), "right").unwrap();
    fs::create_dir(t.path().join("l/sub/new-empty")).unwrap();
    let s = scan(t.path(), true);
    let directory = s.report.rows.iter().find(|r| r.path == "sub").unwrap();
    assert_eq!(directory.status, Status::Changed);
    assert_eq!(directory.left.as_ref().unwrap().bytes, "4");
    assert_eq!(directory.right.as_ref().unwrap().bytes, "5");
    assert_eq!(directory.directory_summary.as_ref().unwrap().changed, 1);
    let nested = s
        .report
        .rows
        .iter()
        .find(|r| r.path == "sub/nested")
        .unwrap();
    let result = tree::query(
        &s,
        &FolderQuery {
            parent_id: Some(nested.id.clone()),
            recursive: false,
            ..query()
        },
        0,
    )
    .unwrap();
    assert_eq!(result.page.total, 1);
    assert_eq!(
        result
            .breadcrumbs
            .iter()
            .map(|b| b.label.as_str())
            .collect::<Vec<_>>(),
        vec!["sub", "nested"]
    );
    let root = tree::query(
        &s,
        &FolderQuery {
            recursive: false,
            differences_only: true,
            ..query()
        },
        0,
    )
    .unwrap();
    assert_eq!(root.page.total, 1);
    assert_eq!(root.page.rows[0].path, "sub");
    assert!(
        tree::query(
            &s,
            &FolderQuery {
                parent_id: Some(result.page.rows[0].id.clone()),
                ..query()
            },
            0
        )
        .is_err()
    );
}
#[test]
fn unverified_children_prevent_a_verified_directory_status_and_size_sort_is_numeric() {
    let t = tempfile::tempdir().unwrap();
    for name in ["l/sub", "r/sub"] {
        fs::create_dir_all(t.path().join(name)).unwrap();
    }
    for side in ["l", "r"] {
        fs::write(t.path().join(side).join("sub/a"), "same").unwrap();
    }
    let s = scan(t.path(), false);
    assert_ne!(
        s.report
            .rows
            .iter()
            .find(|r| r.path == "sub")
            .unwrap()
            .status,
        Status::Same
    );
    fs::write(t.path().join("l/large"), "0123456789").unwrap();
    fs::write(t.path().join("l/small"), "12").unwrap();
    let s = scan(t.path(), true);
    let result = tree::query(
        &s,
        &FolderQuery {
            recursive: false,
            sort: FolderSort::LeftSize,
            descending: true,
            ..query()
        },
        0,
    )
    .unwrap();
    assert_eq!(
        result
            .page
            .rows
            .iter()
            .map(|r| r.path.as_str())
            .collect::<Vec<_>>(),
        vec!["sub", "large", "small"]
    );
}
