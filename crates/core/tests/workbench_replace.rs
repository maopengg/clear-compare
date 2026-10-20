use clear_contracts::{LineEnding, SaveOptions, workbench::*};
use clear_core::workbench::Workspace;
use std::{
    fs,
    path::Path,
    process::Command,
    sync::atomic::{AtomicBool, Ordering},
};
fn git(root: &Path, args: &[&str]) {
    let result = Command::new("git")
        .current_dir(root)
        .args([
            "-c",
            "user.name=Replace Test",
            "-c",
            "user.email=replace@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
}
fn fixture() -> (tempfile::TempDir, Workspace, Vec<String>) {
    let base = tempfile::tempdir().unwrap();
    for name in ["a", "b"] {
        let root = base.path().join(name);
        fs::create_dir(&root).unwrap();
        git(&root, &["init", "-b", "main"]);
        git(
            &root,
            &[
                "config",
                "core.hooksPath",
                root.join(".git/no-hooks").to_str().unwrap(),
            ],
        );
        fs::write(root.join("sample.txt"), "needle value\n").unwrap();
        fs::write(root.join(".gitignore"), "ignored/\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-m", "initial"]);
    }
    let mut workspace = Workspace::default();
    let catalog = workspace
        .scan(
            ScanSettings {
                roots: vec![base.path().to_str().unwrap().into()],
                exclusions: vec![],
            },
            &AtomicBool::new(false),
        )
        .unwrap();
    let ids = catalog
        .repositories
        .iter()
        .map(|repo| repo.id.clone())
        .collect();
    (base, workspace, ids)
}
fn request(ids: &[String], query: &str, replacement: &str) -> SourceReplaceRequest {
    SourceReplaceRequest {
        query: SourceSearch {
            repositories: ids.to_vec(),
            query: query.into(),
            ..SourceSearch::default()
        },
        replacement: replacement.into(),
        line_ending: None,
    }
}
fn selected(preview: &SourceReplacePreview) -> Vec<String> {
    preview
        .files
        .iter()
        .filter(|file| file.issue.is_none())
        .map(|file| file.id.clone())
        .collect()
}
fn backup_bytes(directory: &str) -> Vec<u8> {
    let folder = fs::read_dir(directory)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(
            &fs::read(folder.join("manifest.json")).unwrap()
        )
        .unwrap()["state"],
        "completed"
    );
    fs::read(folder.join("original.bin")).unwrap()
}
#[test]
fn replaces_complete_matching_files_across_repositories_with_verified_backups() {
    let (base, mut workspace, ids) = fixture();
    let cancel = AtomicBool::new(false);
    let original = "needle\r\n".repeat(450);
    fs::write(base.path().join("a/sample.txt"), original.as_bytes()).unwrap();
    let text = "needle 中文\r\n";
    let mut utf16 = vec![0xff, 0xfe];
    for unit in text.encode_utf16() {
        utf16.extend_from_slice(&unit.to_le_bytes());
    }
    fs::write(base.path().join("b/sample.txt"), &utf16).unwrap();
    fs::create_dir(base.path().join("a/ignored")).unwrap();
    fs::write(base.path().join("a/ignored/skip.txt"), "needle").unwrap();
    let preview = workspace
        .source_replace_preview(request(&ids, "needle", "$1 literal"), &cancel)
        .unwrap();
    assert_eq!(preview.files.len(), 2);
    assert_eq!(
        preview.files.iter().map(|file| file.matches).sum::<usize>(),
        451
    );
    assert_eq!(
        fs::read_to_string(base.path().join("a/sample.txt")).unwrap(),
        original
    );
    let patch = workspace
        .source_replace_patch(&preview.id, &preview.files[0].id, 0, &cancel)
        .unwrap();
    assert!(patch.text.contains("+$1 literal"));
    let result = workspace
        .source_replace_apply(
            &preview.id,
            &selected(&preview),
            &base.path().join("recovery"),
            &cancel,
        )
        .unwrap();
    assert_eq!(result.status, "completed");
    assert_eq!(result.outcomes.len(), 2);
    assert_eq!(
        fs::read_to_string(base.path().join("a/sample.txt")).unwrap(),
        "$1 literal\r\n".repeat(450)
    );
    let (saved, encoding, bom) =
        clear_core::editing::decode(&fs::read(base.path().join("b/sample.txt")).unwrap()).unwrap();
    assert_eq!(saved, "$1 literal 中文\r\n");
    assert_eq!(encoding, clear_contracts::Encoding::Utf16Le);
    assert!(bom);
    let backups = result
        .outcomes
        .iter()
        .map(|outcome| backup_bytes(outcome.backup_directory.as_deref().unwrap()))
        .collect::<Vec<_>>();
    assert!(backups.contains(&original.into_bytes()));
    assert!(backups.contains(&utf16));
    assert!(Path::new(&result.journal).is_file());
    assert_eq!(
        fs::read_to_string(base.path().join("a/ignored/skip.txt")).unwrap(),
        "needle"
    );
    assert!(
        workspace
            .source_replace_apply(
                &preview.id,
                &selected(&preview),
                &base.path().join("recovery"),
                &cancel
            )
            .is_err()
    );
}
#[test]
fn regex_groups_zero_width_literal_dollars_and_expansion_limits() {
    let (base, mut workspace, ids) = fixture();
    let cancel = AtomicBool::new(false);
    let mut input = request(&ids, "(?P<word>needle)", "${word}:$1:$$");
    input.query.regex = true;
    let preview = workspace
        .source_replace_preview(input.clone(), &cancel)
        .unwrap();
    let chosen = vec![preview.files[0].id.clone()];
    workspace
        .source_replace_apply(&preview.id, &chosen, &base.path().join("recovery"), &cancel)
        .unwrap();
    assert_eq!(
        fs::read_to_string(base.path().join("a/sample.txt")).unwrap(),
        "needle:needle:$ value\n"
    );
    assert_eq!(
        fs::read_to_string(base.path().join("b/sample.txt")).unwrap(),
        "needle value\n"
    );
    input.replacement = "$missing".into();
    assert!(
        workspace
            .source_replace_preview(input.clone(), &cancel)
            .is_err()
    );
    input.replacement = "${word".into();
    assert!(workspace.source_replace_preview(input, &cancel).is_err());
    let mut zero = request(&ids, "^", "prefix ");
    zero.query.regex = true;
    zero.query.multiline = true;
    let preview = workspace.source_replace_preview(zero, &cancel).unwrap();
    assert!(preview.files.iter().all(|file| file.matches >= 1));
    let mut huge = request(&ids[..1], "(?s).+", &"$0".repeat(200));
    huge.query.regex = true;
    fs::write(base.path().join("a/sample.txt"), "a".repeat(1024 * 1024)).unwrap();
    assert!(
        workspace
            .source_replace_preview(huge, &cancel)
            .unwrap_err()
            .to_string()
            .contains("size limit")
    );
}
#[test]
fn protects_dirty_buffers_and_updates_clean_open_documents() {
    let (base, mut workspace, ids) = fixture();
    let cancel = AtomicBool::new(false);
    let hits = workspace
        .source_search(&request(&ids, "needle", "unused").query, &cancel)
        .unwrap();
    let a = &hits.hits[0];
    let b = &hits.hits[1];
    let dirty = workspace
        .source_open(&a.repository_id, &a.file_id, None, &cancel)
        .unwrap();
    let clean = workspace
        .source_open(&b.repository_id, &b.file_id, None, &cancel)
        .unwrap();
    workspace.source_dirty(&dirty.document.id, true, 1).unwrap();
    let preview = workspace
        .source_replace_preview(request(&ids, "needle", "replaced"), &cancel)
        .unwrap();
    assert_eq!(
        preview
            .files
            .iter()
            .filter(|file| file.issue.as_deref() == Some("unsaved_buffer"))
            .count(),
        1
    );
    let result = workspace
        .source_replace_apply(
            &preview.id,
            &selected(&preview),
            &base.path().join("recovery"),
            &cancel,
        )
        .unwrap();
    assert_eq!(result.updated_documents.len(), 1);
    assert_eq!(result.updated_documents[0].document.id, clean.document.id);
    assert!(workspace.has_dirty_sources());
    assert!(
        result.updated_documents[0]
            .document
            .text
            .starts_with("replaced")
    );
    assert_eq!(
        fs::read_to_string(base.path().join("a/sample.txt")).unwrap(),
        "needle value\n"
    );
    workspace
        .source_save(
            &clean.document.id,
            "a later edit\n",
            &SaveOptions::default(),
            &base.path().join("editor-backups"),
            1,
            &cancel,
        )
        .unwrap();
    assert_eq!(
        fs::read_to_string(base.path().join("b/sample.txt")).unwrap(),
        "a later edit\n"
    );
}
#[test]
fn revalidates_all_selected_targets_rules_and_selection_before_writing() {
    let (base, mut workspace, ids) = fixture();
    let cancel = AtomicBool::new(false);
    fs::write(base.path().join("b/sample.txt"), "needle dirty\n").unwrap();
    let preview = workspace
        .source_replace_preview(request(&ids, "needle", "new"), &cancel)
        .unwrap();
    let path = base.path().join("b/sample.txt");
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    fs::write(&path, "needle other\n").unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(modified))
        .unwrap();
    assert!(
        workspace
            .source_replace_apply(
                &preview.id,
                &selected(&preview),
                &base.path().join("recovery"),
                &cancel
            )
            .is_err()
    );
    assert_eq!(
        fs::read_to_string(base.path().join("a/sample.txt")).unwrap(),
        "needle value\n"
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "needle other\n");
    let preview = workspace
        .source_replace_preview(request(&ids, "needle", "new"), &cancel)
        .unwrap();
    workspace
        .file_settings(
            &ids[0],
            FileSettings {
                exclusions: "sample.txt".into(),
                ..FileSettings::default()
            },
            &cancel,
        )
        .unwrap();
    assert!(
        workspace
            .source_replace_apply(
                &preview.id,
                &selected(&preview),
                &base.path().join("recovery"),
                &cancel
            )
            .is_err()
    );
    let preview = workspace
        .source_replace_preview(request(&ids, "needle", "new"), &cancel)
        .unwrap();
    assert!(
        workspace
            .source_replace_apply(
                &preview.id,
                &["invented-id".into()],
                &base.path().join("recovery"),
                &cancel
            )
            .is_err()
    );
}
#[test]
fn explicit_mixed_line_ending_policy_and_unchanged_content() {
    let (base, mut workspace, ids) = fixture();
    let cancel = AtomicBool::new(false);
    fs::write(base.path().join("a/sample.txt"), "needle\r\nsecond\n").unwrap();
    let preview = workspace
        .source_replace_preview(request(&ids[..1], "needle", "new"), &cancel)
        .unwrap();
    assert_eq!(
        preview.files[0].issue.as_deref(),
        Some("mixed_line_endings")
    );
    let mut input = request(&ids[..1], "needle", "new");
    input.line_ending = Some(LineEnding::CrLf);
    let preview = workspace.source_replace_preview(input, &cancel).unwrap();
    workspace
        .source_replace_apply(
            &preview.id,
            &selected(&preview),
            &base.path().join("recovery"),
            &cancel,
        )
        .unwrap();
    assert_eq!(
        fs::read_to_string(base.path().join("a/sample.txt")).unwrap(),
        "new\r\nsecond\r\n"
    );
    let mut input = request(&ids[..1], "new", "new");
    input.line_ending = Some(LineEnding::CrLf);
    let preview = workspace.source_replace_preview(input, &cancel).unwrap();
    assert_eq!(preview.files[0].issue.as_deref(), Some("unchanged"));
}
#[test]
fn cancellation_and_external_changes_after_first_file_keep_explicit_partial_results() {
    for external_change in [false, true] {
        let (base, mut workspace, ids) = fixture();
        let cancel = AtomicBool::new(false);
        let preview = workspace
            .source_replace_preview(request(&ids, "needle", "new"), &cancel)
            .unwrap();
        let chosen = selected(&preview);
        let result = workspace
            .source_replace_apply_progress(
                &preview.id,
                &chosen,
                &base.path().join("recovery"),
                &cancel,
                |progress| {
                    if progress.processed == 1 {
                        if external_change {
                            fs::write(base.path().join("b/sample.txt"), "external draft\n")
                                .unwrap();
                        } else {
                            cancel.store(true, Ordering::Relaxed);
                        }
                    }
                },
            )
            .unwrap();
        assert_eq!(
            result.status,
            if external_change {
                "partial"
            } else {
                "cancelled"
            }
        );
        assert_eq!(
            fs::read_to_string(base.path().join("a/sample.txt")).unwrap(),
            "new value\n"
        );
        assert_eq!(
            fs::read_to_string(base.path().join("b/sample.txt")).unwrap(),
            if external_change {
                "external draft\n"
            } else {
                "needle value\n"
            }
        );
        assert_eq!(
            backup_bytes(result.outcomes[0].backup_directory.as_deref().unwrap()),
            b"needle value\n"
        );
        let parent = Path::new(&result.journal).parent().unwrap();
        assert!(parent.join("result.json").is_file());
    }
}
#[cfg(unix)]
#[test]
fn symlink_substitution_never_writes_the_link_target() {
    let (base, mut workspace, ids) = fixture();
    let cancel = AtomicBool::new(false);
    let preview = workspace
        .source_replace_preview(request(&ids, "needle", "new"), &cancel)
        .unwrap();
    let outside = base.path().join("outside.txt");
    fs::write(&outside, "outside\n").unwrap();
    fs::remove_file(base.path().join("b/sample.txt")).unwrap();
    std::os::unix::fs::symlink(&outside, base.path().join("b/sample.txt")).unwrap();
    assert!(
        workspace
            .source_replace_apply(
                &preview.id,
                &selected(&preview),
                &base.path().join("recovery"),
                &cancel
            )
            .is_err()
    );
    assert_eq!(fs::read_to_string(outside).unwrap(), "outside\n");
    assert_eq!(
        fs::read_to_string(base.path().join("a/sample.txt")).unwrap(),
        "needle value\n"
    );
}

#[test]
fn replacement_respects_directory_file_and_project_rules_and_multiline_matches() {
    let (base, mut workspace, ids) = fixture();
    let cancel = AtomicBool::new(false);
    let root = base.path().join("a");
    fs::create_dir(root.join("src")).unwrap();
    fs::write(root.join("src/one.txt"), "Needle\r\nVALUE\r\nneedles\r\n").unwrap();
    fs::write(root.join("src/skip.txt"), "Needle\nVALUE\n").unwrap();
    workspace
        .file_settings(
            &ids[0],
            FileSettings {
                exclusions: "src/skip.txt".into(),
                ..FileSettings::default()
            },
            &cancel,
        )
        .unwrap();
    let folder = workspace
        .file_tree(&ids[0], None, 0, &cancel)
        .unwrap()
        .files
        .into_iter()
        .find(|f| f.name == "src")
        .unwrap();
    let mut scoped = request(&ids[..1], r"needle\nvalue", "done");
    scoped.query.directory = Some(folder.id.clone());
    scoped.query.regex = true;
    scoped.query.multiline = true;
    scoped.query.case_sensitive = false;
    scoped.query.whole_word = true;
    scoped.query.include = "*.txt".into();
    let preview = workspace
        .source_replace_preview(scoped.clone(), &cancel)
        .unwrap();
    assert_eq!(preview.files.len(), 1);
    assert_eq!(preview.files[0].path, "src/one.txt");
    assert_eq!(preview.files[0].matches, 1);
    scoped.query.directory = None;
    scoped.query.file = Some(preview.files[0].file_id.clone());
    let preview = workspace.source_replace_preview(scoped, &cancel).unwrap();
    assert_eq!(preview.files.len(), 1);
    workspace
        .source_replace_apply(
            &preview.id,
            &selected(&preview),
            &base.path().join("recovery"),
            &cancel,
        )
        .unwrap();
    assert_eq!(
        fs::read_to_string(root.join("src/one.txt")).unwrap(),
        "done\r\nneedles\r\n"
    );
    assert_eq!(
        fs::read_to_string(root.join("src/skip.txt")).unwrap(),
        "Needle\nVALUE\n"
    );
    assert_eq!(
        fs::read_to_string(base.path().join("b/sample.txt")).unwrap(),
        "needle value\n"
    );
}

#[test]
fn conflicted_and_read_only_files_are_visible_but_never_selectable() {
    let (base, mut workspace, ids) = fixture();
    let cancel = AtomicBool::new(false);
    let root = base.path().join("a");
    git(&root, &["checkout", "-b", "other"]);
    fs::write(root.join("sample.txt"), "needle incoming\n").unwrap();
    git(&root, &["commit", "-am", "incoming"]);
    git(&root, &["checkout", "main"]);
    fs::write(root.join("sample.txt"), "needle current\n").unwrap();
    git(&root, &["commit", "-am", "current"]);
    let merge = Command::new("git")
        .current_dir(&root)
        .args([
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "merge",
            "other",
        ])
        .output()
        .unwrap();
    assert!(!merge.status.success());
    let readonly = base.path().join("b/sample.txt");
    let original_permissions = fs::metadata(&readonly).unwrap().permissions();
    let mut permissions = original_permissions.clone();
    permissions.set_readonly(true);
    fs::set_permissions(&readonly, permissions).unwrap();
    let preview = workspace
        .source_replace_preview(request(&ids, "needle", "new"), &cancel)
        .unwrap();
    assert_eq!(preview.files.len(), 2);
    assert!(
        preview
            .files
            .iter()
            .any(|file| file.issue.as_deref() == Some("git_conflict"))
    );
    assert!(
        preview
            .files
            .iter()
            .any(|file| file.issue.as_deref() == Some("read_only"))
    );
    assert!(selected(&preview).is_empty());
    assert!(
        workspace
            .source_replace_apply(
                &preview.id,
                &[preview.files[0].id.clone()],
                &base.path().join("recovery"),
                &cancel
            )
            .is_err()
    );
    fs::set_permissions(&readonly, original_permissions).unwrap();
    assert!(
        fs::read_to_string(root.join("sample.txt"))
            .unwrap()
            .contains("<<<<<<<")
    );
    assert_eq!(fs::read_to_string(readonly).unwrap(), "needle value\n");
}
