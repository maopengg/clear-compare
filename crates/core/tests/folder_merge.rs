use clear_contracts::{FolderMergeRequest, FolderResolution};
use clear_core::folder_merge;
use std::{fs, path::Path, sync::atomic::AtomicBool};
fn fixture(root: &Path) -> FolderMergeRequest {
    for name in ["b", "l", "r", "out", "journals"] {
        fs::create_dir(root.join(name)).unwrap();
    }
    FolderMergeRequest {
        two_way: false,
        base: root.join("b").to_string_lossy().into(),
        left: root.join("l").to_string_lossy().into(),
        right: root.join("r").to_string_lossy().into(),
        output: root.join("out").to_string_lossy().into(),
        exclusions: String::new(),
    }
}
#[test]
fn independent_text_changes_merge_into_separate_output_and_inputs_are_preserved() {
    let t = tempfile::tempdir().unwrap();
    let request = fixture(t.path());
    for (side, text) in [("b", "a\nb\nc\n"), ("l", "A\nb\nc\n"), ("r", "a\nb\nC\n")] {
        fs::create_dir(t.path().join(side).join("sub")).unwrap();
        fs::write(t.path().join(side).join("sub/a"), text).unwrap();
    }
    fs::write(t.path().join("l/new"), "left addition").unwrap();
    let plan = folder_merge::prepare(
        &request,
        &t.path().join("journals"),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(plan.preview(0).conflicts, 0);
    assert!(
        plan.preview(0)
            .rows
            .iter()
            .any(|r| r.classification == "auto_merged_text")
    );
    let result = folder_merge::execute(&plan, &AtomicBool::new(false)).unwrap();
    assert!(result.actions.iter().all(|a| a.state == "completed"));
    assert_eq!(
        fs::read_to_string(t.path().join("out/sub/a")).unwrap(),
        "A\nb\nC\n"
    );
    assert_eq!(
        fs::read_to_string(t.path().join("l/sub/a")).unwrap(),
        "A\nb\nc\n"
    );
    assert_eq!(
        fs::read_to_string(t.path().join("out/new")).unwrap(),
        "left addition"
    );
}
#[test]
fn directory_deletion_vs_child_edit_conflicts_and_explicit_subtree_choice_resolves() {
    let t = tempfile::tempdir().unwrap();
    let request = fixture(t.path());
    for (side, text) in [("b", "old"), ("r", "edited")] {
        fs::create_dir(t.path().join(side).join("sub")).unwrap();
        fs::write(t.path().join(side).join("sub/a"), text).unwrap();
    }
    let mut plan = folder_merge::prepare(
        &request,
        &t.path().join("journals"),
        &AtomicBool::new(false),
    )
    .unwrap();
    let preview = plan.preview(0);
    assert!(preview.conflicts >= 2);
    assert!(folder_merge::execute(&plan, &AtomicBool::new(false)).is_err());
    assert!(fs::read_dir(t.path().join("out")).unwrap().next().is_none());
    let directory = preview.rows.iter().find(|r| r.path == "sub").unwrap();
    plan.resolve(&directory.id, FolderResolution::Right)
        .unwrap();
    assert_eq!(plan.preview(0).conflicts, 0);
    assert_ne!(plan.preview(0).id, preview.id);
    folder_merge::execute(&plan, &AtomicBool::new(false)).unwrap();
    assert_eq!(
        fs::read_to_string(t.path().join("out/sub/a")).unwrap(),
        "edited"
    );
    assert!(!t.path().join("l/sub").exists());
}
#[test]
fn probable_rename_vs_edit_is_visible_and_source_changes_block_output() {
    let t = tempfile::tempdir().unwrap();
    let request = fixture(t.path());
    fs::write(t.path().join("b/old"), "original").unwrap();
    fs::write(t.path().join("l/renamed"), "original").unwrap();
    fs::write(t.path().join("r/old"), "modified").unwrap();
    let mut plan = folder_merge::prepare(
        &request,
        &t.path().join("journals"),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(
        plan.preview(0)
            .rows
            .iter()
            .filter(|r| r.classification == "rename_modify_conflict")
            .count(),
        2
    );
    for row in plan.preview(0).rows {
        plan.resolve(&row.id, FolderResolution::Right).unwrap();
    }
    fs::write(t.path().join("b/old"), "changed baseline").unwrap();
    assert!(folder_merge::execute(&plan, &AtomicBool::new(false)).is_err());
    assert!(fs::read_dir(t.path().join("out")).unwrap().next().is_none());
}
#[test]
fn changed_existing_output_is_never_overwritten() {
    let t = tempfile::tempdir().unwrap();
    let request = fixture(t.path());
    fs::write(t.path().join("l/a"), "selected").unwrap();
    let plan = folder_merge::prepare(
        &request,
        &t.path().join("journals"),
        &AtomicBool::new(false),
    )
    .unwrap();
    fs::write(t.path().join("out/a"), "external").unwrap();
    assert!(folder_merge::execute(&plan, &AtomicBool::new(false)).is_err());
    assert_eq!(
        fs::read_to_string(t.path().join("out/a")).unwrap(),
        "external"
    );
}

#[test]
fn two_way_merge_combines_independent_additions_and_requires_competing_file_choice() {
    let t = tempfile::tempdir().unwrap();
    let mut request = fixture(t.path());
    request.two_way = true;
    request.base.clear();
    fs::write(t.path().join("l/left"), "left").unwrap();
    fs::write(t.path().join("r/right"), "right").unwrap();
    fs::write(t.path().join("l/competing"), "one").unwrap();
    fs::write(t.path().join("r/competing"), "two").unwrap();
    let mut plan = folder_merge::prepare(
        &request,
        &t.path().join("journals"),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(plan.preview(0).two_way);
    assert_eq!(plan.preview(0).conflicts, 1);
    plan.resolve_path(Path::new("competing"), FolderResolution::Right)
        .unwrap();
    let result = folder_merge::execute(&plan, &AtomicBool::new(false)).unwrap();
    assert!(result.actions.iter().all(|a| a.state == "completed"));
    assert_eq!(
        fs::read_to_string(t.path().join("out/competing")).unwrap(),
        "two"
    );
    assert!(t.path().join("out/left").exists());
    assert!(t.path().join("out/right").exists());
}
