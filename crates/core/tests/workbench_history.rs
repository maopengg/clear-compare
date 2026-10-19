use clear_contracts::workbench::{Action, HistoryQuery, ScanSettings};
use clear_core::workbench::Workspace;
use std::{fs, path::Path, process::Command, sync::atomic::AtomicBool};
static CANCEL: AtomicBool = AtomicBool::new(false);
fn git(root: &Path, args: &[&str]) -> String {
    let r = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        r.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    String::from_utf8_lossy(&r.stdout).trim().to_string()
}
fn fixture() -> (tempfile::TempDir, Workspace, String) {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    git(dir.path(), &["config", "user.name", "History Test"]);
    git(dir.path(), &["config", "user.email", "history@example.com"]);
    git(dir.path(), &["config", "commit.gpgsign", "false"]);
    fs::write(dir.path().join("first name.txt"), "old text\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "root commit"]);
    let mut w = Workspace::default();
    let c = w
        .scan(
            ScanSettings {
                roots: vec![dir.path().display().to_string()],
                exclusions: vec![],
            },
            &CANCEL,
        )
        .unwrap();
    let id = c.repositories[0].id.clone();
    (dir, w, id)
}
#[test]
fn root_renamed_binary_and_merged_changes_have_pinned_file_diffs() {
    let (dir, mut w, id) = fixture();
    let page = w.history(&id, &HistoryQuery::default(), &CANCEL).unwrap();
    assert_eq!(page.commits.len(), 1);
    let root = page.commits[0].commit.id.clone();
    let detail = w.commit_detail(&id, &root, 0, &CANCEL).unwrap();
    assert!(detail.parent.is_none());
    assert_eq!(detail.files[0].status, "A");
    assert!(
        w.commit_patch(&id, &detail.id, &detail.files[0].id, &CANCEL)
            .unwrap()
            .text
            .contains("+old text")
    );
    git(dir.path(), &["mv", "first name.txt", "renamed file.txt"]);
    git(dir.path(), &["commit", "-m", "rename file"]);
    let detail = w.commit_detail(&id, "HEAD", 0, &CANCEL).unwrap();
    assert!(detail.files[0].status.starts_with('R'));
    assert_eq!(
        detail.files[0].original_path.as_deref(),
        Some("first name.txt")
    );
    assert!(
        w.commit_patch(&id, &detail.id, &detail.files[0].id, &CANCEL)
            .unwrap()
            .text
            .contains("rename to renamed file.txt")
    );
    fs::write(dir.path().join("binary.dat"), [0, 1, 2, 3]).unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "binary"]);
    let binary = w.commit_detail(&id, "HEAD", 0, &CANCEL).unwrap();
    assert!(
        w.commit_patch(&id, &binary.id, &binary.files[0].id, &CANCEL)
            .unwrap()
            .binary
    );
    git(dir.path(), &["switch", "-c", "feature"]);
    fs::write(dir.path().join("feature.txt"), "feature\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "feature"]);
    git(dir.path(), &["switch", "main"]);
    fs::write(dir.path().join("main.txt"), "main\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "main"]);
    git(
        dir.path(),
        &["merge", "--no-ff", "feature", "-m", "merge feature"],
    );
    let first = w.commit_detail(&id, "HEAD", 0, &CANCEL).unwrap();
    let second = w.commit_detail(&id, "HEAD", 1, &CANCEL).unwrap();
    assert_eq!(first.commit.parents.len(), 2);
    assert_ne!(first.parent, second.parent);
    assert_eq!(first.files[0].path, "feature.txt");
    assert_eq!(second.files[0].path, "main.txt");
    // Later branch movement cannot change the original root or rename detail.
    assert!(
        w.commit_patch(&id, &detail.id, &detail.files[0].id, &CANCEL)
            .unwrap()
            .text
            .contains("rename to")
    );
    assert!(
        w.commit_patch("foreign", &detail.id, &detail.files[0].id, &CANCEL)
            .is_err()
    );
    assert!(
        w.commit_patch(&id, &detail.id, "foreign-file", &CANCEL)
            .is_err()
    );
    let history = w.history(&id, &HistoryQuery::default(), &CANCEL).unwrap();
    assert!(history.commits.iter().any(|c| c.edges.len() > 1));
    assert_eq!(history.commits[0].commit.subject, "merge feature");
}
#[test]
fn literal_filters_hash_search_paging_and_changed_references_are_explicit() {
    let (dir, mut w, id) = fixture();
    // Make 105 commits with deterministic order, without relying on wall-clock dates.
    for n in 0..105 {
        git(
            dir.path(),
            &["commit", "--allow-empty", "-m", &format!("step {n}")],
        );
    }
    let first = w.history(&id, &HistoryQuery::default(), &CANCEL).unwrap();
    assert_eq!(first.commits.len(), 100);
    assert!(first.has_more);
    let second = w
        .history(
            &id,
            &HistoryQuery {
                offset: 100,
                snapshot: Some(first.snapshot.clone()),
                ..Default::default()
            },
            &CANCEL,
        )
        .unwrap();
    assert_eq!(second.commits.len(), 6);
    assert!(!second.has_more);
    assert!(
        !first
            .commits
            .iter()
            .any(|a| second.commits.iter().any(|b| a.commit.id == b.commit.id))
    );
    let query = HistoryQuery {
        search: "step 104".into(),
        ..Default::default()
    };
    assert_eq!(w.history(&id, &query, &CANCEL).unwrap().commits.len(), 1);
    let hash = first.commits[0].commit.id[..8].to_string();
    let p = w
        .history(
            &id,
            &HistoryQuery {
                search: hash,
                ..Default::default()
            },
            &CANCEL,
        )
        .unwrap();
    assert_eq!(p.commits.len(), 1);
    assert_eq!(p.commits[0].commit.id, first.commits[0].commit.id);
    assert!(
        w.history(
            &id,
            &HistoryQuery {
                author: "[no regex]".into(),
                ..Default::default()
            },
            &CANCEL
        )
        .unwrap()
        .commits
        .is_empty()
    );
    assert_eq!(
        w.history(
            &id,
            &HistoryQuery {
                path: Some("first name.txt".into()),
                ..Default::default()
            },
            &CANCEL
        )
        .unwrap()
        .commits
        .len(),
        1
    );
    assert!(
        w.history(
            &id,
            &HistoryQuery {
                path: Some("../outside".into()),
                ..Default::default()
            },
            &CANCEL
        )
        .is_err()
    );
    git(
        dir.path(),
        &["commit", "--allow-empty", "-m", "external commit"],
    );
    assert!(
        w.history(
            &id,
            &HistoryQuery {
                offset: 100,
                snapshot: Some(first.snapshot),
                ..Default::default()
            },
            &CANCEL
        )
        .is_err()
    );
    assert!(
        w.history(&id, &HistoryQuery::default(), &AtomicBool::new(true))
            .is_err()
    );
}
#[test]
fn file_names_that_look_like_pathspecs_are_literal_for_writes_and_history() {
    let (dir, mut w, id) = fixture();
    let name = if cfg!(windows) {
        "[literal].txt"
    } else {
        ":(glob)*"
    };
    fs::write(dir.path().join(name), "literal\n").unwrap();
    fs::write(dir.path().join("other.txt"), "other\n").unwrap();
    let s = w.open(&id, &CANCEL).unwrap();
    let file = s.changes.iter().find(|c| c.path == name).unwrap();
    let p = w
        .prepare(
            &id,
            &s.revision,
            Action::Stage {
                files: vec![file.id.clone()],
            },
            &CANCEL,
        )
        .unwrap();
    w.apply(&p.id, &CANCEL).unwrap();
    assert_eq!(git(dir.path(), &["diff", "--cached", "--name-only"]), name);
    git(dir.path(), &["commit", "-m", "literal name"]);
    let p = w
        .history(
            &id,
            &HistoryQuery {
                path: Some(name.into()),
                ..Default::default()
            },
            &CANCEL,
        )
        .unwrap();
    assert_eq!(p.commits.len(), 1);
    assert_eq!(p.commits[0].commit.subject, "literal name");
}

#[test]
fn patch_pages_are_bounded_and_report_long_line_previews() {
    let (dir, mut w, id) = fixture();
    let contents = (0..1200).map(|n| format!("line {n}\n")).collect::<String>();
    fs::write(dir.path().join("long.txt"), contents).unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "many lines"]);
    let detail = w.commit_detail(&id, "HEAD", 0, &CANCEL).unwrap();
    let file = detail.files.iter().find(|f| f.path == "long.txt").unwrap();
    let first = w.commit_patch(&id, &detail.id, &file.id, &CANCEL).unwrap();
    assert_eq!(first.text.lines().count(), 500);
    assert_eq!(first.next_offset, Some(500));
    assert!(first.total_lines > 1200);
    let next = w
        .commit_patch_page(&id, &detail.id, &file.id, 500, &CANCEL)
        .unwrap();
    assert_eq!(next.offset, 500);
    assert_eq!(next.text.lines().count(), 500);
    assert_ne!(first.text, next.text);
    assert!(
        w.commit_patch_page(&id, &detail.id, &file.id, first.total_lines + 1, &CANCEL)
            .is_err()
    );
    fs::write(dir.path().join("wide.txt"), "a".repeat(10_000)).unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "wide line"]);
    let detail = w.commit_detail(&id, "HEAD", 0, &CANCEL).unwrap();
    let patch = w
        .commit_patch(&id, &detail.id, &detail.files[0].id, &CANCEL)
        .unwrap();
    assert!(patch.long_lines);
    assert!(patch.text.lines().all(|l| l.chars().count() <= 8193));
}

#[test]
fn child_navigation_follows_all_branches_merge_parents_and_captured_details() {
    let (dir, mut w, id) = fixture();
    let root = git(dir.path(), &["rev-parse", "HEAD"]);
    let detail = w.commit_detail(&id, &root, 0, &CANCEL).unwrap();
    git(dir.path(), &["commit", "--allow-empty", "-m", "main child"]);
    let main = git(dir.path(), &["rev-parse", "HEAD"]);
    git(dir.path(), &["switch", "-c", "feature", &root]);
    git(
        dir.path(),
        &["commit", "--allow-empty", "-m", "feature child"],
    );
    let feature = git(dir.path(), &["rev-parse", "HEAD"]);
    git(dir.path(), &["switch", "main"]);
    git(dir.path(), &["merge", "--no-ff", "feature", "-m", "merge"]);
    let merge = git(dir.path(), &["rev-parse", "HEAD"]);
    let children = w
        .commit_children(&id, &detail.id, 0, None, &CANCEL)
        .unwrap();
    assert_eq!(children.target, root);
    assert_eq!(children.total, 2);
    assert!(children.children.contains(&main));
    assert!(children.children.contains(&feature));
    assert!(!children.children.contains(&merge));
    for hash in [&main, &feature] {
        let d = w.commit_detail(&id, hash, 0, &CANCEL).unwrap();
        assert_eq!(
            w.commit_children(&id, &d.id, 0, None, &CANCEL)
                .unwrap()
                .children,
            vec![merge.clone()]
        );
    }
    let tip = w.commit_detail(&id, &merge, 0, &CANCEL).unwrap();
    assert_eq!(
        w.commit_children(&id, &tip.id, 0, None, &CANCEL)
            .unwrap()
            .total,
        0
    );
    assert!(
        w.commit_children("foreign", &detail.id, 0, None, &CANCEL)
            .is_err()
    );
    assert!(w.commit_children(&id, "foreign", 0, None, &CANCEL).is_err());
    assert!(
        w.commit_children(&id, &detail.id, usize::MAX, None, &CANCEL)
            .is_err()
    );
    assert!(
        w.commit_children(&id, &detail.id, 0, None, &AtomicBool::new(true))
            .is_err()
    );
    git(dir.path(), &["switch", "--detach", &root]);
    git(
        dir.path(),
        &["commit", "--allow-empty", "-m", "detached child"],
    );
    let detached = git(dir.path(), &["rev-parse", "HEAD"]);
    assert!(
        w.commit_children(&id, &detail.id, 0, None, &CANCEL)
            .unwrap()
            .children
            .contains(&detached)
    );
    git(dir.path(), &["tag", "-a", "only-tag", "-m", "kept by tag"]);
    git(dir.path(), &["switch", "main"]);
    assert!(
        w.commit_children(&id, &detail.id, 0, None, &CANCEL)
            .unwrap()
            .children
            .contains(&detached)
    );
    assert!(
        w.commit_children(&id, &detail.id, 0, Some(&children.snapshot), &CANCEL)
            .is_err()
    );
}

#[test]
fn child_navigation_pages_fanout_and_rejects_changed_reference_snapshot() {
    let (dir, mut w, id) = fixture();
    let root = git(dir.path(), &["rev-parse", "HEAD"]);
    let tree = git(dir.path(), &["rev-parse", "HEAD^{tree}"]);
    let detail = w.commit_detail(&id, &root, 0, &CANCEL).unwrap();
    for n in 0..105 {
        let child = git(
            dir.path(),
            &[
                "commit-tree",
                &tree,
                "-p",
                &root,
                "-m",
                &format!("child {n}"),
            ],
        );
        let reference = if n % 2 == 0 {
            format!("refs/heads/child-{n}")
        } else {
            format!("refs/remotes/example/child-{n}")
        };
        git(dir.path(), &["update-ref", &reference, &child]);
    }
    let first = w
        .commit_children(&id, &detail.id, 0, None, &CANCEL)
        .unwrap();
    assert_eq!(first.total, 105);
    assert_eq!(first.children.len(), 100);
    assert_eq!(first.next_offset, Some(100));
    let second = w
        .commit_children(&id, &detail.id, 100, Some(&first.snapshot), &CANCEL)
        .unwrap();
    assert_eq!(second.children.len(), 5);
    assert_eq!(second.next_offset, None);
    assert!(second.children.iter().all(|c| !first.children.contains(c)));
    git(dir.path(), &["update-ref", "-d", "refs/heads/child-0"]);
    assert!(
        w.commit_children(&id, &detail.id, 100, Some(&first.snapshot), &CANCEL)
            .is_err()
    );
    assert_eq!(
        w.commit_children(&id, &detail.id, 0, None, &CANCEL)
            .unwrap()
            .total,
        104
    );
}
