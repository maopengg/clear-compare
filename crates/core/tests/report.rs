use clear_contracts::*;
use clear_core::{compare, report};
use std::{fs, path::Path, sync::atomic::AtomicBool};
fn snapshot(root: &Path) -> compare::Snapshot {
    let left = root.join("left");
    let right = root.join("right");
    fs::create_dir(&left).unwrap();
    fs::create_dir(&right).unwrap();
    fs::write(left.join("a"), "OLD_SECRET_BODY").unwrap();
    fs::write(right.join("a"), "different").unwrap();
    fs::write(left.join("same"), "same").unwrap();
    fs::write(right.join("same"), "same").unwrap();
    compare::compare(
        &CompareRequest {
            left: left.display().to_string(),
            right: right.display().to_string(),
            exclusions: "-README.md".into(),
            content: true,
        },
        &AtomicBool::new(false),
    )
    .unwrap()
}
fn request(report: &Comparison, path: &Path, format: ReportFormat) -> ReportRequest {
    ReportRequest {
        source: ReportSource::Local,
        session: report.session.clone(),
        format,
        destination: path.display().to_string(),
        english: true,
        differences_only: false,
    }
}
#[test]
fn json_exports_full_frozen_results_with_criteria_and_filtered_uncertainty() {
    let t = tempfile::tempdir().unwrap();
    let s = snapshot(t.path());
    let out = t.path().join("report.json");
    fs::write(s.left.join("a"), "new current content").unwrap();
    let r = report::export(
        &s.report,
        &request(&s.report, &out, ReportFormat::Json),
        &[s.left.clone(), s.right.clone()],
        &AtomicBool::new(false),
    )
    .unwrap();
    let bytes = fs::read(&out).unwrap();
    assert_eq!(r.bytes, bytes.len().to_string());
    assert_eq!(r.rows, 2);
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(json["schema"], "clear-compare-report");
    assert_eq!(json["version"], 1);
    assert_eq!(json["policy"]["exclusions"], "-README.md");
    assert_eq!(json["policy"]["verification"], "sha256");
    assert_eq!(json["summary"]["same"], 1);
    assert!(!String::from_utf8_lossy(&bytes).contains("OLD_SECRET_BODY"));
    assert!(!String::from_utf8_lossy(&bytes).contains("new current content"));
    let mut report = s.report;
    report.rows[0].status = Status::MetadataMatch;
    report.rows.push(Row {
        id: "error".into(),
        path: "unreadable".into(),
        left: None,
        right: None,
        status: Status::Error,
        directory_summary: None,
    });
    let q = request(&report, &t.path().join("filtered.json"), ReportFormat::Json);
    let q = ReportRequest {
        differences_only: true,
        ..q
    };
    let result = report::export(&report, &q, &[], &AtomicBool::new(false)).unwrap();
    assert_eq!(result.rows, 2);
    let json: serde_json::Value =
        serde_json::from_slice(&fs::read(&q.destination).unwrap()).unwrap();
    assert!(json["filtered"].as_bool().unwrap());
    assert_eq!(json["summary"]["same"], 1);
    assert_eq!(json["rows"][0]["status"], "metadata_match");
    assert_eq!(json["rows"][1]["status"], "error");
}
#[test]
fn html_escapes_all_dynamic_fields_and_is_self_contained() {
    let t = tempfile::tempdir().unwrap();
    let mut s = snapshot(t.path());
    let attack = "</style><script>alert('x' & \"y\")</script><img src=x onerror=evil()>";
    s.report.left = attack.into();
    s.report.right = attack.into();
    s.report.session = attack.into();
    s.report.warnings = vec![attack.into()];
    s.report.policy.as_mut().unwrap().exclusions = attack.into();
    s.report.rows[0].path = attack.into();
    let e = s.report.rows[0].left.as_mut().unwrap();
    e.error = Some(attack.into());
    e.digest = Some(attack.into());
    e.modified_ms = Some(attack.into());
    let q = request(&s.report, &t.path().join("report.html"), ReportFormat::Html);
    let r = report::export(&s.report, &q, &[], &AtomicBool::new(false)).unwrap();
    let html = fs::read_to_string(&q.destination).unwrap();
    assert_eq!(r.rows, 2);
    assert!(html.contains("Content-Security-Policy"));
    assert!(html.contains("default-src 'none'"));
    assert!(!html.contains("<script"));
    assert!(!html.contains("<img"));
    assert!(!html.contains("<link"));
    assert!(!html.contains("<iframe"));
    assert!(html.contains("&lt;script&gt;"));
    assert!(html.contains("&#39;x&#39; &amp; &quot;y&quot;"));
    assert_eq!(html.matches("<style>").count(), 1);
    assert_eq!(html.matches("</style>").count(), 1);
}
#[test]
fn exports_reject_stale_sessions_overwrites_source_containment_and_cancellation() {
    let t = tempfile::tempdir().unwrap();
    let s = snapshot(t.path());
    let out = t.path().join("report.json");
    let roots = [s.left.clone(), s.right.clone()];
    let q = request(&s.report, &out, ReportFormat::Json);
    fs::write(&out, "keep original").unwrap();
    assert!(report::export(&s.report, &q, &roots, &AtomicBool::new(false)).is_err());
    assert_eq!(fs::read_to_string(&out).unwrap(), "keep original");
    let inside = request(&s.report, &s.left.join("report.json"), ReportFormat::Json);
    assert!(report::export(&s.report, &inside, &roots, &AtomicBool::new(false)).is_err());
    assert!(!Path::new(&inside.destination).exists());
    fs::remove_file(&out).unwrap();
    let mut bad = q.clone();
    bad.session = "expired".into();
    assert!(report::export(&s.report, &bad, &roots, &AtomicBool::new(false)).is_err());
    assert!(report::export(&s.report, &q, &roots, &AtomicBool::new(true)).is_err());
    assert!(!out.exists());
    assert_eq!(fs::read_dir(t.path()).unwrap().count(), 2);
}
#[test]
fn exports_include_entries_beyond_the_ui_page() {
    let t = tempfile::tempdir().unwrap();
    let mut s = snapshot(t.path());
    s.report.rows = (0..601)
        .map(|i| {
            let mut r = s.report.rows[0].clone();
            r.id = i.to_string();
            r.path = format!("item-{i}");
            r
        })
        .collect();
    let q = request(&s.report, &t.path().join("all.json"), ReportFormat::Json);
    let r = report::export(&s.report, &q, &[], &AtomicBool::new(false)).unwrap();
    assert_eq!(r.rows, 601);
    let json: serde_json::Value =
        serde_json::from_slice(&fs::read(&q.destination).unwrap()).unwrap();
    assert_eq!(json["rows"][600]["path"], "item-600");
}
#[cfg(unix)]
#[test]
fn symlink_aliases_cannot_bypass_destination_protection() {
    let t = tempfile::tempdir().unwrap();
    let s = snapshot(t.path());
    let alias = t.path().join("alias");
    std::os::unix::fs::symlink(&s.left, &alias).unwrap();
    let q = request(&s.report, &alias.join("out.json"), ReportFormat::Json);
    assert!(report::export(&s.report, &q, &[s.left.clone()], &AtomicBool::new(false)).is_err());
    let out = t.path().join("dangling.json");
    std::os::unix::fs::symlink(t.path().join("absent"), &out).unwrap();
    let q = request(&s.report, &out, ReportFormat::Json);
    assert!(report::export(&s.report, &q, &[], &AtomicBool::new(false)).is_err());
    assert!(fs::symlink_metadata(&out).unwrap().file_type().is_symlink());
}

#[test]
fn archive_and_tree_snapshot_reports_keep_native_source_policy_and_protection() {
    use std::io::Write;
    let t = tempfile::tempdir().unwrap();
    let s = snapshot(t.path());
    let zip_path = t.path().join("files.zip");
    let mut zip = zip::ZipWriter::new(fs::File::create(&zip_path).unwrap());
    zip.start_file("a", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"zip data").unwrap();
    zip.finish().unwrap();
    let a = clear_core::archive::compare(
        &ArchiveRequest {
            left: zip_path.display().to_string(),
            right: s.left.display().to_string(),
            exclusions: String::new(),
            content: true,
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    let roots = a.report_roots();
    assert_eq!(roots, vec![s.left.clone()]);
    let q = ReportRequest {
        source: ReportSource::Archive,
        ..request(
            &a.report,
            &t.path().join("archive.json"),
            ReportFormat::Json,
        )
    };
    report::export(&a.report, &q, &roots, &AtomicBool::new(false)).unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&fs::read(&q.destination).unwrap()).unwrap();
    assert_eq!(json["source"], "archive");
    assert_eq!(json["policy"]["verification"], "sha256");
    let saved = t.path().join("saved.cctree");
    clear_core::tree_snapshot::capture(
        &TreeSnapshotRequest {
            root: s.left.display().to_string(),
            destination: saved.display().to_string(),
            exclusions: String::new(),
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    let tree = clear_core::tree_snapshot::compare(
        &ArchiveRequest {
            left: saved.display().to_string(),
            right: s.left.display().to_string(),
            exclusions: String::new(),
            content: false,
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(tree.report_roots(), vec![s.left]);
    let q = ReportRequest {
        source: ReportSource::TreeSnapshot,
        differences_only: true,
        ..request(
            &tree.report,
            &t.path().join("tree.json"),
            ReportFormat::Json,
        )
    };
    report::export(
        &tree.report,
        &q,
        &tree.report_roots(),
        &AtomicBool::new(false),
    )
    .unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&fs::read(&q.destination).unwrap()).unwrap();
    assert_eq!(json["source"], "tree_snapshot");
    assert_eq!(json["policy"]["verification"], "size_and_time");
    assert_eq!(json["summary"]["unverified"], 2);
    assert_eq!(json["rows"].as_array().unwrap().len(), 2);
}
