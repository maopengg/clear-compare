use clear_contracts::{Encoding, LineEnding, TextOptions, workbench::ScanSettings};
use clear_core::workbench::Workspace;
use std::{fs, path::Path, process::Command, sync::atomic::AtomicBool};
static CANCEL: AtomicBool = AtomicBool::new(false);
fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().into()
}
fn scan(root: &Path) -> (Workspace, String) {
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
    (w, c.repositories[0].id.clone())
}
fn fixture() -> (tempfile::TempDir, Workspace, String, String) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "user.name", "Revision Test"]);
    git(root, &["config", "user.email", "revision@example.com"]);
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
    fs::create_dir(root.join("src")).unwrap();
    fs::write(root.join("src/old name.txt"), "old\r\n共同\r\n").unwrap();
    fs::write(root.join("unchanged.txt"), "untouched\n").unwrap();
    fs::write(root.join("empty.txt"), "").unwrap();
    fs::write(
        root.join("binary.dat"),
        (0..9000).map(|n| (n % 256) as u8).collect::<Vec<_>>(),
    )
    .unwrap();
    let mut utf16 = vec![0xff, 0xfe];
    utf16.extend("中文\r\n".encode_utf16().flat_map(u16::to_le_bytes));
    fs::write(root.join("unicode.txt"), utf16).unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "initial"]);
    let commit = git(root, &["rev-parse", "HEAD"]);
    let (w, id) = scan(root);
    (dir, w, id, commit)
}
#[test]
fn lazy_complete_tree_includes_unchanged_files_and_remains_pinned_after_checkout_changes() {
    let (dir, mut w, id, commit) = fixture();
    let page = w.revision_open(&id, &commit, &CANCEL).unwrap();
    assert!(page.entries.iter().any(|e| e.name == "unchanged.txt"));
    assert!(!page.entries.iter().any(|e| e.path.contains('/')));
    let folder = page.entries.iter().find(|e| e.name == "src").unwrap();
    let folder_id = folder.id.clone();
    let inner = w
        .revision_tree(&id, &page.id, Some(&folder_id), 0, "", &CANCEL)
        .unwrap();
    assert_eq!(inner.path, "src");
    assert!(inner.parent.is_none());
    assert_eq!(inner.entries[0].path, "src/old name.txt");
    fs::write(dir.path().join("src/old name.txt"), "new body\n").unwrap();
    git(dir.path(), &["commit", "-am", "new version"]);
    let value = w
        .revision_content(&id, &page.id, &inner.entries[0].id, "auto", 0, &CANCEL)
        .unwrap();
    assert_eq!(value.text, "old\n共同");
    assert_eq!(value.line_ending, Some(LineEnding::CrLf));
    let reloaded = w
        .revision_tree(&id, &page.id, Some(&folder_id), 0, "", &CANCEL)
        .unwrap();
    assert_eq!(inner.entries[0].id, reloaded.entries[0].id);
    assert_eq!(
        fs::read_to_string(dir.path().join("src/old name.txt")).unwrap(),
        "new body\n"
    );
}
#[test]
fn formats_binary_and_text_pages_preserve_real_content_and_explicit_bounds() {
    let (_dir, mut w, id, commit) = fixture();
    let page = w.revision_open(&id, &commit, &CANCEL).unwrap();
    let file = |name: &str| {
        page.entries
            .iter()
            .find(|e| e.name == name)
            .unwrap()
            .id
            .clone()
    };
    let unicode = w
        .revision_content(&id, &page.id, &file("unicode.txt"), "text", 0, &CANCEL)
        .unwrap();
    assert_eq!(unicode.encoding, Some(Encoding::Utf16Le));
    assert_eq!(unicode.text, "中文");
    let empty = w
        .revision_content(&id, &page.id, &file("empty.txt"), "auto", 0, &CANCEL)
        .unwrap();
    assert_eq!(empty.mode, "text");
    assert_eq!(empty.total, 0);
    let binary = w
        .revision_content(&id, &page.id, &file("binary.dat"), "auto", 0, &CANCEL)
        .unwrap();
    assert_eq!(binary.mode, "hex");
    assert!(binary.text.starts_with("00000000  00 01 02"));
    assert_eq!(binary.next_offset, Some(4096));
    let next = w
        .revision_content(&id, &page.id, &file("binary.dat"), "hex", 4096, &CANCEL)
        .unwrap();
    assert!(next.text.starts_with("00001000"));
    assert!(
        w.revision_content(&id, &page.id, &file("binary.dat"), "hex", 3, &CANCEL)
            .is_err()
    );
    assert!(
        w.revision_content(&id, &page.id, &file("binary.dat"), "text", 0, &CANCEL)
            .is_err()
    );
    assert!(
        w.revision_content(&id, &page.id, &file("unicode.txt"), "text", 10, &CANCEL)
            .is_err()
    );
}
#[test]
fn tree_filter_paging_parent_ids_and_line_limits_are_stable() {
    let (dir, mut w, id, _) = fixture();
    fs::create_dir_all(dir.path().join("src/nested")).unwrap();
    for n in 0..305 {
        fs::write(
            dir.path().join(format!("src/nested/file-{n:03}.txt")),
            "line\n",
        )
        .unwrap();
    }
    fs::write(
        dir.path().join("lines.txt"),
        format!("{}{}\n", "row\n".repeat(600), "x".repeat(9000)),
    )
    .unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "pages"]);
    let commit = git(dir.path(), &["rev-parse", "HEAD"]);
    let tree = w.revision_open(&id, &commit, &CANCEL).unwrap();
    let src = tree.entries.iter().find(|e| e.name == "src").unwrap();
    let src = w
        .revision_tree(&id, &tree.id, Some(&src.id), 0, "", &CANCEL)
        .unwrap();
    let nested = src.entries.iter().find(|e| e.name == "nested").unwrap();
    let page = w
        .revision_tree(&id, &tree.id, Some(&nested.id), 0, "", &CANCEL)
        .unwrap();
    assert_eq!(page.parent, src.directory);
    assert_eq!(page.entries.len(), 300);
    assert_eq!(page.total, 305);
    let last = w
        .revision_tree(&id, &tree.id, Some(&nested.id), 300, "", &CANCEL)
        .unwrap();
    assert_eq!(last.entries.len(), 5);
    assert_eq!(last.entries[0].name, "file-300.txt");
    let filter = w
        .revision_tree(&id, &tree.id, Some(&nested.id), 0, "FILE-30", &CANCEL)
        .unwrap();
    assert_eq!(filter.total, 5);
    assert!(
        w.revision_tree(&id, &tree.id, Some(&nested.id), 6, "FILE-30", &CANCEL)
            .is_err()
    );
    let file = tree.entries.iter().find(|e| e.name == "lines.txt").unwrap();
    let first = w
        .revision_content(&id, &tree.id, &file.id, "text", 0, &CANCEL)
        .unwrap();
    assert_eq!(first.total, 601);
    assert_eq!(first.next_offset, Some(500));
    let last = w
        .revision_content(&id, &tree.id, &file.id, "text", 500, &CANCEL)
        .unwrap();
    assert!(last.long_lines);
    assert_eq!(last.text.lines().last().unwrap().len(), 8192);
}
#[test]
fn local_diff_is_single_file_disk_only_and_source_capability_preserves_unsaved_buffer() {
    let (dir, mut w, id, commit) = fixture();
    let tree = w.revision_open(&id, &commit, &CANCEL).unwrap();
    let folder = tree.entries.iter().find(|e| e.name == "src").unwrap();
    let nested = w
        .revision_tree(&id, &tree.id, Some(&folder.id), 0, "", &CANCEL)
        .unwrap();
    let file = &nested.entries[0];
    fs::write(dir.path().join("src/old name.txt"), "local\r\n共同\r\n").unwrap();
    let diff = w
        .revision_local_diff(&id, &tree.id, &file.id, &TextOptions::default(), &CANCEL)
        .unwrap();
    assert!(!diff.local_missing);
    assert!(diff.diff.lines.iter().any(|l| l.text.contains("old")));
    assert!(diff.diff.lines.iter().any(|l| l.text.contains("local")));
    let open = w
        .source_open(&id, diff.local_file_id.as_deref().unwrap(), None, &CANCEL)
        .unwrap();
    w.source_dirty(&open.document.id, true, 1).unwrap();
    let again = w
        .revision_local_diff(&id, &tree.id, &file.id, &TextOptions::default(), &CANCEL)
        .unwrap();
    let same = w
        .source_open(&id, again.local_file_id.as_deref().unwrap(), None, &CANCEL)
        .unwrap();
    assert_eq!(open.document.id, same.document.id);
    assert!(w.has_dirty_sources());
    fs::remove_file(dir.path().join("src/old name.txt")).unwrap();
    fs::remove_dir(dir.path().join("src")).unwrap();
    let missing = w
        .revision_local_diff(&id, &tree.id, &file.id, &TextOptions::default(), &CANCEL)
        .unwrap();
    assert!(missing.local_missing);
    assert!(missing.local_file_id.is_none());
    assert!(!dir.path().join("src").exists());
}
#[test]
fn foreign_capabilities_cancellation_and_repository_replacement_never_mutate_files() {
    let (dir, mut w, id, commit) = fixture();
    let tree = w.revision_open(&id, &commit, &CANCEL).unwrap();
    let file = tree
        .entries
        .iter()
        .find(|e| e.name == "unchanged.txt")
        .unwrap();
    let cancelled = AtomicBool::new(true);
    assert!(w.revision_open(&id, "HEAD", &CANCEL).is_err());
    assert!(
        w.revision_content(&id, &tree.id, "invented", "auto", 0, &CANCEL)
            .is_err()
    );
    assert!(
        w.revision_tree(&id, &tree.id, Some(&file.id), 0, "", &CANCEL)
            .is_err()
    );
    assert!(
        w.revision_content("foreign", &tree.id, &file.id, "auto", 0, &CANCEL)
            .is_err()
    );
    assert!(
        w.revision_content(&id, &tree.id, &file.id, "auto", 0, &cancelled)
            .is_err()
    );
    assert_eq!(
        w.revision_content(&id, &tree.id, &file.id, "auto", 0, &CANCEL)
            .unwrap()
            .text,
        "untouched"
    );
    fs::rename(dir.path().join(".git"), dir.path().join("old-git")).unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    assert!(
        w.revision_content(&id, &tree.id, &file.id, "auto", 0, &CANCEL)
            .is_err()
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("unchanged.txt")).unwrap(),
        "untouched\n"
    );
}
#[test]
fn large_blob_uses_disk_capture_and_bounded_pages_without_checkout() {
    let (dir, mut w, id, _) = fixture();
    let data = vec![42u8; 17 * 1024 * 1024 + 17];
    fs::write(dir.path().join("large.bin"), &data).unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "large"]);
    let commit = git(dir.path(), &["rev-parse", "HEAD"]);
    fs::remove_file(dir.path().join("large.bin")).unwrap();
    let tree = w.revision_open(&id, &commit, &CANCEL).unwrap();
    let file = tree.entries.iter().find(|e| e.name == "large.bin").unwrap();
    let page = w
        .revision_content(&id, &tree.id, &file.id, "auto", 0, &CANCEL)
        .unwrap();
    assert_eq!(page.mode, "hex");
    assert_eq!(page.total, data.len());
    assert!(page.text.len() < 22000);
    let last = w
        .revision_content(&id, &tree.id, &file.id, "hex", 17 * 1024 * 1024, &CANCEL)
        .unwrap();
    assert!(last.text.starts_with("01100000  2A 2A"));
    assert!(last.next_offset.is_none());
    assert!(!dir.path().join("large.bin").exists());
    assert!(
        w.revision_content(&id, &tree.id, &file.id, "text", 0, &CANCEL)
            .is_err()
    );
}
#[test]
fn bare_repositories_browse_without_inventing_local_files() {
    let (dir, _w, _id, commit) = fixture();
    let base = tempfile::tempdir().unwrap();
    git(
        base.path(),
        &["clone", "--bare", dir.path().to_str().unwrap(), "bare"],
    );
    let (mut w, id) = scan(&base.path().join("bare"));
    let tree = w.revision_open(&id, &commit, &CANCEL).unwrap();
    let file = tree
        .entries
        .iter()
        .find(|e| e.name == "unchanged.txt")
        .unwrap();
    assert_eq!(
        w.revision_content(&id, &tree.id, &file.id, "auto", 0, &CANCEL)
            .unwrap()
            .text,
        "untouched"
    );
    assert!(
        w.revision_local_diff(&id, &tree.id, &file.id, &TextOptions::default(), &CANCEL)
            .is_err()
    );
}
#[cfg(unix)]
#[test]
fn links_and_submodules_are_metadata_not_followed_and_native_names_stay_literal() {
    let (dir, mut w, id, _) = fixture();
    std::os::unix::fs::symlink("../outside.txt", dir.path().join("link")).unwrap();
    fs::write(dir.path().join("src/:(glob)*.txt"), "literal\n").unwrap();
    git(dir.path(), &["add", "."]);
    let commit = git(dir.path(), &["rev-parse", "HEAD"]);
    git(
        dir.path(),
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{commit},module"),
        ],
    );
    git(dir.path(), &["commit", "-m", "link and module"]);
    let commit = git(dir.path(), &["rev-parse", "HEAD"]);
    let tree = w.revision_open(&id, &commit, &CANCEL).unwrap();
    let link = tree.entries.iter().find(|e| e.name == "link").unwrap();
    assert_eq!(
        w.revision_content(&id, &tree.id, &link.id, "auto", 0, &CANCEL)
            .unwrap()
            .text,
        "../outside.txt"
    );
    assert!(
        w.revision_local_diff(&id, &tree.id, &link.id, &TextOptions::default(), &CANCEL)
            .is_err()
    );
    let module = tree.entries.iter().find(|e| e.name == "module").unwrap();
    assert_eq!(module.kind, "submodule");
    assert!(
        w.revision_content(&id, &tree.id, &module.id, "auto", 0, &CANCEL)
            .is_err()
    );
    let src = tree.entries.iter().find(|e| e.name == "src").unwrap();
    let nested = w
        .revision_tree(&id, &tree.id, Some(&src.id), 0, "", &CANCEL)
        .unwrap();
    let literal = nested
        .entries
        .iter()
        .find(|e| e.name == ":(glob)*.txt")
        .unwrap();
    assert_eq!(
        w.revision_content(&id, &tree.id, &literal.id, "auto", 0, &CANCEL)
            .unwrap()
            .text,
        "literal"
    );
}

#[test]
fn replacement_objects_and_textconv_configuration_do_not_change_historical_bytes() {
    let (dir, mut w, id, commit) = fixture();
    fs::write(dir.path().join(".gitattributes"), "*.txt diff=unsafe\n").unwrap();
    git(
        dir.path(),
        &[
            "config",
            "diff.unsafe.textconv",
            "sh -c 'echo ran > sentinel; cat'",
        ],
    );
    fs::write(dir.path().join("replacement.txt"), "replacement object\n").unwrap();
    let old = git(dir.path(), &["rev-parse", "HEAD:unchanged.txt"]);
    let new = git(dir.path(), &["hash-object", "-w", "replacement.txt"]);
    git(dir.path(), &["replace", &old, &new]);
    let tree = w.revision_open(&id, &commit, &CANCEL).unwrap();
    let file = tree
        .entries
        .iter()
        .find(|e| e.name == "unchanged.txt")
        .unwrap();
    assert_eq!(
        w.revision_content(&id, &tree.id, &file.id, "text", 0, &CANCEL)
            .unwrap()
            .text,
        "untouched"
    );
    let compared = w
        .revision_local_diff(&id, &tree.id, &file.id, &TextOptions::default(), &CANCEL)
        .unwrap();
    assert!(compared.diff.lines.iter().all(|line| line.tag == "equal"));
    assert!(!dir.path().join("sentinel").exists());
}

#[test]
fn sha256_object_identities_are_verified_without_assuming_sha1() {
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
    fs::write(dir.path().join("file.txt"), "sha256 bytes\n").unwrap();
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "sha256"]);
    let commit = git(dir.path(), &["rev-parse", "HEAD"]);
    assert_eq!(commit.len(), 64);
    let (mut w, id) = scan(dir.path());
    let tree = w.revision_open(&id, &commit, &CANCEL).unwrap();
    assert_eq!(tree.entries[0].object.len(), 64);
    assert_eq!(
        w.revision_content(&id, &tree.id, &tree.entries[0].id, "auto", 0, &CANCEL)
            .unwrap()
            .text,
        "sha256 bytes"
    );
}
