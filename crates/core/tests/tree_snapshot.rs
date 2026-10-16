use clear_contracts::{ArchiveRequest, Status, TreeSnapshotRequest};
use clear_core::tree_snapshot;
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
fn cancel() -> AtomicBool {
    AtomicBool::new(false)
}
fn capture(root: &Path, file: &Path) -> clear_contracts::TreeSnapshotInfo {
    tree_snapshot::capture(
        &TreeSnapshotRequest {
            root: root.display().to_string(),
            destination: file.display().to_string(),
            exclusions: "-excluded/".into(),
        },
        &cancel(),
    )
    .unwrap()
}
fn request(left: &Path, right: Option<&Path>) -> ArchiveRequest {
    ArchiveRequest {
        left: left.display().to_string(),
        right: right.map(|p| p.display().to_string()).unwrap_or_default(),
        exclusions: String::new(),
        content: false,
    }
}
fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let t = tempfile::tempdir().unwrap();
    let root = t.path().join("root");
    fs::create_dir(&root).unwrap();
    let file = t.path().join("tree.cctree");
    (t, root, file)
}
#[test]
fn snapshot_survives_source_removal_without_capturing_contents_or_claiming_equality() {
    let (_t, root, file) = fixture();
    fs::create_dir(root.join("empty")).unwrap();
    fs::create_dir(root.join("sub")).unwrap();
    fs::create_dir(root.join("excluded")).unwrap();
    fs::write(root.join("sub/hello.txt"), "SECRET_FILE_BODY_ABCDE").unwrap();
    fs::write(root.join("excluded/hidden"), "excluded").unwrap();
    let info = capture(&root, &file);
    assert_eq!(info.entries, 3);
    let stored = fs::read_to_string(&file).unwrap();
    assert!(!stored.contains("SECRET_FILE_BODY_ABCDE"));
    assert!(!stored.contains(root.to_str().unwrap()));
    assert!(!stored.contains("hidden"));
    let comparison = tree_snapshot::compare(&request(&file, Some(&root)), &cancel()).unwrap();
    let text = comparison
        .report
        .rows
        .iter()
        .find(|r| r.path == "sub/hello.txt")
        .unwrap();
    assert_eq!(text.status, Status::MetadataMatch);
    assert!(text.left.as_ref().unwrap().digest.is_none());
    fs::remove_dir_all(&root).unwrap();
    let copy = file.with_extension("other.cctree");
    fs::copy(&file, &copy).unwrap();
    let result = tree_snapshot::compare(&request(&file, Some(&copy)), &cancel()).unwrap();
    assert_eq!(result.report.summary.unverified, 1);
    assert_eq!(result.report.summary.same, 0);
    assert_eq!(result.page(0, "hello", false).unwrap().total, 1);
    assert_eq!(result.page(500, "", false).unwrap().rows.len(), 0);
}
#[test]
fn time_only_changes_remain_unverified_and_missing_paths_are_explicit() {
    let (_t, root, file) = fixture();
    fs::write(root.join("a"), "one").unwrap();
    capture(&root, &file);
    let mut data: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    data["entries"][0]["modified_ms"] = json!("1");
    let other = file.with_extension("other");
    fs::write(&other, serde_json::to_vec(&data).unwrap()).unwrap();
    let r = tree_snapshot::compare(&request(&file, Some(&other)), &cancel()).unwrap();
    assert_eq!(r.report.summary.changed, 0);
    assert_eq!(r.report.summary.unverified, 1);
    fs::remove_file(root.join("a")).unwrap();
    fs::write(root.join("new"), "longer").unwrap();
    let r = tree_snapshot::compare(&request(&file, Some(&root)), &cancel()).unwrap();
    assert_eq!(r.report.summary.left_only, 1);
    assert_eq!(r.report.summary.right_only, 1);
    let mut q = request(&file, None);
    q.content = true;
    assert!(tree_snapshot::compare(&q, &cancel()).is_err());
}
#[test]
fn exports_never_overwrite_or_pollute_sources_and_cancellation_leaves_no_output() {
    let (t, root, file) = fixture();
    fs::write(&file, b"preserve").unwrap();
    let req = TreeSnapshotRequest {
        root: root.display().to_string(),
        destination: file.display().to_string(),
        exclusions: String::new(),
    };
    assert!(tree_snapshot::capture(&req, &cancel()).is_err());
    assert_eq!(fs::read(&file).unwrap(), b"preserve");
    let mut q = req.clone();
    q.destination = root.join("inside.cctree").display().to_string();
    assert!(tree_snapshot::capture(&q, &cancel()).is_err());
    assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    fs::remove_file(&file).unwrap();
    assert!(tree_snapshot::capture(&req, &AtomicBool::new(true)).is_err());
    assert_eq!(fs::read_dir(t.path()).unwrap().count(), 1);
}
#[test]
fn malicious_inventories_fail_before_comparison_even_when_excluded() {
    let (_t, root, file) = fixture();
    fs::write(root.join("a"), "a").unwrap();
    capture(&root, &file);
    let good: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    let mut cases = Vec::new();
    let mut bad = good.clone();
    bad["version"] = json!(999);
    cases.push(bad);
    let mut bad = good.clone();
    let duplicate = bad["entries"][0].clone();
    bad["entries"].as_array_mut().unwrap().push(duplicate);
    cases.push(bad);
    for parts in [
        vec!["..", "escape"],
        vec!["/absolute"],
        vec!["a\\b"],
        vec!["C:drive"],
        vec!["missing", "child"],
    ] {
        let mut bad = good.clone();
        bad["entries"][0]["path"] = json!({"encoding":"portable","value":parts});
        cases.push(bad);
    }
    let mut bad = good.clone();
    bad["entries"][0]["bytes"] = json!("-1");
    cases.push(bad);
    let mut bad = good.clone();
    bad["entries"][0]["digest"] = json!("forged");
    cases.push(bad);
    let mut bad = good.clone();
    bad["entries"][0]["path"] =
        json!({"encoding":"native","value":{"encoding":"unix","value":[0]}});
    cases.push(bad);
    for bad in cases {
        fs::write(&file, serde_json::to_vec(&bad).unwrap()).unwrap();
        let mut q = request(&file, None);
        q.exclusions = "-*".into();
        assert!(
            tree_snapshot::compare(&q, &cancel()).is_err(),
            "accepted {bad}"
        );
    }
    fs::write(&file, b"{broken").unwrap();
    assert!(tree_snapshot::compare(&request(&file, None), &cancel()).is_err());
}
#[cfg(unix)]
#[test]
fn native_names_are_lossless_and_symbolic_links_are_never_followed() {
    use std::os::unix::fs::symlink;
    let (t, root, file) = fixture();
    for name in ["a", "b"] {
        fs::write(root.join(name), b"x").unwrap();
    }
    let outside = t.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("hidden"), "body").unwrap();
    symlink(&outside, root.join("link")).unwrap();
    capture(&root, &file);
    let r = tree_snapshot::compare(&request(&file, None), &cancel()).unwrap();
    assert_eq!(r.report.rows.len(), 3);
    assert!(!r.report.rows.iter().any(|r| r.path.ends_with("hidden")));
    let r = tree_snapshot::compare(&request(&file, Some(&root)), &cancel()).unwrap();
    assert_eq!(r.report.summary.unverified, 3);
    assert_eq!(r.report.summary.same, 0);
}

#[cfg(unix)]
#[test]
fn non_unicode_inventory_names_remain_distinct_despite_equal_display_labels() {
    let (_t, root, file) = fixture();
    for name in ["a", "b"] {
        fs::write(root.join(name), b"x").unwrap();
    }
    capture(&root, &file);
    let mut data: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    for (i, byte) in [255, 254].into_iter().enumerate() {
        data["entries"][i]["path"] =
            json!({"encoding":"native","value":{"encoding":"unix","value":[97,byte]}});
    }
    fs::write(&file, serde_json::to_vec(&data).unwrap()).unwrap();
    let r = tree_snapshot::compare(&request(&file, Some(&file)), &cancel()).unwrap();
    assert_eq!(r.report.rows.len(), 2);
    assert_eq!(r.report.summary.unverified, 2);
    assert_ne!(r.report.rows[0].id, r.report.rows[1].id);
}

#[test]
fn large_snapshot_inputs_and_incompatible_native_encodings_are_rejected() {
    let (_t, root, file) = fixture();
    fs::write(root.join("a"), b"x").unwrap();
    capture(&root, &file);
    let mut data: Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    #[cfg(unix)]
    {
        data["entries"][0]["path"] =
            json!({"encoding":"native","value":{"encoding":"windows","value":[97]}});
    }
    #[cfg(windows)]
    {
        data["entries"][0]["path"] =
            json!({"encoding":"native","value":{"encoding":"unix","value":[97]}});
    }
    fs::write(&file, serde_json::to_vec(&data).unwrap()).unwrap();
    assert!(tree_snapshot::compare(&request(&file, None), &cancel()).is_err());
    fs::File::create(&file)
        .unwrap()
        .set_len(64 * 1024 * 1024 + 1)
        .unwrap();
    assert!(tree_snapshot::compare(&request(&file, None), &cancel()).is_err());
}
