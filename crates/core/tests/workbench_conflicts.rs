use clear_contracts::{
    SaveOptions,
    workbench::{Action, ConflictDocument, ScanSettings},
};
use clear_core::workbench::Workspace;
use std::{fs, path::Path, process::Command, sync::atomic::AtomicBool};
const CANCEL: AtomicBool = AtomicBool::new(false);
const NAME: &str = "冲突 with space.txt";
fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().into()
}
fn fixture(
    base: Option<&[u8]>,
    ours: Option<&[u8]>,
    theirs: Option<&[u8]>,
) -> (tempfile::TempDir, Workspace, String, String) {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    for (key, value) in [
        ("user.name", "Conflict Test"),
        ("user.email", "conflict@example.com"),
        ("commit.gpgsign", "false"),
        ("core.autocrlf", "false"),
    ] {
        git(dir.path(), &["config", key, value]);
    }
    fs::write(dir.path().join("untouched.txt"), b"keep\n").unwrap();
    if let Some(base) = base {
        fs::write(dir.path().join(NAME), base).unwrap();
    }
    git(dir.path(), &["add", "."]);
    git(dir.path(), &["commit", "-m", "base"]);
    git(dir.path(), &["branch", "incoming"]);
    for (branch, bytes) in [("main", ours), ("incoming", theirs)] {
        git(dir.path(), &["checkout", branch]);
        if let Some(bytes) = bytes {
            fs::write(dir.path().join(NAME), bytes).unwrap();
        } else if dir.path().join(NAME).exists() {
            fs::remove_file(dir.path().join(NAME)).unwrap();
        }
        git(dir.path(), &["add", "--", NAME]);
        git(dir.path(), &["commit", "-m", branch]);
    }
    git(dir.path(), &["checkout", "main"]);
    let merge = Command::new("git")
        .current_dir(dir.path())
        .args(["merge", "incoming"])
        .output()
        .unwrap();
    assert!(!merge.status.success(), "fixture should conflict");
    let mut w = Workspace::default();
    let catalog = w
        .scan(
            ScanSettings {
                roots: vec![dir.path().display().to_string()],
                exclusions: vec![],
            },
            &CANCEL,
        )
        .unwrap();
    let id = catalog.repositories[0].id.clone();
    let state = w.open(&id, &CANCEL).unwrap();
    let file = state
        .changes
        .iter()
        .find(|c| c.path == NAME && c.conflicted)
        .unwrap()
        .id
        .clone();
    (dir, w, id, file)
}
fn normal() -> (tempfile::TempDir, Workspace, String, String) {
    fixture(Some(b"base\n"), Some(b"ours\n"), Some(b"theirs\n"))
}
fn document(view: ConflictDocument) -> String {
    view.document.unwrap().document.id
}
fn save(w: &mut Workspace, doc: &str, text: &str, backup: &Path) -> clear_contracts::SaveResult {
    w.source_dirty(doc, true, 1).unwrap();
    w.source_save(doc, text, &SaveOptions::default(), backup, 1, &CANCEL)
        .unwrap()
}
#[test]
fn captured_versions_save_and_resolve_only_selected_file_then_continue() {
    let (dir, mut w, id, file) = normal();
    let view = w.conflict_open(&id, &file, &CANCEL).unwrap();
    assert_eq!(view.base.as_ref().unwrap().text.as_deref(), Some("base\n"));
    assert_eq!(
        view.current.as_ref().unwrap().text.as_deref(),
        Some("ours\n")
    );
    assert_eq!(
        view.incoming.as_ref().unwrap().text.as_deref(),
        Some("theirs\n")
    );
    assert_eq!(view.operation.as_deref(), Some("merge"));
    assert_eq!(view.suggestion.as_ref().unwrap().conflicts, 1);
    let before = fs::read(dir.path().join(NAME)).unwrap();
    let doc = document(view);
    fs::write(dir.path().join("untouched.txt"), b"unrelated local edit\n").unwrap();
    let backup = tempfile::tempdir().unwrap();
    let saved = save(&mut w, &doc, "merged\n", backup.path());
    assert_eq!(fs::read(saved.backup).unwrap(), before);
    let state = w.open(&id, &CANCEL).unwrap();
    let plan = w
        .prepare(
            &id,
            &state.revision,
            Action::ResolveConflict {
                document: doc.clone(),
            },
            &CANCEL,
        )
        .unwrap();
    let result = w.apply(&plan.id, &CANCEL).unwrap();
    assert_eq!(result.state.repository.conflicts, 0);
    let recovery = std::path::PathBuf::from(result.recovery_path.unwrap());
    assert!(recovery.join("index.original").is_file());
    assert!(recovery.join("result.json").is_file());
    for stage in 1..=3 {
        assert!(
            !git(
                dir.path(),
                &[
                    "rev-parse",
                    &format!("refs/clear-compare/conflicts/{}/stage-{stage}", plan.id)
                ]
            )
            .is_empty()
        );
    }
    assert_eq!(git(dir.path(), &["show", &format!(":{NAME}")]), "merged");
    assert_eq!(
        fs::read(dir.path().join("untouched.txt")).unwrap(),
        b"unrelated local edit\n"
    );
    assert_eq!(git(dir.path(), &["show", ":untouched.txt"]), "keep");
    // Once resolved the same document becomes a normal source buffer.
    save(&mut w, &doc, "merged again\n", backup.path());
    assert_eq!(git(dir.path(), &["show", &format!(":{NAME}")]), "merged");
}
#[test]
fn dirty_markers_and_external_saved_result_changes_block_resolution() {
    let (dir, mut w, id, file) = normal();
    let doc = document(w.conflict_open(&id, &file, &CANCEL).unwrap());
    let state = w.open(&id, &CANCEL).unwrap();
    assert!(
        w.prepare(
            &id,
            &state.revision,
            Action::ResolveConflict {
                document: doc.clone()
            },
            &CANCEL
        )
        .is_err()
    );
    w.source_dirty(&doc, true, 1).unwrap();
    assert!(
        w.prepare(
            &id,
            &state.revision,
            Action::ResolveConflict {
                document: doc.clone()
            },
            &CANCEL
        )
        .is_err()
    );
    let backup = tempfile::tempdir().unwrap();
    save(&mut w, &doc, "resolved\n", backup.path());
    let state = w.open(&id, &CANCEL).unwrap();
    let plan = w
        .prepare(
            &id,
            &state.revision,
            Action::ResolveConflict { document: doc },
            &CANCEL,
        )
        .unwrap();
    fs::write(dir.path().join(NAME), "external\n").unwrap();
    assert!(w.apply(&plan.id, &CANCEL).is_err());
    assert!(!git(dir.path(), &["ls-files", "--unmerged"]).is_empty());
    assert_eq!(
        fs::read_to_string(dir.path().join(NAME)).unwrap(),
        "external\n"
    );
}
#[test]
fn index_changes_and_abort_block_old_buffer_saves_without_losing_dirty_state() {
    for abort in [false, true] {
        let (dir, mut w, id, file) = normal();
        let doc = document(w.conflict_open(&id, &file, &CANCEL).unwrap());
        w.source_dirty(&doc, true, 1).unwrap();
        if abort {
            git(dir.path(), &["merge", "--abort"]);
        } else {
            git(dir.path(), &["add", "--", NAME]);
        }
        let before = fs::read(dir.path().join(NAME)).unwrap();
        let backup = tempfile::tempdir().unwrap();
        assert!(
            w.source_save(
                &doc,
                "stale result\n",
                &SaveOptions::default(),
                backup.path(),
                1,
                &CANCEL
            )
            .is_err()
        );
        assert!(w.has_dirty_sources());
        assert_eq!(fs::read(dir.path().join(NAME)).unwrap(), before);
        assert!(fs::read_dir(backup.path()).unwrap().next().is_none());
    }
}
#[test]
fn add_add_and_modify_delete_preserve_explicit_stage_absence() {
    for (base, ours, theirs, missing) in [
        (
            None,
            Some(b"ours\n".as_slice()),
            Some(b"theirs\n".as_slice()),
            1,
        ),
        (
            Some(b"base\n".as_slice()),
            Some(b"ours\n".as_slice()),
            None,
            3,
        ),
        (
            Some(b"base\n".as_slice()),
            None,
            Some(b"theirs\n".as_slice()),
            2,
        ),
    ] {
        let (_dir, mut w, id, file) = fixture(base, ours, theirs);
        let view = w.conflict_open(&id, &file, &CANCEL).unwrap();
        assert_eq!(
            [
                view.base.is_none(),
                view.current.is_none(),
                view.incoming.is_none()
            ],
            [missing == 1, missing == 2, missing == 3]
        );
        assert!(view.document.is_some());
    }
}
#[test]
fn binary_conflicts_are_described_without_creating_fake_text_buffers() {
    let (dir, mut w, id, file) = fixture(Some(b"b\0ase"), Some(b"o\0urs"), Some(b"t\0heirs"));
    let before = fs::read(dir.path().join(NAME)).unwrap();
    let view = w.conflict_open(&id, &file, &CANCEL).unwrap();
    assert_eq!(view.issue.as_deref(), Some("binary_or_encoding"));
    assert!(view.document.is_none() && view.suggestion.is_none());
    assert!(view.current.unwrap().text.is_none());
    assert_eq!(fs::read(dir.path().join(NAME)).unwrap(), before);
}
#[test]
fn rescans_preserve_conflict_guards_and_foreign_or_cancelled_requests_do_not_write() {
    let (dir, mut w, id, file) = normal();
    let doc = document(w.conflict_open(&id, &file, &CANCEL).unwrap());
    w.source_dirty(&doc, true, 1).unwrap();
    w.scan(
        ScanSettings {
            roots: vec![dir.path().display().to_string()],
            exclusions: vec![],
        },
        &CANCEL,
    )
    .unwrap();
    assert!(w.has_dirty_sources());
    assert!(w.conflict_open("foreign", &file, &CANCEL).is_err());
    assert!(w.conflict_open(&id, "foreign", &CANCEL).is_err());
    assert!(w.conflict_open(&id, &file, &AtomicBool::new(true)).is_err());
    git(dir.path(), &["add", "--", NAME]);
    let backup = tempfile::tempdir().unwrap();
    assert!(
        w.source_save(
            &doc,
            "stale\n",
            &SaveOptions::default(),
            backup.path(),
            1,
            &CANCEL
        )
        .is_err()
    );
    w.source_close(&doc, true).unwrap();
    assert!(!w.has_dirty_sources());
}

#[test]
fn crlf_result_keeps_encoding_and_failed_staging_retains_saved_data_and_recovery() {
    let (dir, mut w, id, file) = fixture(Some(b"base\r\n"), Some(b"ours\r\n"), Some(b"theirs\r\n"));
    let doc = document(w.conflict_open(&id, &file, &CANCEL).unwrap());
    let backup = tempfile::tempdir().unwrap();
    w.source_dirty(&doc, true, 1).unwrap();
    let options = SaveOptions {
        line_ending: Some(clear_contracts::LineEnding::CrLf),
        ..Default::default()
    };
    w.source_save(&doc, "resolved\n", &options, backup.path(), 1, &CANCEL)
        .unwrap();
    assert_eq!(fs::read(dir.path().join(NAME)).unwrap(), b"resolved\r\n");
    let state = w.open(&id, &CANCEL).unwrap();
    let plan = w
        .prepare(
            &id,
            &state.revision,
            Action::ResolveConflict {
                document: doc.clone(),
            },
            &CANCEL,
        )
        .unwrap();
    fs::write(dir.path().join(".git/index.lock"), b"external owner").unwrap();
    let error = w.apply(&plan.id, &CANCEL).unwrap_err().to_string();
    assert!(error.contains("Conflict recovery"));
    assert_eq!(fs::read(dir.path().join(NAME)).unwrap(), b"resolved\r\n");
    assert_eq!(
        fs::read(dir.path().join(".git/index.lock")).unwrap(),
        b"external owner"
    );
    assert!(!git(dir.path(), &["ls-files", "--unmerged"]).is_empty());
    let recovery = dir
        .path()
        .join(".git/clear-compare-conflict-operations")
        .join(&plan.id);
    let outcome: serde_json::Value =
        serde_json::from_slice(&fs::read(recovery.join("result.json")).unwrap()).unwrap();
    assert_eq!(outcome["success"], false);
    fs::remove_file(dir.path().join(".git/index.lock")).unwrap();
    let state = w.open(&id, &CANCEL).unwrap();
    let plan = w
        .prepare(
            &id,
            &state.revision,
            Action::ResolveConflict { document: doc },
            &CANCEL,
        )
        .unwrap();
    assert_eq!(
        w.apply(&plan.id, &CANCEL)
            .unwrap()
            .state
            .repository
            .conflicts,
        0
    );
}
