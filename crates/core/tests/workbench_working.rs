use clear_contracts::workbench::*;
use clear_core::workbench::Workspace;
use std::{fs, path::Path, process::Command, sync::atomic::AtomicBool};
const CANCEL: AtomicBool = AtomicBool::new(false);
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
    String::from_utf8(out.stdout).unwrap()
}
fn fixture() -> (tempfile::TempDir, Workspace, String) {
    let d = tempfile::tempdir().unwrap();
    git(d.path(), &["init", "-b", "main"]);
    git(d.path(), &["config", "user.name", "Working Diff"]);
    git(d.path(), &["config", "user.email", "diff@example.com"]);
    git(d.path(), &["config", "commit.gpgsign", "false"]);
    git(d.path(), &["config", "core.autocrlf", "false"]);
    fs::write(
        d.path().join("code.txt"),
        (0..40).map(|i| format!("line-{i}\n")).collect::<String>(),
    )
    .unwrap();
    git(d.path(), &["add", "."]);
    git(d.path(), &["commit", "-m", "initial"]);
    let mut w = Workspace::default();
    let c = w
        .scan(
            ScanSettings {
                roots: vec![d.path().display().to_string()],
                exclusions: vec![],
            },
            &CANCEL,
        )
        .unwrap();
    let id = c.repositories[0].id.clone();
    (d, w, id)
}
fn changed(w: &mut Workspace, id: &str, path: &str) -> String {
    w.open(id, &CANCEL)
        .unwrap()
        .changes
        .into_iter()
        .find(|c| c.path == path)
        .unwrap()
        .id
}
fn selection(
    w: &mut Workspace,
    id: &str,
    diff: &WorkingDiff,
    hunks: Vec<String>,
    lines: Vec<String>,
) -> ActionResult {
    let p = w
        .prepare(
            id,
            &diff.revision,
            Action::StageSelection {
                diff_id: diff.id.clone(),
                hunks,
                lines,
            },
            &CANCEL,
        )
        .unwrap();
    w.apply(&p.id, &CANCEL).unwrap()
}
#[test]
fn hunks_stage_and_unstage_only_selected_changes_and_keep_worktree() {
    let (d, mut w, id) = fixture();
    let original = fs::read_to_string(d.path().join("code.txt")).unwrap();
    let edited = original
        .replace("line-2\n", "edited-2\n")
        .replace("line-24\n", "edited-24\n");
    fs::write(d.path().join("code.txt"), &edited).unwrap();
    let file = changed(&mut w, &id, "code.txt");
    let diff = w
        .working_diff(&id, &file, "worktree", None, 0, &CANCEL)
        .unwrap();
    assert_eq!(diff.hunks.len(), 2);
    assert!(diff.partial);
    let result = selection(&mut w, &id, &diff, vec![diff.hunks[0].id.clone()], vec![]);
    assert_eq!(
        git(d.path(), &["show", ":code.txt"]),
        original.replace("line-2\n", "edited-2\n")
    );
    assert_eq!(
        fs::read_to_string(d.path().join("code.txt")).unwrap(),
        edited
    );
    let backup = result.backup_reference.unwrap();
    assert_eq!(git(d.path(), &["cat-file", "-p", &backup]), original);
    let diff = w
        .working_diff(&id, &file, "index", None, 0, &CANCEL)
        .unwrap();
    selection(&mut w, &id, &diff, vec![diff.hunks[0].id.clone()], vec![]);
    assert_eq!(git(d.path(), &["show", ":code.txt"]), original);
}
#[test]
fn selected_lines_handle_new_files_partial_unstage_and_partial_deletion() {
    let (d, mut w, id) = fixture();
    fs::write(d.path().join("new name.txt"), "one\ntwo\nthree\n").unwrap();
    let file = changed(&mut w, &id, "new name.txt");
    let diff = w
        .working_diff(&id, &file, "worktree", None, 0, &CANCEL)
        .unwrap();
    let line = diff.hunks[0]
        .lines
        .iter()
        .find(|l| l.text == "two")
        .unwrap()
        .id
        .clone()
        .unwrap();
    selection(&mut w, &id, &diff, vec![], vec![line]);
    assert_eq!(git(d.path(), &["show", ":new name.txt"]), "two\n");
    git(d.path(), &["add", "new name.txt"]);
    let diff = w
        .working_diff(&id, &file, "index", None, 0, &CANCEL)
        .unwrap();
    let line = diff.hunks[0]
        .lines
        .iter()
        .find(|l| l.text == "two")
        .unwrap()
        .id
        .clone()
        .unwrap();
    selection(&mut w, &id, &diff, vec![], vec![line]);
    assert_eq!(git(d.path(), &["show", ":new name.txt"]), "one\nthree\n");
    fs::remove_file(d.path().join("code.txt")).unwrap();
    let f = changed(&mut w, &id, "code.txt");
    let diff = w
        .working_diff(&id, &f, "worktree", None, 0, &CANCEL)
        .unwrap();
    let line = diff.hunks[0]
        .lines
        .iter()
        .find(|l| l.text == "line-2")
        .unwrap()
        .id
        .clone()
        .unwrap();
    selection(&mut w, &id, &diff, vec![], vec![line]);
    assert!(!git(d.path(), &["show", ":code.txt"]).contains("line-2\n"));
    assert!(!d.path().join("code.txt").exists());
    git(d.path(), &["update-index", "--force-remove", "code.txt"]);
    let diff = w.working_diff(&id, &f, "index", None, 0, &CANCEL).unwrap();
    let line = diff.hunks[0]
        .lines
        .iter()
        .find(|l| l.text == "line-2")
        .unwrap()
        .id
        .clone()
        .unwrap();
    selection(&mut w, &id, &diff, vec![], vec![line]);
    assert_eq!(git(d.path(), &["show", ":code.txt"]), "line-2\n");
    assert!(!d.path().join("code.txt").exists());
}
#[test]
fn stale_content_and_late_unsaved_buffers_reject_prepared_actions() {
    let (d, mut w, id) = fixture();
    fs::write(d.path().join("code.txt"), "edited\n").unwrap();
    let f = changed(&mut w, &id, "code.txt");
    let diff = w
        .working_diff(&id, &f, "worktree", None, 0, &CANCEL)
        .unwrap();
    let action = Action::StageSelection {
        diff_id: diff.id.clone(),
        hunks: vec![diff.hunks[0].id.clone()],
        lines: vec![],
    };
    let plan = w.prepare(&id, &diff.revision, action, &CANCEL).unwrap();
    fs::write(d.path().join("code.txt"), "external\n").unwrap();
    assert!(w.apply(&plan.id, &CANCEL).is_err());
    assert!(git(d.path(), &["show", ":code.txt"]).starts_with("line-0"));
    let file = w.change_file(&id, &f, &CANCEL).unwrap();
    let document = w.source_open(&id, &file.id, None, &CANCEL).unwrap();
    let state = w.open(&id, &CANCEL).unwrap();
    let plan = w
        .prepare(
            &id,
            &state.revision,
            Action::Stage { files: vec![f] },
            &CANCEL,
        )
        .unwrap();
    w.source_dirty(&document.document.id, true, 1).unwrap();
    assert!(w.apply(&plan.id, &CANCEL).is_err());
    assert!(git(d.path(), &["show", ":code.txt"]).starts_with("line-0"));
}
#[test]
fn pagination_binary_metadata_and_head_views_are_explicit() {
    let (d, mut w, id) = fixture();
    fs::write(
        d.path().join("many.txt"),
        (0..1200).map(|i| format!("{i}\n")).collect::<String>(),
    )
    .unwrap();
    let f = changed(&mut w, &id, "many.txt");
    let first = w
        .working_diff(&id, &f, "worktree", None, 0, &CANCEL)
        .unwrap();
    assert_eq!(first.next_offset, Some(500));
    assert!(
        w.prepare(
            &id,
            &first.revision,
            Action::StageSelection {
                diff_id: first.id.clone(),
                hunks: vec!["foreign-hunk".into()],
                lines: vec![]
            },
            &CANCEL
        )
        .is_err()
    );
    assert!(
        w.prepare(
            &id,
            &first.revision,
            Action::StageSelection {
                diff_id: first.id.clone(),
                hunks: vec![],
                lines: vec!["foreign-line".into()]
            },
            &CANCEL
        )
        .is_err()
    );

    let next = w
        .working_diff(&id, &f, "worktree", Some(&first.id), 500, &CANCEL)
        .unwrap();
    assert_eq!(next.hunks[0].lines[0].after, Some(501));
    assert!(next.hunks[0].continued);
    assert_eq!(first.id, next.id);
    assert!(
        w.working_diff(&id, &f, "worktree", Some("invented"), 0, &CANCEL)
            .is_err()
    );
    fs::write(d.path().join("binary.bin"), [0, 1, 2, 3]).unwrap();
    let f = changed(&mut w, &id, "binary.bin");
    let binary = w
        .working_diff(&id, &f, "worktree", None, 0, &CANCEL)
        .unwrap();
    assert!(!binary.partial);
    assert_eq!(binary.reason.as_deref(), Some("binary"));
    assert!(binary.raw.contains("Binary"));
    let head = w.working_diff(&id, &f, "head", None, 0, &CANCEL).unwrap();
    assert!(!head.partial);
}
#[test]
fn selected_replacement_lines_preserve_crlf_missing_newline_and_other_edits() {
    let (d, mut w, id) = fixture();
    let original = "α\r\nold\r\nuntouched\r\nlast";
    fs::write(d.path().join("code.txt"), original).unwrap();
    git(d.path(), &["add", "code.txt"]);
    git(d.path(), &["commit", "-m", "line endings"]);
    let changed_text = original
        .replace("old", "new")
        .replace("untouched", "other edit");
    fs::write(d.path().join("code.txt"), &changed_text).unwrap();
    let f = changed(&mut w, &id, "code.txt");
    let diff = w
        .working_diff(&id, &f, "worktree", None, 0, &CANCEL)
        .unwrap();
    assert!(
        diff.hunks
            .iter()
            .flat_map(|h| &h.lines)
            .any(|l| l.no_newline)
    );
    let lines = diff
        .hunks
        .iter()
        .flat_map(|h| &h.lines)
        .filter(|l| l.text == "old" || l.text == "new")
        .filter_map(|l| l.id.clone())
        .collect();
    selection(&mut w, &id, &diff, vec![], lines);
    assert_eq!(
        git(d.path(), &["show", ":code.txt"]),
        original.replace("old", "new")
    );
    assert_eq!(
        fs::read_to_string(d.path().join("code.txt")).unwrap(),
        changed_text
    );
    let diff = w.working_diff(&id, &f, "index", None, 0, &CANCEL).unwrap();
    let lines = diff
        .hunks
        .iter()
        .flat_map(|h| &h.lines)
        .filter(|l| l.text == "old" || l.text == "new")
        .filter_map(|l| l.id.clone())
        .collect();
    selection(&mut w, &id, &diff, vec![], lines);
    assert_eq!(git(d.path(), &["show", ":code.txt"]), original);
}
#[test]
fn selected_content_guard_catches_edits_even_with_restored_size_and_mtime() {
    let (d, mut w, id) = fixture();
    let path = d.path().join("code.txt");
    fs::write(&path, "aaaa\n").unwrap();
    let f = changed(&mut w, &id, "code.txt");
    let diff = w
        .working_diff(&id, &f, "worktree", None, 0, &CANCEL)
        .unwrap();
    let plan = w
        .prepare(
            &id,
            &diff.revision,
            Action::StageSelection {
                diff_id: diff.id,
                hunks: vec![diff.hunks[0].id.clone()],
                lines: vec![],
            },
            &CANCEL,
        )
        .unwrap();
    let stamp = fs::metadata(&path).unwrap().modified().unwrap();
    fs::write(&path, "bbbb\n").unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(stamp))
        .unwrap();
    assert!(w.apply(&plan.id, &CANCEL).is_err());
    assert!(git(d.path(), &["show", ":code.txt"]).starts_with("line-0"));
}
#[cfg(unix)]
#[test]
fn literal_paths_missing_final_newlines_and_permission_only_changes_are_distinct() {
    use std::os::unix::fs::PermissionsExt;
    let (d, mut w, id) = fixture();
    let name = ":(glob)*.txt";
    fs::write(d.path().join(name), "no final newline").unwrap();
    fs::write(d.path().join("other.txt"), "untouched").unwrap();
    let f = changed(&mut w, &id, name);
    let diff = w
        .working_diff(&id, &f, "worktree", None, 0, &CANCEL)
        .unwrap();
    selection(&mut w, &id, &diff, vec![diff.hunks[0].id.clone()], vec![]);
    assert_eq!(
        git(d.path(), &["show", &format!(":{name}")]),
        "no final newline"
    );
    assert!(!git(d.path(), &["ls-files"]).contains("other.txt"));
    fs::write(d.path().join("code.txt"), "changed content\n").unwrap();
    fs::set_permissions(d.path().join("code.txt"), fs::Permissions::from_mode(0o755)).unwrap();
    let f = changed(&mut w, &id, "code.txt");
    let diff = w
        .working_diff(&id, &f, "worktree", None, 0, &CANCEL)
        .unwrap();
    selection(&mut w, &id, &diff, vec![diff.hunks[0].id.clone()], vec![]);
    assert!(git(d.path(), &["ls-files", "--stage", "code.txt"]).starts_with("100644"));
    assert_eq!(git(d.path(), &["show", ":code.txt"]), "changed content\n");
}
