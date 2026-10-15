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
        left_source: clear_contracts::GitSource::Commit,
        right_source: clear_contracts::GitSource::Commit,
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
                .lines
        )
        .unwrap(),
        serde_json::to_string(&diff.lines).unwrap()
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

#[test]
fn report_protection_uses_actual_repository_roots_from_a_subdirectory() {
    let source = fixture();
    let outer = tempfile::tempdir().unwrap();
    let moved = outer.path().join("repo with space");
    std::fs::rename(source.path(), &moved).unwrap();
    let r = git::compare(&req(&moved.join("sub")), &cancel()).unwrap();
    let roots = r.report_roots();
    assert!(roots.contains(&fs::canonicalize(&moved).unwrap()));
    assert!(roots.contains(&fs::canonicalize(moved.join(".git")).unwrap()));
    let request = clear_contracts::ReportRequest {
        source: clear_contracts::ReportSource::Git,
        format: clear_contracts::ReportFormat::Json,
        session: r.report.session.clone(),
        destination: moved.join("report.json").display().to_string(),
        english: true,
        differences_only: false,
    };
    assert!(clear_core::report::export(&r.report, &request, &roots, &cancel()).is_err());
    assert!(!moved.join("report.json").exists());
}

#[test]
fn linked_worktree_reports_protect_common_object_storage() {
    let repo = fixture();
    let outer = tempfile::tempdir().unwrap();
    let worktree = outer.path().join("linked");
    run(
        repo.path(),
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            worktree.to_str().unwrap(),
            "HEAD",
        ],
    );
    let r = git::compare(&req(&worktree), &cancel()).unwrap();
    let roots = r.report_roots();
    assert!(roots.contains(&fs::canonicalize(repo.path().join(".git")).unwrap()));
    assert!(roots.contains(&fs::canonicalize(&worktree).unwrap()));
    let request = clear_contracts::ReportRequest {
        source: clear_contracts::ReportSource::Git,
        format: clear_contracts::ReportFormat::Json,
        session: r.report.session.clone(),
        destination: repo.path().join(".git/report.json").display().to_string(),
        english: true,
        differences_only: false,
    };
    assert!(clear_core::report::export(&r.report, &request, &roots, &cancel()).is_err());
    assert!(!repo.path().join(".git/report.json").exists());
}

fn index_request(repo: &Path) -> GitCompareRequest {
    let mut request = req(repo);
    request.left_revision = "HEAD".into();
    request.right_source = clear_contracts::GitSource::Index;
    request
}
#[test]
fn captured_index_ignores_worktree_bytes_and_pins_staged_objects() {
    let t = fixture();
    let repo = t.path();
    fs::write(repo.join("sub/a.txt"), "staged version\n").unwrap();
    fs::write(repo.join("added"), "added\n").unwrap();
    run(repo, &["add", "sub/a.txt", "added"]);
    run(repo, &["rm", "--cached", "new file.txt"]);
    fs::write(repo.join("sub/a.txt"), "working only\n").unwrap();
    fs::write(repo.join("untracked"), "untracked\n").unwrap();
    let before = fs::read(repo.join(".git/index")).unwrap();
    let head = fs::read(repo.join(".git/HEAD")).unwrap();
    let q = index_request(&repo.join("sub"));
    let result = git::compare(&q, &cancel()).unwrap();
    assert_eq!(fs::read(repo.join(".git/index")).unwrap(), before);
    assert_eq!(fs::read(repo.join(".git/HEAD")).unwrap(), head);
    assert!(!repo.join(".git/index.lock").exists());
    assert_eq!(
        fs::read_to_string(repo.join("sub/a.txt")).unwrap(),
        "working only\n"
    );
    assert!(result.report.right.starts_with("Index @ "));
    let row = |path: &str| result.report.rows.iter().find(|r| r.path == path).unwrap();
    assert_eq!(row("added").status, Status::RightOnly);
    assert_eq!(row("new file.txt").status, Status::LeftOnly);
    assert_eq!(row("sub").status, Status::Changed);
    assert_eq!(row("sub/a.txt").right.as_ref().unwrap().bytes, "15");
    assert!(!result.report.rows.iter().any(|r| r.path == "untracked"));
    run(repo, &["add", "sub/a.txt"]);
    let diff = result
        .text_diff(&row("sub/a.txt").id, &TextOptions::default(), &cancel())
        .unwrap();
    assert!(
        diff.lines
            .iter()
            .any(|l| l.right_text.as_deref().unwrap_or(&l.text) == "staged version\n")
    );
    assert!(!diff.lines.iter().any(|l| {
        l.text.contains("working only")
            || l.right_text
                .as_ref()
                .is_some_and(|t| t.contains("working only"))
    }));
    let refreshed = git::compare(&q, &cancel()).unwrap();
    assert_ne!(result.report.right, refreshed.report.right);
    let mut reverse = index_request(repo);
    reverse.left_source = clear_contracts::GitSource::Index;
    reverse.right_source = clear_contracts::GitSource::Commit;
    let reverse = git::compare(&reverse, &cancel()).unwrap();
    assert_eq!(
        reverse
            .report
            .rows
            .iter()
            .find(|r| r.path == "added")
            .unwrap()
            .status,
        Status::LeftOnly
    );
}
#[test]
fn empty_and_unborn_index_sources_do_not_require_a_commit() {
    let t = tempfile::tempdir().unwrap();
    run(t.path(), &["init", "-q", "--template="]);
    let mut q = index_request(t.path());
    q.left_source = clear_contracts::GitSource::Index;
    q.left_revision = String::new();
    q.right_revision = "--not-a-revision".into();
    let empty = git::compare(&q, &cancel()).unwrap();
    assert!(empty.report.rows.is_empty());
    assert_eq!(empty.report.left, empty.report.right);
    assert!(!t.path().join(".git/index").exists());
    fs::write(t.path().join("first"), "first\n").unwrap();
    run(t.path(), &["add", "first"]);
    let result = git::compare(&q, &cancel()).unwrap();
    assert_eq!(result.report.summary.same, 1);
    let row = &result.report.rows[0];
    assert_eq!(
        result
            .text_diff(&row.id, &TextOptions::default(), &cancel())
            .unwrap()
            .lines[0]
            .text,
        "first\n"
    );
}
#[test]
fn index_executable_modes_exclusions_and_intent_to_add_are_explicit() {
    let t = fixture();
    let repo = t.path();
    run(repo, &["update-index", "--chmod=+x", "same"]);
    fs::create_dir(repo.join("hidden")).unwrap();
    fs::write(repo.join("hidden/item"), "secret\n").unwrap();
    run(repo, &["add", "hidden/item"]);
    fs::write(repo.join("intent"), "not staged contents\n").unwrap();
    run(repo, &["add", "-N", "intent"]);
    let mut q = index_request(repo);
    q.exclusions = "-./hidden/".into();
    let result = git::compare(&q, &cancel()).unwrap();
    assert_eq!(
        result
            .report
            .rows
            .iter()
            .find(|r| r.path == "same")
            .unwrap()
            .status,
        Status::Changed
    );
    assert!(
        !result
            .report
            .rows
            .iter()
            .any(|r| r.path.starts_with("hidden"))
    );
    let intent = result
        .report
        .rows
        .iter()
        .find(|r| r.path == "intent")
        .unwrap();
    assert_eq!(intent.right.as_ref().unwrap().bytes, "0");
    assert!(
        result
            .text_diff(&intent.id, &TextOptions::default(), &cancel())
            .unwrap()
            .lines
            .is_empty()
    );
    assert!(
        result
            .report
            .warnings
            .iter()
            .any(|w| w.contains("Intent-to-add"))
    );
}
#[test]
fn unmerged_index_stages_are_errors_instead_of_an_arbitrary_version() {
    use std::{io::Write, process::Stdio};
    let t = fixture();
    let repo = t.path();
    let base = run(repo, &["rev-parse", "HEAD~1:sub/a.txt"]);
    let changed = run(repo, &["rev-parse", "HEAD:sub/a.txt"]);
    let mut child = Command::new("git")
        .current_dir(repo)
        .args(["update-index", "--index-info"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(format!("100644 {base} 1\tconflict.txt\n100644 {base} 2\tconflict.txt\n100644 {changed} 3\tconflict.txt\n").as_bytes()).unwrap();
    assert!(child.wait().unwrap().success());
    let before = fs::read(repo.join(".git/index")).unwrap();
    let result = git::compare(&index_request(repo), &cancel()).unwrap();
    let row = result
        .report
        .rows
        .iter()
        .find(|r| r.path == "conflict.txt")
        .unwrap();
    assert_eq!(row.status, Status::Error);
    assert!(
        row.right
            .as_ref()
            .unwrap()
            .error
            .as_ref()
            .unwrap()
            .contains("1, 2, 3")
    );
    assert!(
        result
            .text_diff(&row.id, &TextOptions::default(), &cancel())
            .is_err()
    );
    assert_eq!(fs::read(repo.join(".git/index")).unwrap(), before);
}
#[test]
fn index_selection_uses_the_linked_worktrees_own_index_and_rejects_bare_repos() {
    let t = fixture();
    let outer = tempfile::tempdir().unwrap();
    let linked = outer.path().join("linked");
    run(
        t.path(),
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            linked.to_str().unwrap(),
            "HEAD",
        ],
    );
    fs::write(linked.join("same"), "linked staged\n").unwrap();
    run(&linked, &["add", "same"]);
    let primary = fs::read(t.path().join(".git/index")).unwrap();
    let result = git::compare(&index_request(&linked), &cancel()).unwrap();
    assert_eq!(result.report.summary.changed, 1);
    assert_eq!(fs::read(t.path().join(".git/index")).unwrap(), primary);
    assert_eq!(
        git::compare(&index_request(t.path()), &cancel())
            .unwrap()
            .report
            .summary
            .changed,
        0
    );
    let bare = outer.path().join("bare");
    fs::create_dir(&bare).unwrap();
    run(&bare, &["init", "--bare", "-q", "--template="]);
    assert!(git::compare(&index_request(&bare), &cancel()).is_err());
}
#[test]
fn sha256_index_object_ids_and_submodule_references_are_handled() {
    let t = tempfile::tempdir().unwrap();
    let repo = t.path();
    run(
        repo,
        &["init", "-q", "--template=", "--object-format=sha256"],
    );
    fs::write(repo.join("a"), "a\n").unwrap();
    run(repo, &["add", "a"]);
    run(repo, &["commit", "-qm", "first"]);
    let mut q = index_request(repo);
    let result = git::compare(&q, &cancel()).unwrap();
    assert_eq!(result.report.summary.same, 1);
    let head = run(repo, &["rev-parse", "HEAD"]);
    assert_eq!(head.len(), 64);
    run(
        repo,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{head},module"),
        ],
    );
    let result = git::compare(&q, &cancel()).unwrap();
    let module = result
        .report
        .rows
        .iter()
        .find(|r| r.path == "module")
        .unwrap();
    assert_eq!(module.status, Status::Error);
    q.left_source = clear_contracts::GitSource::Index;
    assert_eq!(
        git::compare(&q, &cancel()).unwrap().report.summary.errors,
        1
    );
}

#[test]
fn split_and_sparse_indexes_are_read_through_git_without_rewriting_them() {
    let t = fixture();
    let repo = t.path();
    fs::create_dir(repo.join("outside")).unwrap();
    fs::write(repo.join("outside/file"), "outside\n").unwrap();
    run(repo, &["add", "outside/file"]);
    run(repo, &["commit", "-qm", "outside"]);
    run(repo, &["update-index", "--split-index"]);
    let before = fs::read(repo.join(".git/index")).unwrap();
    let result = git::compare(&index_request(repo), &cancel()).unwrap();
    assert_eq!(result.report.summary.changed, 0);
    assert_eq!(fs::read(repo.join(".git/index")).unwrap(), before);
    run(repo, &["update-index", "--no-split-index"]);
    run(
        repo,
        &["sparse-checkout", "init", "--cone", "--sparse-index"],
    );
    run(repo, &["sparse-checkout", "set", "sub"]);
    assert!(!repo.join("outside/file").exists());
    let before = fs::read(repo.join(".git/index")).unwrap();
    let result = git::compare(&index_request(repo), &cancel()).unwrap();
    assert_eq!(result.report.summary.changed, 0);
    assert_eq!(
        result
            .report
            .rows
            .iter()
            .find(|r| r.path == "outside/file")
            .unwrap()
            .status,
        Status::Same
    );
    assert_eq!(fs::read(repo.join(".git/index")).unwrap(), before);
    assert!(!repo.join("outside/file").exists());
}
