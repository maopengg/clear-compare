use clear_contracts::{GitCompareRequest, Status, TextOptions};
use clear_core::git;
use std::{fs, path::Path, process::Command, sync::atomic::AtomicBool};
fn run(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(repo)
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.autocrlf=false",
            "-c",
            "core.hooksPath=",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{:?}: {}",
        args,
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}
fn fixture() -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    run(t.path(), &["init", "-q", "--template="]);
    fs::create_dir(t.path().join("sub")).unwrap();
    fs::write(t.path().join("sub/a.txt"), "first\n").unwrap();
    fs::write(t.path().join("same"), "same\n").unwrap();
    run(t.path(), &["add", "."]);
    run(t.path(), &["commit", "-qm", "first"]);
    fs::write(t.path().join("sub/a.txt"), "second\n").unwrap();
    fs::write(t.path().join("new file.txt"), "new\n").unwrap();
    run(t.path(), &["add", "."]);
    run(t.path(), &["commit", "-qm", "second"]);
    t
}
fn req(repo: &Path) -> GitCompareRequest {
    GitCompareRequest {
        repository: repo.display().to_string(),
        left_revision: "HEAD~1".into(),
        right_revision: "HEAD".into(),
        exclusions: String::new(),
    }
}
fn cancel() -> AtomicBool {
    AtomicBool::new(false)
}
#[test]
fn committed_trees_are_pinned_without_touching_dirty_worktree_or_index() {
    let t = fixture();
    let repo = t.path();
    fs::write(repo.join("sub/a.txt"), "dirty working copy").unwrap();
    fs::write(repo.join("untracked"), "untouched").unwrap();
    let index = fs::read(repo.join(".git/index")).unwrap();
    let head = fs::read(repo.join(".git/HEAD")).unwrap();
    let r = git::compare(&req(repo), &cancel()).unwrap();
    assert_eq!(r.report.summary.same, 1);
    assert_eq!(r.report.summary.changed, 1);
    assert_eq!(r.report.summary.right_only, 1);
    assert!(!r.report.rows.iter().any(|r| r.path == "untracked"));
    let file = r
        .report
        .rows
        .iter()
        .find(|r| r.path == "sub/a.txt")
        .unwrap();
    assert_eq!(
        r.report
            .rows
            .iter()
            .find(|r| r.path == "sub")
            .unwrap()
            .status,
        Status::Changed
    );
    let diff = r
        .text_diff(&file.id, &TextOptions::default(), &cancel())
        .unwrap();
    assert!(!format!("{diff:?}").contains("dirty working copy"));
    run(repo, &["update-ref", "HEAD", "HEAD~1"]);
    assert_eq!(
        serde_json::to_string(
            &r.text_diff(&file.id, &TextOptions::default(), &cancel())
                .unwrap()
        )
        .unwrap(),
        serde_json::to_string(&diff).unwrap()
    );
    assert_eq!(fs::read(repo.join(".git/index")).unwrap(), index);
    assert_eq!(fs::read(repo.join(".git/HEAD")).unwrap(), head);
    assert_eq!(
        fs::read_to_string(repo.join("sub/a.txt")).unwrap(),
        "dirty working copy"
    );
    assert_eq!(r.page(0, "new file", false).unwrap().total, 1);
    assert_eq!(r.page(0, "", true).unwrap().total, 3);
    assert!(
        r.text_diff("missing", &TextOptions::default(), &cancel())
            .is_err()
    );
}
#[test]
fn exclusions_and_revision_validation_are_applied_before_use() {
    let t = fixture();
    let mut q = req(t.path());
    q.exclusions = "-./sub/".into();
    let r = git::compare(&q, &cancel()).unwrap();
    assert!(!r.report.rows.iter().any(|r| r.path.starts_with("sub")));
    for rev in ["--help", "does-not-exist", "HEAD\nHEAD", "HEAD:sub/a.txt"] {
        q.left_revision = rev.into();
        assert!(git::compare(&q, &cancel()).is_err());
    }
    assert!(git::compare(&req(t.path()), &AtomicBool::new(true)).is_err());
}
#[test]
fn unusual_filenames_and_missing_sides_have_bounded_text_previews() {
    let t = fixture();
    let repo = t.path();
    let name = if cfg!(windows) {
        "space file.txt"
    } else {
        "space and tab\tfile.txt"
    };
    fs::write(repo.join(name), "text\n").unwrap();
    run(repo, &["add", "."]);
    run(repo, &["commit", "-qm", "tab"]);
    let r = git::compare(&req(repo), &cancel()).unwrap();
    let row = r.report.rows.iter().find(|r| r.path == name).unwrap();
    assert_eq!(row.status, Status::RightOnly);
    assert!(
        r.text_diff(&row.id, &TextOptions::default(), &cancel())
            .is_ok()
    );
    fs::write(repo.join("large"), vec![b'x'; 4 * 1024 * 1024 + 1]).unwrap();
    run(repo, &["add", "large"]);
    run(repo, &["commit", "-qm", "large"]);
    let r = git::compare(&req(repo), &cancel()).unwrap();
    let row = r.report.rows.iter().find(|r| r.path == "large").unwrap();
    assert!(
        r.text_diff(&row.id, &TextOptions::default(), &cancel())
            .is_err()
    );
}
#[cfg(unix)]
#[test]
fn links_are_not_followed_and_executable_mode_changes_are_visible() {
    let t = fixture();
    let repo = t.path();
    std::os::unix::fs::symlink("/not/read/from/worktree", repo.join("link")).unwrap();
    run(repo, &["add", "link"]);
    run(repo, &["update-index", "--chmod=+x", "same"]);
    run(repo, &["commit", "-qm", "mode and link"]);
    let r = git::compare(&req(repo), &cancel()).unwrap();
    assert_eq!(
        r.report
            .rows
            .iter()
            .find(|r| r.path == "same")
            .unwrap()
            .status,
        Status::Changed
    );
    let link = r.report.rows.iter().find(|r| r.path == "link").unwrap();
    assert!(
        r.text_diff(&link.id, &TextOptions::default(), &cancel())
            .is_err()
    );
    let mut q = req(repo);
    q.left_revision = "HEAD".into();
    let r = git::compare(&q, &cancel()).unwrap();
    assert_eq!(
        r.report
            .rows
            .iter()
            .find(|r| r.path == "link")
            .unwrap()
            .status,
        Status::MetadataMatch
    );
}

#[test]
fn missing_local_objects_fail_without_fetching_or_using_worktree_fallbacks() {
    let t = fixture();
    let repo = t.path();
    let q = req(repo);
    let r = git::compare(&q, &cancel()).unwrap();
    let row = r
        .report
        .rows
        .iter()
        .find(|r| r.path == "sub/a.txt")
        .unwrap();
    let oid = run(repo, &["rev-parse", "HEAD:sub/a.txt"]);
    let object = repo.join(".git/objects").join(&oid[..2]).join(&oid[2..]);
    fs::remove_file(&object).unwrap();
    run(repo, &["config", "remote.origin.promisor", "true"]);
    run(
        repo,
        &[
            "config",
            "remote.origin.url",
            "https://example.invalid/never-fetch",
        ],
    );
    assert!(
        r.text_diff(&row.id, &TextOptions::default(), &cancel())
            .is_err()
    );
    assert!(!object.exists());
    assert!(git::compare(&q, &cancel()).is_err());
    assert!(!object.exists());
}
