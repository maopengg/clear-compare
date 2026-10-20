use clear_contracts::workbench::*;
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
    String::from_utf8_lossy(&r.stdout).trim().into()
}
fn fixture() -> (tempfile::TempDir, Workspace, String, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "user.name", "历史测试"]);
    git(root, &["config", "user.email", "history@example.com"]);
    git(root, &["config", "commit.gpgsign", "false"]);
    git(root, &["config", "core.autocrlf", "false"]);
    git(
        root,
        &[
            "config",
            "core.hooksPath",
            root.join(".git/no-hooks").to_str().unwrap(),
        ],
    );
    fs::write(root.join("old name.txt"), "root line\r\n共同\r\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "root"]);
    let first = git(root, &["rev-parse", "HEAD"]);
    fs::write(root.join("old name.txt"), "root line\r\nchanged\r\n").unwrap();
    git(root, &["commit", "-am", "edit before rename"]);
    git(root, &["mv", "old name.txt", "renamed 文件.txt"]);
    git(root, &["commit", "-m", "rename"]);
    fs::write(
        root.join("renamed 文件.txt"),
        "root line\r\nchanged\r\nlatest\n",
    )
    .unwrap();
    git(root, &["commit", "-am", "latest"]);
    let latest = git(root, &["rev-parse", "HEAD"]);
    let mut w = Workspace::default();
    let c = w
        .scan(
            ScanSettings {
                roots: vec![root.to_str().unwrap().into()],
                exclusions: vec![],
            },
            &CANCEL,
        )
        .unwrap();
    let id = c.repositories[0].id.clone();
    (dir, w, id, first, latest)
}
fn source(w: &mut Workspace, id: &str) -> FileHistorySource {
    let file = w
        .file_tree(id, None, 0, &CANCEL)
        .unwrap()
        .files
        .into_iter()
        .find(|f| f.name == "renamed 文件.txt")
        .unwrap();
    FileHistorySource::Source {
        repository_id: id.into(),
        file_id: file.id,
    }
}
#[test]
fn follows_renames_pins_anchor_and_opens_exact_old_version_without_checkout() {
    let (dir, mut w, id, first, latest) = fixture();
    let source = source(&mut w, &id);
    let page = w.file_history_open(source, &CANCEL).unwrap();
    assert_eq!(page.anchor, latest);
    assert_eq!(page.entries.len(), 4);
    assert_eq!(page.entries.last().unwrap().commit.id, first);
    assert_eq!(page.entries.last().unwrap().path, "old name.txt");
    assert!(
        page.entries
            .iter()
            .any(|entry| entry.previous_path.as_deref() == Some("old name.txt"))
    );
    fs::write(dir.path().join("renamed 文件.txt"), "unsaved on disk\n").unwrap();
    git(dir.path(), &["commit", "-am", "later"]);
    let pinned = w.file_history_page(&page.id, 0, &CANCEL).unwrap();
    assert_eq!(pinned.anchor, latest);
    assert_eq!(pinned.total, 4);
    let focus = w
        .file_history_version(
            &page.id,
            &TraceTarget::Commit {
                id: page.entries.last().unwrap().id.clone(),
            },
            &CANCEL,
        )
        .unwrap();
    assert_eq!(focus.tree.commit, first);
    assert_eq!(focus.entry.path, "old name.txt");
    let body = w
        .revision_content(&id, &focus.tree.id, &focus.entry.id, "text", 0, &CANCEL)
        .unwrap();
    assert_eq!(body.text, "root line\n共同");
    assert_eq!(
        fs::read_to_string(dir.path().join("renamed 文件.txt")).unwrap(),
        "unsaved on disk\n"
    );
    assert!(!dir.path().join("old name.txt").exists());
}
#[test]
fn blame_matches_pinned_crlf_bytes_authors_and_original_rename_paths() {
    let (dir, mut w, id, first, latest) = fixture();
    let src = source(&mut w, &id);
    let page = w.file_history_open(src, &CANCEL).unwrap();
    fs::write(dir.path().join("renamed 文件.txt"), "disk draft\n").unwrap();
    let blame = w
        .file_blame(&page.id, &TraceTarget::Anchor, 0, &CANCEL)
        .unwrap();
    assert_eq!(blame.commit, latest);
    assert_eq!(blame.total, 3);
    assert_eq!(blame.lines[0].commit, first);
    assert_eq!(blame.lines[0].original_path, "old name.txt");
    assert_eq!(blame.lines[0].author, "历史测试");
    assert_eq!(blame.lines[0].text, "root line");
    assert_eq!(blame.lines[2].text, "latest");
    assert_eq!(blame.lines[2].commit, latest);
    let target = w
        .file_history_version(
            &page.id,
            &TraceTarget::Line {
                id: blame.lines[0].id.clone(),
            },
            &CANCEL,
        )
        .unwrap();
    assert_eq!(target.entry.path, "old name.txt");
    assert_eq!(target.tree.commit, first);
    assert_eq!(target.line, 1);
    assert_eq!(
        fs::read_to_string(dir.path().join("renamed 文件.txt")).unwrap(),
        "disk draft\n"
    );
}
#[test]
fn historical_source_and_unborn_or_untracked_working_files_are_explicit() {
    let (dir, mut w, id, first, _) = fixture();
    let tree = w.revision_open(&id, &first, &CANCEL).unwrap();
    let file = tree
        .entries
        .iter()
        .find(|file| file.name == "old name.txt")
        .unwrap();
    let page = w
        .file_history_open(
            FileHistorySource::Revision {
                repository_id: id.clone(),
                session: tree.id,
                file_id: file.id.clone(),
            },
            &CANCEL,
        )
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(
        w.file_blame(&page.id, &TraceTarget::Anchor, 0, &CANCEL)
            .unwrap()
            .lines
            .len(),
        2
    );
    fs::write(dir.path().join("new.txt"), "draft\n").unwrap();
    let file = w
        .file_tree(&id, None, 0, &CANCEL)
        .unwrap()
        .files
        .into_iter()
        .find(|file| file.name == "new.txt")
        .unwrap();
    let page = w
        .file_history_open(
            FileHistorySource::Source {
                repository_id: id.clone(),
                file_id: file.id,
            },
            &CANCEL,
        )
        .unwrap();
    assert_eq!(page.status, "untracked_at_revision");
    assert_eq!(page.total, 0);
    assert!(
        w.file_blame(&page.id, &TraceTarget::Anchor, 0, &CANCEL)
            .is_err()
    );
    let base = tempfile::tempdir().unwrap();
    git(base.path(), &["init", "-b", "main"]);
    fs::write(base.path().join("first.txt"), "draft").unwrap();
    let mut w = Workspace::default();
    let catalog = w
        .scan(
            ScanSettings {
                roots: vec![base.path().to_str().unwrap().into()],
                exclusions: vec![],
            },
            &CANCEL,
        )
        .unwrap();
    let id = catalog.repositories[0].id.clone();
    let file = w.file_tree(&id, None, 0, &CANCEL).unwrap().files[0].clone();
    let page = w
        .file_history_open(
            FileHistorySource::Source {
                repository_id: id,
                file_id: file.id,
            },
            &CANCEL,
        )
        .unwrap();
    assert_eq!(page.status, "unborn");
    assert!(page.anchor.is_empty());
    assert!(
        w.file_history_version(&page.id, &TraceTarget::Anchor, &CANCEL)
            .is_err()
    );
}
#[test]
fn paging_blame_long_lines_empty_files_and_wrong_selections_are_bounded() {
    let (dir, mut w, id, _, _) = fixture();
    fs::write(
        dir.path().join("renamed 文件.txt"),
        format!("{}{}\n", "line\n".repeat(450), "x".repeat(9000)),
    )
    .unwrap();
    git(dir.path(), &["commit", "-am", "many lines"]);
    let src = source(&mut w, &id);
    let page = w.file_history_open(src, &CANCEL).unwrap();
    let first = w
        .file_blame(&page.id, &TraceTarget::Anchor, 0, &CANCEL)
        .unwrap();
    assert_eq!(first.lines.len(), 200);
    assert_eq!(first.next_offset, Some(200));
    let last = w
        .file_blame(&page.id, &TraceTarget::Anchor, 400, &CANCEL)
        .unwrap();
    assert_eq!(last.lines.len(), 51);
    assert!(last.lines.last().unwrap().long_line);
    assert_eq!(last.lines.last().unwrap().text.len(), 8192);
    assert!(
        w.file_blame(&page.id, &TraceTarget::Anchor, 452, &CANCEL)
            .is_err()
    );
    assert!(
        w.file_blame(
            &page.id,
            &TraceTarget::Line {
                id: "foreign".into()
            },
            0,
            &CANCEL
        )
        .is_err()
    );
    assert!(w.file_history_page(&page.id, 999, &CANCEL).is_err());
    assert!(
        w.file_history_version(
            &page.id,
            &TraceTarget::Commit {
                id: "foreign".into()
            },
            &CANCEL
        )
        .is_err()
    );
    assert!(w.file_history_page("foreign", 0, &CANCEL).is_err());
    fs::write(dir.path().join("renamed 文件.txt"), "").unwrap();
    git(dir.path(), &["commit", "-am", "empty"]);
    let src = source(&mut w, &id);
    let empty = w.file_history_open(src, &CANCEL).unwrap();
    assert_eq!(
        w.file_blame(&empty.id, &TraceTarget::Anchor, 0, &CANCEL)
            .unwrap()
            .total,
        0
    );
}
#[test]
fn binary_link_cancelled_and_replaced_repositories_do_not_fake_attribution() {
    let (dir, mut w, id, _, _) = fixture();
    fs::write(dir.path().join("renamed 文件.txt"), [0, 255, 1, 10]).unwrap();
    git(dir.path(), &["commit", "-am", "binary"]);
    let src = source(&mut w, &id);
    let page = w.file_history_open(src, &CANCEL).unwrap();
    assert!(
        w.file_blame(&page.id, &TraceTarget::Anchor, 0, &CANCEL)
            .unwrap_err()
            .to_string()
            .contains("UTF-8")
    );
    assert!(
        w.file_blame(&page.id, &TraceTarget::Anchor, 0, &AtomicBool::new(true))
            .is_err()
    );
    assert!(
        w.file_history_page(&page.id, 0, &AtomicBool::new(true))
            .is_err()
    );
    fs::rename(dir.path().join(".git"), dir.path().join("old-git")).unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    assert!(w.file_history_page(&page.id, 0, &CANCEL).is_err());
    assert!(
        w.file_blame(&page.id, &TraceTarget::Anchor, 0, &CANCEL)
            .is_err()
    );
}
#[cfg(unix)]
#[test]
fn porcelain_quoted_names_tabs_newlines_and_literal_magic_are_native_paths() {
    let (dir, mut w, id, _, _) = fixture();
    let name = "line\tname\n:(glob)*.txt";
    fs::write(dir.path().join(name), "literal\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "unusual filename"]);
    let file = w
        .file_tree(&id, None, 0, &CANCEL)
        .unwrap()
        .files
        .into_iter()
        .find(|file| file.name == name)
        .unwrap();
    let page = w
        .file_history_open(
            FileHistorySource::Source {
                repository_id: id.clone(),
                file_id: file.id,
            },
            &CANCEL,
        )
        .unwrap();
    let blame = w
        .file_blame(&page.id, &TraceTarget::Anchor, 0, &CANCEL)
        .unwrap();
    assert_eq!(blame.lines[0].original_path, name);
    let focus = w
        .file_history_version(
            &page.id,
            &TraceTarget::Line {
                id: blame.lines[0].id.clone(),
            },
            &CANCEL,
        )
        .unwrap();
    assert_eq!(focus.entry.path, name);
    assert_eq!(
        w.revision_content(&id, &focus.tree.id, &focus.entry.id, "auto", 0, &CANCEL)
            .unwrap()
            .text,
        "literal"
    );
}

#[test]
fn history_pages_stay_pinned_and_shallow_boundaries_are_visible() {
    let (dir, mut w, id, _, _) = fixture();
    for n in 0..103 {
        fs::write(
            dir.path().join("renamed 文件.txt"),
            format!("version {n}\n"),
        )
        .unwrap();
        git(dir.path(), &["commit", "-am", &format!("change {n}")]);
    }
    let src = source(&mut w, &id);
    let page = w.file_history_open(src, &CANCEL).unwrap();
    assert_eq!(page.entries.len(), 100);
    assert_eq!(page.total, 107);
    let last = w.file_history_page(&page.id, 100, &CANCEL).unwrap();
    assert_eq!(last.entries.len(), 7);
    assert_eq!(last.entries[0].commit.subject, "change 2");
    assert_eq!(last.entries[4].commit.subject, "rename");
    let repeated = w.file_history_page(&page.id, 100, &CANCEL).unwrap();
    assert_eq!(last.entries[0].id, repeated.entries[0].id);
    fs::write(
        dir.path().join(".git/shallow"),
        format!("{}\n", page.anchor),
    )
    .unwrap();
    let src = source(&mut w, &id);
    let shallow = w.file_history_open(src, &CANCEL).unwrap();
    assert!(shallow.shallow);
    assert_eq!(shallow.total, 1);
    let blame = w
        .file_blame(&shallow.id, &TraceTarget::Anchor, 0, &CANCEL)
        .unwrap();
    assert!(blame.lines[0].boundary);
}
#[test]
fn bom_missing_final_newline_and_dirty_source_buffers_keep_exact_attribution() {
    let (dir, mut w, id, _, _) = fixture();
    fs::write(
        dir.path().join("renamed 文件.txt"),
        "\u{feff}BOM first\nlast without newline",
    )
    .unwrap();
    git(dir.path(), &["commit", "-am", "BOM and last line"]);
    let selected = source(&mut w, &id);
    let file_id = match &selected {
        FileHistorySource::Source { file_id, .. } => file_id.clone(),
        _ => unreachable!(),
    };
    let opened = w.source_open(&id, &file_id, None, &CANCEL).unwrap();
    w.source_dirty(&opened.document.id, true, 10).unwrap();
    let page = w.file_history_open(selected, &CANCEL).unwrap();
    let blame = w
        .file_blame(&page.id, &TraceTarget::Anchor, 0, &CANCEL)
        .unwrap();
    assert_eq!(blame.total, 2);
    assert_eq!(blame.lines[0].text, "BOM first");
    assert_eq!(blame.lines[1].text, "last without newline");
    assert!(w.has_dirty_sources());
    assert_eq!(
        w.source_open(&id, &file_id, None, &CANCEL)
            .unwrap()
            .document
            .id,
        opened.document.id
    );
    let mut utf16 = vec![255, 254];
    utf16.extend("unicode\n".encode_utf16().flat_map(u16::to_le_bytes));
    fs::write(dir.path().join("renamed 文件.txt"), utf16).unwrap();
    git(dir.path(), &["commit", "-am", "UTF16"]);
    let src = source(&mut w, &id);
    let page = w.file_history_open(src, &CANCEL).unwrap();
    assert!(
        w.file_blame(&page.id, &TraceTarget::Anchor, 0, &CANCEL)
            .is_err()
    );
    assert!(!page.entries.is_empty());
    let focus = w
        .file_history_version(&page.id, &TraceTarget::Anchor, &CANCEL)
        .unwrap();
    assert_eq!(
        w.revision_content(&id, &focus.tree.id, &focus.entry.id, "text", 0, &CANCEL)
            .unwrap()
            .text,
        "unicode"
    );
}
#[test]
fn bare_repositories_support_file_history_and_attribution_without_working_files() {
    let (dir, _w, _id, first, _) = fixture();
    let base = tempfile::tempdir().unwrap();
    git(
        base.path(),
        &["clone", "--bare", dir.path().to_str().unwrap(), "bare"],
    );
    let mut w = Workspace::default();
    let catalog = w
        .scan(
            ScanSettings {
                roots: vec![base.path().join("bare").to_str().unwrap().into()],
                exclusions: vec![],
            },
            &CANCEL,
        )
        .unwrap();
    let id = catalog.repositories[0].id.clone();
    let tree = w.revision_open(&id, &first, &CANCEL).unwrap();
    let file = tree
        .entries
        .iter()
        .find(|file| file.name == "old name.txt")
        .unwrap();
    let page = w
        .file_history_open(
            FileHistorySource::Revision {
                repository_id: id.clone(),
                session: tree.id,
                file_id: file.id.clone(),
            },
            &CANCEL,
        )
        .unwrap();
    assert_eq!(page.total, 1);
    assert_eq!(
        w.file_blame(&page.id, &TraceTarget::Anchor, 0, &CANCEL)
            .unwrap()
            .lines[0]
            .text,
        "root line"
    );
    assert!(!base.path().join("bare/old name.txt").exists());
}
#[test]
fn sha256_commits_and_blame_targets_keep_full_identities() {
    let dir = tempfile::tempdir().unwrap();
    git(
        dir.path(),
        &["init", "--object-format=sha256", "-b", "main"],
    );
    git(dir.path(), &["config", "user.name", "Test"]);
    git(dir.path(), &["config", "user.email", "test@example.com"]);
    git(dir.path(), &["config", "commit.gpgsign", "false"]);
    git(
        dir.path(),
        &[
            "config",
            "core.hooksPath",
            dir.path().join(".git/no-hooks").to_str().unwrap(),
        ],
    );
    fs::write(dir.path().join("renamed 文件.txt"), "sha256 line\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "sha256"]);
    let mut w = Workspace::default();
    let catalog = w
        .scan(
            ScanSettings {
                roots: vec![dir.path().to_str().unwrap().into()],
                exclusions: vec![],
            },
            &CANCEL,
        )
        .unwrap();
    let id = catalog.repositories[0].id.clone();
    let src = source(&mut w, &id);
    let page = w.file_history_open(src, &CANCEL).unwrap();
    assert_eq!(page.anchor.len(), 64);
    let blame = w
        .file_blame(&page.id, &TraceTarget::Anchor, 0, &CANCEL)
        .unwrap();
    assert_eq!(blame.lines[0].commit.len(), 64);
    let focus = w
        .file_history_version(
            &page.id,
            &TraceTarget::Line {
                id: blame.lines[0].id.clone(),
            },
            &CANCEL,
        )
        .unwrap();
    assert_eq!(focus.tree.commit, page.anchor);
}
#[test]
fn changed_shallow_graph_expires_old_attribution_without_touching_working_files() {
    let (dir, mut w, id, _, latest) = fixture();
    let src = source(&mut w, &id);
    let page = w.file_history_open(src, &CANCEL).unwrap();
    fs::write(dir.path().join(".git/shallow"), format!("{latest}\n")).unwrap();
    assert!(
        w.file_history_page(&page.id, 0, &CANCEL)
            .unwrap_err()
            .to_string()
            .contains("graph changed")
    );
    assert!(
        w.file_blame(&page.id, &TraceTarget::Anchor, 0, &CANCEL)
            .is_err()
    );
    assert!(
        w.file_history_version(&page.id, &TraceTarget::Anchor, &CANCEL)
            .is_err()
    );
    let src = source(&mut w, &id);
    let shallow = w.file_history_open(src, &CANCEL).unwrap();
    assert!(shallow.shallow);
    assert_eq!(shallow.total, 1);
    assert_eq!(
        fs::read_to_string(dir.path().join("renamed 文件.txt")).unwrap(),
        "root line\r\nchanged\r\nlatest\n"
    );
}
#[cfg(unix)]
#[test]
fn symbolic_links_have_history_and_target_versions_but_no_fake_line_blame() {
    let (dir, mut w, id, _, _) = fixture();
    std::os::unix::fs::symlink("../outside.txt", dir.path().join("link")).unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "link"]);
    let head = git(dir.path(), &["rev-parse", "HEAD"]);
    let tree = w.revision_open(&id, &head, &CANCEL).unwrap();
    let file = tree
        .entries
        .iter()
        .find(|file| file.name == "link")
        .unwrap();
    let page = w
        .file_history_open(
            FileHistorySource::Revision {
                repository_id: id.clone(),
                session: tree.id,
                file_id: file.id.clone(),
            },
            &CANCEL,
        )
        .unwrap();
    assert_eq!(page.total, 1);
    assert!(
        w.file_blame(&page.id, &TraceTarget::Anchor, 0, &CANCEL)
            .unwrap_err()
            .to_string()
            .contains("regular")
    );
    let focus = w
        .file_history_version(&page.id, &TraceTarget::Anchor, &CANCEL)
        .unwrap();
    assert_eq!(
        w.revision_content(&id, &focus.tree.id, &focus.entry.id, "auto", 0, &CANCEL)
            .unwrap()
            .text,
        "../outside.txt"
    );
    assert!(
        fs::symlink_metadata(dir.path().join("link"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
}
#[test]
fn tracing_many_versions_keeps_the_origin_reader_alive_and_close_releases_history() {
    let (dir, mut w, id, _, _) = fixture();
    for n in 0..5 {
        fs::write(dir.path().join("renamed 文件.txt"), format!("extra {n}\n")).unwrap();
        git(dir.path(), &["commit", "-am", &format!("extra {n}")]);
    }
    let head = git(dir.path(), &["rev-parse", "HEAD"]);
    let origin = w.revision_open(&id, &head, &CANCEL).unwrap();
    let file = origin
        .entries
        .iter()
        .find(|file| file.name == "renamed 文件.txt")
        .unwrap();
    let file_id = file.id.clone();
    assert_eq!(w.revision_open(&id, &head, &CANCEL).unwrap().id, origin.id);
    let page = w
        .file_history_open(
            FileHistorySource::Revision {
                repository_id: id.clone(),
                session: origin.id.clone(),
                file_id: file.id.clone(),
            },
            &CANCEL,
        )
        .unwrap();
    for entry in &page.entries {
        w.file_history_version(
            &page.id,
            &TraceTarget::Commit {
                id: entry.id.clone(),
            },
            &CANCEL,
        )
        .unwrap();
    }
    assert_eq!(
        w.revision_content(&id, &origin.id, &file_id, "auto", 0, &CANCEL)
            .unwrap()
            .text,
        "extra 4"
    );
    w.file_history_close(&page.id).unwrap();
    w.file_history_close(&page.id).unwrap();
    assert!(w.file_history_page(&page.id, 0, &CANCEL).is_err());
}
#[test]
fn linked_worktree_history_uses_the_captured_common_directory_and_rejects_redirection() {
    let (dir, _w, _id, _first, latest) = fixture();
    let base = tempfile::tempdir().unwrap();
    let linked = base.path().join("linked");
    git(
        dir.path(),
        &[
            "worktree",
            "add",
            "--detach",
            linked.to_str().unwrap(),
            &latest,
        ],
    );
    let mut w = Workspace::default();
    let catalog = w
        .scan(
            ScanSettings {
                roots: vec![linked.to_str().unwrap().into()],
                exclusions: vec![],
            },
            &CANCEL,
        )
        .unwrap();
    let id = catalog.repositories[0].id.clone();
    let src = source(&mut w, &id);
    let page = w.file_history_open(src, &CANCEL).unwrap();
    assert_eq!(
        w.file_blame(&page.id, &TraceTarget::Anchor, 0, &CANCEL)
            .unwrap()
            .total,
        3
    );
    let other = base.path().join("other");
    fs::create_dir(&other).unwrap();
    git(&other, &["init", "-b", "main"]);
    let git_dir = git(&linked, &["rev-parse", "--absolute-git-dir"]);
    fs::write(
        Path::new(&git_dir).join("commondir"),
        format!("{}\n", other.join(".git").display()),
    )
    .unwrap();
    assert!(w.file_history_page(&page.id, 0, &CANCEL).is_err());
    assert!(
        w.file_blame(&page.id, &TraceTarget::Anchor, 0, &CANCEL)
            .is_err()
    );
    assert_eq!(
        fs::read_to_string(linked.join("renamed 文件.txt")).unwrap(),
        "root line\r\nchanged\r\nlatest\n"
    );
}
