use clear_contracts::{Encoding, LineEnding, SaveOptions, workbench::*};
use clear_core::workbench::Workspace;
use std::{fs, path::Path, process::Command, sync::atomic::AtomicBool};
const CANCEL: AtomicBool = AtomicBool::new(false);
fn git(root: &Path, args: &[&str]) {
    let o = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
}
fn fixture() -> (tempfile::TempDir, Workspace, String) {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    git(dir.path(), &["config", "user.name", "Files Test"]);
    git(dir.path(), &["config", "user.email", "files@example.com"]);
    git(dir.path(), &["config", "commit.gpgsign", "false"]);
    fs::write(dir.path().join("tracked.txt"), "hello world\n").unwrap();
    git(dir.path(), &["add", "tracked.txt"]);
    git(dir.path(), &["commit", "-m", "initial"]);
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
fn file(w: &mut Workspace, id: &str, name: &str) -> String {
    w.file_tree(id, None, 0, &CANCEL)
        .unwrap()
        .files
        .into_iter()
        .find(|f| f.name == name)
        .unwrap()
        .id
}
#[test]
fn tree_uses_ids_hides_git_metadata_and_retains_project_rules() {
    let (dir, mut w, id) = fixture();
    fs::write(dir.path().join(".gitignore"), "ignored/\nignored.txt\n").unwrap();
    fs::create_dir(dir.path().join("ignored")).unwrap();
    fs::write(dir.path().join("ignored/inside"), "skip").unwrap();
    fs::write(dir.path().join("ignored.txt"), "skip").unwrap();
    fs::write(dir.path().join(".hidden"), "hidden").unwrap();
    let p = w.file_tree(&id, None, 0, &CANCEL).unwrap();
    assert_eq!(
        p.files.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
        vec!["tracked.txt"]
    );
    w.file_settings(
        &id,
        FileSettings {
            exclusions: "tracked.txt".into(),
            show_hidden: true,
            respect_gitignore: false,
        },
        &CANCEL,
    )
    .unwrap();
    let p = w.file_tree(&id, None, 0, &CANCEL).unwrap();
    assert!(p.files.iter().any(|f| f.name == ".hidden"));
    assert!(
        !p.files
            .iter()
            .any(|f| f.name == ".git" || f.name == "tracked.txt")
    );
    let base = tempfile::tempdir().unwrap();
    w.save(base.path()).unwrap();
    let mut reloaded = Workspace::load(base.path()).unwrap();
    assert_eq!(
        reloaded.catalog.file_settings[&id].exclusions,
        "tracked.txt"
    );
    assert!(
        !reloaded
            .file_tree(&id, None, 0, &CANCEL)
            .unwrap()
            .files
            .iter()
            .any(|f| f.name == "tracked.txt")
    );
    assert!(w.file_tree(&id, Some("invented-path"), 0, &CANCEL).is_err());
}
#[test]
fn creating_folders_and_files_never_overwrites_or_escapes_the_selected_parent() {
    let (dir, mut w, id) = fixture();
    let folder = w.source_create(&id, None, "folder", true, &CANCEL).unwrap();
    let created = w
        .source_create(&id, Some(&folder.id), "new.rs", false, &CANCEL)
        .unwrap();
    assert!(dir.path().join("folder/new.rs").is_file());
    let p = w.file_tree(&id, Some(&folder.id), 0, &CANCEL).unwrap();
    assert_eq!(p.files[0].id, created.id);
    let root = w.file_tree(&id, p.parent.as_deref(), 0, &CANCEL).unwrap();
    assert!(root.path.is_empty());
    assert!(
        w.source_create(&id, Some(&folder.id), "new.rs", false, &CANCEL)
            .is_err()
    );
    assert!(
        w.source_create(&id, None, "../escape", false, &CANCEL)
            .is_err()
    );
    assert!(w.source_create(&id, None, ".GIT", true, &CANCEL).is_err());
    assert!(
        w.source_open("foreign", &created.id, None, &CANCEL)
            .is_err()
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(dir.path().join("folder"), dir.path().join("linked")).unwrap();
        let cap = file(&mut w, &id, "linked");
        assert!(w.file_tree(&id, Some(&cap), 0, &CANCEL).is_err());
        assert!(w.source_open(&id, &cap, None, &CANCEL).is_err());
    }
}
#[test]
fn saves_preserve_encodings_back_up_and_reject_external_edits_and_stale_dirty_messages() {
    let (dir, mut w, id) = fixture();
    let cap = file(&mut w, &id, "tracked.txt");
    let doc = w.source_open(&id, &cap, None, &CANCEL).unwrap().document;
    let base = tempfile::tempdir().unwrap();
    w.source_dirty(&doc.id, true, 1).unwrap();
    assert!(w.has_dirty_sources());
    assert!(w.source_close(&doc.id, false).is_err());
    let saved = w
        .source_save(
            &doc.id,
            "你好\nnext\n",
            &SaveOptions {
                encoding: Some(Encoding::Utf16Le),
                line_ending: Some(LineEnding::CrLf),
                bom: Some(true),
            },
            base.path(),
            2,
            &CANCEL,
        )
        .unwrap();
    assert_eq!(saved.document.id, doc.id);
    assert!(
        fs::read(dir.path().join("tracked.txt"))
            .unwrap()
            .starts_with(&[255, 254])
    );
    assert_eq!(fs::read_to_string(saved.backup).unwrap(), "hello world\n");
    w.source_dirty(&doc.id, true, 1).unwrap();
    assert!(!w.has_dirty_sources());
    w.source_dirty(&doc.id, true, 5).unwrap();
    assert!(
        w.source_save(
            &doc.id,
            "stale save",
            &SaveOptions::default(),
            base.path(),
            4,
            &CANCEL
        )
        .is_err()
    );
    w.source_dirty(&doc.id, false, 6).unwrap();
    assert!(!w.source_check(&CANCEL).unwrap()[0].changed);
    fs::write(dir.path().join("tracked.txt"), "external\n").unwrap();
    assert!(w.source_check(&CANCEL).unwrap()[0].changed);
    assert!(
        w.source_save(
            &doc.id,
            "overwrite",
            &SaveOptions::default(),
            base.path(),
            7,
            &CANCEL
        )
        .is_err()
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("tracked.txt")).unwrap(),
        "external\n"
    );
    w.source_close(&doc.id, true).unwrap();
    assert!(w.source_check(&CANCEL).unwrap().is_empty());
}
#[test]
fn search_covers_regex_words_case_multiline_unicode_offsets_and_scopes() {
    let (dir, mut w, id) = fixture();
    fs::write(
        dir.path().join("unicode.txt"),
        "😀你好 needle\r\nnext Needle\r\nneedlework\r\n",
    )
    .unwrap();
    fs::write(dir.path().join(".gitignore"), "ignored.txt\n").unwrap();
    fs::write(dir.path().join("ignored.txt"), "needle\n").unwrap();
    fs::create_dir(dir.path().join("sub")).unwrap();
    fs::write(dir.path().join("sub/code.rs"), "needle\n").unwrap();
    let q = SourceSearch {
        repositories: vec![id.clone()],
        query: "needle".into(),
        whole_word: true,
        ..Default::default()
    };
    let results = w.source_search(&q, &CANCEL).unwrap();
    assert_eq!(results.hits.len(), 3);
    let hit = results
        .hits
        .iter()
        .find(|h| h.path == "unicode.txt")
        .unwrap();
    assert_eq!(
        (hit.line, hit.column, hit.offset_utf16, hit.length_utf16),
        (1, 5, 5, 6)
    );
    let d = w
        .source_open(&id, &hit.file_id, hit.digest.as_deref(), &CANCEL)
        .unwrap();
    assert!(d.document.text.contains("needle"));
    w.source_close(&d.document.id, false).unwrap();
    let q = SourceSearch {
        case_sensitive: true,
        include: "*.txt".into(),
        ..q.clone()
    };
    assert_eq!(w.source_search(&q, &CANCEL).unwrap().hits.len(), 1);
    let q = SourceSearch {
        query: "needle\nnext Needle".into(),
        regex: true,
        multiline: true,
        whole_word: false,
        include: String::new(),
        ..q
    };
    assert_eq!(w.source_search(&q, &CANCEL).unwrap().hits.len(), 1);
    let cap = file(&mut w, &id, "sub");
    let q = SourceSearch {
        directory: Some(cap),
        query: "needle".into(),
        regex: false,
        multiline: false,
        case_sensitive: false,
        ..q
    };
    let r = w.source_search(&q, &CANCEL).unwrap();
    assert_eq!(r.hits.len(), 1);
    assert_eq!(r.hits[0].path, "sub/code.rs");
    assert!(
        w.source_search(
            &SourceSearch {
                regex: true,
                query: "[".into(),
                ..q.clone()
            },
            &CANCEL
        )
        .is_err()
    );
    assert!(
        w.source_search(&q, &AtomicBool::new(true))
            .unwrap()
            .cancelled
    );
}
#[test]
fn cancelled_scans_keep_open_documents_and_stale_search_hits_are_rejected() {
    let (dir, mut w, id) = fixture();
    let q = SourceSearch {
        repositories: vec![id.clone()],
        query: "hello".into(),
        ..Default::default()
    };
    let hit = w.source_search(&q, &CANCEL).unwrap().hits.remove(0);
    let doc = w
        .source_open(&id, &hit.file_id, hit.digest.as_deref(), &CANCEL)
        .unwrap()
        .document;
    w.source_dirty(&doc.id, true, 1).unwrap();
    assert!(
        w.scan(
            ScanSettings {
                roots: vec![dir.path().display().to_string()],
                exclusions: vec![]
            },
            &AtomicBool::new(true)
        )
        .is_err()
    );
    assert!(w.has_dirty_sources());
    assert!(w.source_close(&doc.id, false).is_err());
    w.source_close(&doc.id, true).unwrap();
    fs::write(dir.path().join("tracked.txt"), "changed\n").unwrap();
    assert!(
        w.source_open(&id, &hit.file_id, hit.digest.as_deref(), &CANCEL)
            .is_err()
    );
}

#[test]
fn changing_scan_roots_keeps_open_buffer_capabilities_until_the_last_tab_closes() {
    let (dir, mut w, id) = fixture();
    let cap = file(&mut w, &id, "tracked.txt");
    let doc = w.source_open(&id, &cap, None, &CANCEL).unwrap().document;
    w.source_dirty(&doc.id, true, 1).unwrap();
    assert!(w.remove(&id).is_err());
    w.scan(ScanSettings::default(), &CANCEL).unwrap();
    assert!(w.catalog.repositories.is_empty());
    let backup = tempfile::tempdir().unwrap();
    w.source_save(
        &doc.id,
        "saved after scan change\n",
        &SaveOptions::default(),
        backup.path(),
        2,
        &CANCEL,
    )
    .unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("tracked.txt")).unwrap(),
        "saved after scan change\n"
    );
    w.source_close(&doc.id, false).unwrap();
    assert!(w.source_open(&id, &cap, None, &CANCEL).is_err());
}

#[test]
fn externally_grown_files_are_rejected_and_working_tree_operations_guard_unsaved_buffers() {
    let (dir, mut w, id) = fixture();
    let cap = file(&mut w, &id, "tracked.txt");
    let doc = w.source_open(&id, &cap, None, &CANCEL).unwrap().document;
    w.source_dirty(&doc.id, true, 1).unwrap();
    let state = w.open(&id, &CANCEL).unwrap();
    assert!(
        w.prepare(
            &id,
            &state.revision,
            Action::Switch {
                reference: "HEAD".into(),
                detach: true
            },
            &CANCEL
        )
        .is_err()
    );
    fs::write(
        dir.path().join("tracked.txt"),
        vec![b'x'; 4 * 1024 * 1024 + 1],
    )
    .unwrap();
    assert!(w.source_check(&CANCEL).unwrap()[0].changed);
    let backup = tempfile::tempdir().unwrap();
    assert!(
        w.source_save(
            &doc.id,
            "overwrite",
            &SaveOptions::default(),
            backup.path(),
            2,
            &CANCEL
        )
        .is_err()
    );
    assert_eq!(
        fs::metadata(dir.path().join("tracked.txt")).unwrap().len(),
        4 * 1024 * 1024 + 1
    );
}
