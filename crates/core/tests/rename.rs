use clear_contracts::{CompareRequest, Side, SyncResult};
use clear_core::{compare, editing, native_path, recovery, sync};
use std::{
    ffi::OsStr,
    fs,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
fn fixture() -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    for d in ["l", "r", "b", "recover", "editor"] {
        fs::create_dir(t.path().join(d)).unwrap();
    }
    t
}
fn capture(root: &Path) -> compare::Snapshot {
    compare::compare(
        &CompareRequest {
            left: root.join("l").display().to_string(),
            right: root.join("r").display().to_string(),
            exclusions: "-keep.txt".into(),
            content: true,
        },
        &AtomicBool::new(false),
    )
    .unwrap()
}
fn id(snapshot: &compare::Snapshot, name: &str) -> String {
    snapshot
        .report
        .rows
        .iter()
        .find(|r| r.path == name)
        .unwrap()
        .id
        .clone()
}
fn plan(
    snapshot: &compare::Snapshot,
    root: &Path,
    old: &str,
    new: &str,
    side: Side,
) -> clear_contracts::SyncPlan {
    sync::plan_rename(
        snapshot,
        &id(snapshot, old),
        &root.join("b"),
        &AtomicBool::new(false),
        side,
        OsStr::new(new),
    )
    .unwrap()
}
fn exact(root: &Path, name: &str) -> bool {
    fs::read_dir(root)
        .unwrap()
        .any(|e| e.unwrap().file_name() == OsStr::new(name))
}
fn stage(root: &Path, plan: &clear_contracts::SyncPlan) -> PathBuf {
    root.join(
        native_path::decode(&plan.actions[0].rename_to.as_ref().unwrap().temporary_native).unwrap(),
    )
}
fn execute(snapshot: &compare::Snapshot, plan: &clear_contracts::SyncPlan) -> SyncResult {
    let result = sync::execute(snapshot, plan, &AtomicBool::new(false)).unwrap();
    assert_eq!(result.actions[0].state, "completed", "{result:?}");
    result
}
#[test]
fn rename_is_reviewed_preserves_native_file_and_counterpart_then_rolls_back_without_copying() {
    let t = fixture();
    let root = t.path();
    fs::write(root.join("l/a.txt"), "left").unwrap();
    fs::write(root.join("r/a.txt"), "original").unwrap();
    let before = fs::metadata(root.join("r/a.txt")).unwrap();
    let snapshot = capture(root);
    let p = plan(&snapshot, root, "a.txt", "b.txt", Side::Right);
    assert_eq!(p.version, 3);
    assert!(!Path::new(&p.backup_root).exists());
    assert!(root.join("r/a.txt").exists());
    let result = execute(&snapshot, &p);
    assert!(!root.join("r/a.txt").exists());
    assert_eq!(fs::read(root.join("r/b.txt")).unwrap(), b"original");
    assert_eq!(fs::read(root.join("l/a.txt")).unwrap(), b"left");
    assert!(!stage(&root.join("r"), &p).exists());
    assert_eq!(
        fs::metadata(root.join("r/b.txt"))
            .unwrap()
            .modified()
            .unwrap(),
        before.modified().unwrap()
    );
    assert_eq!(
        fs::read(Path::new(&p.backup_root).join("files/a.txt")).unwrap(),
        b"original"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(
            fs::metadata(root.join("r/b.txt")).unwrap().ino(),
            before.ino()
        );
    }
    let undo = recovery::preview(
        Path::new(&result.journal),
        &root.join("recover"),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(undo.view.actions[0].operation, "rename_back");
    assert_eq!(undo.view.actions[0].path, "b.txt → a.txt");
    let undone = recovery::execute(&undo, &AtomicBool::new(false)).unwrap();
    assert_eq!(undone.actions[0].state, "completed", "{undone:?}");
    assert_eq!(fs::read(root.join("r/a.txt")).unwrap(), b"original");
    assert!(!root.join("r/b.txt").exists());
    assert_eq!(
        fs::metadata(root.join("r/a.txt"))
            .unwrap()
            .modified()
            .unwrap(),
        before.modified().unwrap()
    );
}
#[test]
fn case_only_rename_and_rollback_use_exact_names_on_insensitive_or_sensitive_filesystems() {
    let t = fixture();
    let root = t.path();
    fs::write(root.join("r/Case.TXT"), "case").unwrap();
    let snapshot = capture(root);
    let p = plan(&snapshot, root, "Case.TXT", "case.txt", Side::Right);
    let result = execute(&snapshot, &p);
    assert!(!exact(&root.join("r"), "Case.TXT"));
    assert!(exact(&root.join("r"), "case.txt"));
    let undo = recovery::preview(
        Path::new(&result.journal),
        &root.join("recover"),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(
        recovery::execute(&undo, &AtomicBool::new(false))
            .unwrap()
            .actions[0]
            .state,
        "completed"
    );
    assert!(exact(&root.join("r"), "Case.TXT"));
    assert!(!exact(&root.join("r"), "case.txt"));
}
#[test]
fn existing_names_and_links_are_never_overwritten_including_hardlinks() {
    let t = fixture();
    let root = t.path();
    fs::write(root.join("r/a"), "a").unwrap();
    fs::write(root.join("r/b"), "b").unwrap();
    let snapshot = capture(root);
    assert!(
        sync::plan_rename(
            &snapshot,
            &id(&snapshot, "a"),
            &root.join("b"),
            &AtomicBool::new(false),
            Side::Right,
            OsStr::new("b")
        )
        .is_err()
    );
    fs::remove_file(root.join("r/b")).unwrap();
    fs::hard_link(root.join("r/a"), root.join("r/b")).unwrap();
    assert!(
        sync::plan_rename(
            &snapshot,
            &id(&snapshot, "a"),
            &root.join("b"),
            &AtomicBool::new(false),
            Side::Right,
            OsStr::new("b")
        )
        .is_err()
    );
    fs::remove_file(root.join("r/b")).unwrap();
    let p = plan(&snapshot, root, "a", "b", Side::Right);
    fs::write(root.join("r/b"), "external").unwrap();
    let result = sync::execute(&snapshot, &p, &AtomicBool::new(false)).unwrap();
    assert_eq!(result.actions[0].state, "failed");
    assert_eq!(fs::read(root.join("r/a")).unwrap(), b"a");
    assert_eq!(fs::read(root.join("r/b")).unwrap(), b"external");
}
#[test]
fn cancelled_or_stale_renames_do_not_move_the_source() {
    let t = fixture();
    let root = t.path();
    fs::write(root.join("r/a"), "original").unwrap();
    let snapshot = capture(root);
    let p = plan(&snapshot, root, "a", "b", Side::Right);
    let result = sync::execute(&snapshot, &p, &AtomicBool::new(true)).unwrap();
    assert_eq!(result.actions[0].state, "cancelled");
    assert!(root.join("r/a").exists());
    assert!(!root.join("r/b").exists());
    let p = plan(&snapshot, root, "a", "b", Side::Right);
    fs::write(root.join("r/a"), "external").unwrap();
    let result = sync::execute(&snapshot, &p, &AtomicBool::new(false)).unwrap();
    assert_eq!(result.actions[0].state, "failed");
    assert_eq!(fs::read(root.join("r/a")).unwrap(), b"external");
    assert!(!root.join("r/b").exists());
}
#[test]
fn recovery_reconciles_staged_interruption_including_an_interrupted_prior_rollback() {
    for state in ["running", "failed", "completed"] {
        let t = fixture();
        let root = t.path();
        fs::write(root.join("r/a"), "original").unwrap();
        let snapshot = capture(root);
        let p = plan(&snapshot, root, "a", "b", Side::Right);
        let mut result = execute(&snapshot, &p);
        fs::rename(root.join("r/b"), stage(&root.join("r"), &p)).unwrap();
        result.actions[0].state = state.into();
        fs::write(&result.journal, serde_json::to_vec(&result).unwrap()).unwrap();
        let undo = recovery::preview(
            Path::new(&result.journal),
            &root.join("recover"),
            &AtomicBool::new(false),
        )
        .unwrap();
        let out = recovery::execute(&undo, &AtomicBool::new(false)).unwrap();
        assert_eq!(out.actions[0].state, "completed", "{out:?}");
        assert_eq!(fs::read(root.join("r/a")).unwrap(), b"original");
        assert!(!stage(&root.join("r"), &p).exists());
    }
}
#[test]
fn untouched_running_actions_need_no_backup_and_foreign_temporary_names_are_preserved() {
    let t = fixture();
    let root = t.path();
    fs::write(root.join("r/a"), "original").unwrap();
    let snapshot = capture(root);
    let p = plan(&snapshot, root, "a", "b", Side::Right);
    fs::write(stage(&root.join("r"), &p), "foreign").unwrap();
    let mut result = sync::execute(&snapshot, &p, &AtomicBool::new(false)).unwrap();
    assert_eq!(result.actions[0].state, "failed");
    assert_eq!(fs::read(stage(&root.join("r"), &p)).unwrap(), b"foreign");
    assert_eq!(fs::read(root.join("r/a")).unwrap(), b"original");
    result.actions[0].state = "running".into();
    fs::write(&result.journal, serde_json::to_vec(&result).unwrap()).unwrap();
    assert!(
        recovery::preview(
            Path::new(&result.journal),
            &root.join("recover"),
            &AtomicBool::new(false)
        )
        .is_err()
    );
    assert_eq!(fs::read(stage(&root.join("r"), &p)).unwrap(), b"foreign");
}
#[test]
fn recreated_original_paths_and_changed_destination_or_backup_block_rollback() {
    let t = fixture();
    let root = t.path();
    fs::write(root.join("r/a"), "original").unwrap();
    let snapshot = capture(root);
    let result = execute(&snapshot, &plan(&snapshot, root, "a", "b", Side::Right));
    let undo = recovery::preview(
        Path::new(&result.journal),
        &root.join("recover"),
        &AtomicBool::new(false),
    )
    .unwrap();
    fs::write(root.join("r/a"), "external").unwrap();
    assert!(
        recovery::preview(
            Path::new(&result.journal),
            &root.join("recover"),
            &AtomicBool::new(false)
        )
        .is_err()
    );
    assert_eq!(
        recovery::execute(&undo, &AtomicBool::new(false))
            .unwrap()
            .actions[0]
            .state,
        "failed"
    );
    assert_eq!(fs::read(root.join("r/a")).unwrap(), b"external");
    assert_eq!(fs::read(root.join("r/b")).unwrap(), b"original");
    fs::remove_file(root.join("r/a")).unwrap();
    fs::write(root.join("r/b"), "edited").unwrap();
    assert!(
        recovery::preview(
            Path::new(&result.journal),
            &root.join("recover"),
            &AtomicBool::new(false)
        )
        .is_err()
    );
    fs::write(root.join("r/b"), "original").unwrap();
    fs::write(
        Path::new(&result.plan.backup_root).join("files/a"),
        "corrupt",
    )
    .unwrap();
    assert!(
        recovery::preview(
            Path::new(&result.journal),
            &root.join("recover"),
            &AtomicBool::new(false)
        )
        .is_err()
    );
}
#[test]
fn invalid_new_names_exclusions_and_forged_staging_paths_are_rejected_before_mutation() {
    let t = fixture();
    let root = t.path();
    fs::write(root.join("r/a"), "original").unwrap();
    let snapshot = capture(root);
    for name in [
        "", ".", "..", "../b", "dir/b", "b/", "a", "keep.txt", "a\0b",
    ] {
        assert!(
            sync::plan_rename(
                &snapshot,
                &id(&snapshot, "a"),
                &root.join("b"),
                &AtomicBool::new(false),
                Side::Right,
                OsStr::new(name)
            )
            .is_err(),
            "{name:?}"
        );
    }
    let mut p = plan(&snapshot, root, "a", "b", Side::Right);
    p.actions[0].rename_to.as_mut().unwrap().temporary_native =
        native_path::encode(Path::new("foreign"));
    assert!(sync::execute(&snapshot, &p, &AtomicBool::new(false)).is_err());
    assert!(!Path::new(&p.backup_root).exists());
    assert_eq!(fs::read(root.join("r/a")).unwrap(), b"original");
}
#[test]
fn editor_location_tracks_rename_and_rollback_without_changing_id_or_saved_buffer() {
    let t = fixture();
    let root = t.path();
    fs::write(root.join("r/a"), "original").unwrap();
    let snapshot = capture(root);
    let document = editing::open(&snapshot, &id(&snapshot, "a"), Side::Right).unwrap();
    let result = execute(&snapshot, &plan(&snapshot, root, "a", "b", Side::Right));
    let changes = sync::relocations(&snapshot, &result).unwrap();
    let relocated = document.relocated(&changes[0]).unwrap();
    assert_eq!(relocated.view.id, document.view.id);
    assert_eq!(relocated.view.text, document.view.text);
    assert_eq!(relocated.view.digest, document.view.digest);
    assert_eq!(relocated.view.path, "b");
    assert!(relocated.location_valid());
    let undo = recovery::preview(
        Path::new(&result.journal),
        &root.join("recover"),
        &AtomicBool::new(false),
    )
    .unwrap();
    let out = recovery::execute(&undo, &AtomicBool::new(false)).unwrap();
    let changes = undo.relocations(&out);
    let restored = relocated.relocated(&changes[0]).unwrap();
    assert_eq!(restored.view.id, document.view.id);
    assert_eq!(restored.view.path, "a");
    assert!(restored.location_valid());
}
#[test]
fn last_save_backup_remains_bound_after_native_rename_and_unsaved_text_can_be_saved_at_new_name() {
    let t = fixture();
    let root = t.path();
    fs::write(root.join("r/a"), "original").unwrap();
    let snapshot = capture(root);
    let document = editing::open(&snapshot, &id(&snapshot, "a"), Side::Right).unwrap();
    let (saved, save) = editing::save(&document, "saved", &root.join("editor")).unwrap();
    let snapshot = capture(root);
    let result = execute(&snapshot, &plan(&snapshot, root, "a", "b", Side::Right));
    let relocated = saved
        .relocated(&sync::relocations(&snapshot, &result).unwrap()[0])
        .unwrap();
    let restore = editing::preview_restore(&relocated, Path::new(&save.backup)).unwrap();
    assert_eq!(
        restore.view.target,
        fs::canonicalize(root.join("r/b"))
            .unwrap()
            .display()
            .to_string()
    );
    let (restored, _) = editing::apply_restore(&restore, &root.join("editor")).unwrap();
    assert_eq!(fs::read(root.join("r/b")).unwrap(), b"original");
    assert!(!root.join("r/a").exists());
    let (_, saved) = editing::save(&restored, "unsaved buffer", &root.join("editor")).unwrap();
    assert_eq!(saved.document.path, "b");
    assert_eq!(fs::read(root.join("r/b")).unwrap(), b"unsaved buffer");
    assert!(!root.join("r/a").exists());
}

#[test]
fn left_side_unicode_rename_tracks_the_editor_and_restores_the_original_name() {
    let t = fixture();
    let root = t.path();
    fs::create_dir(root.join("l/sub")).unwrap();
    fs::write(root.join("l/sub/original"), "unicode").unwrap();
    let snapshot = capture(root);
    let document = editing::open(&snapshot, &id(&snapshot, "sub/original"), Side::Left).unwrap();
    let p = plan(&snapshot, root, "sub/original", "caf\u{e9}.txt", Side::Left);
    let result = execute(&snapshot, &p);
    assert_eq!(
        fs::read(root.join("l/sub/caf\u{e9}.txt")).unwrap(),
        b"unicode"
    );
    let relocated = document
        .relocated(&sync::relocations(&snapshot, &result).unwrap()[0])
        .unwrap();
    assert!(relocated.location_valid());
    let undo = recovery::preview(
        Path::new(&result.journal),
        &root.join("recover"),
        &AtomicBool::new(false),
    )
    .unwrap();
    let out = recovery::execute(&undo, &AtomicBool::new(false)).unwrap();
    assert_eq!(out.actions[0].state, "completed", "{out:?}");
    assert_eq!(fs::read(root.join("l/sub/original")).unwrap(), b"unicode");
    assert!(!root.join("l/sub/caf\u{e9}.txt").exists());
}
#[test]
fn external_case_change_cannot_be_silently_reversed_by_saving_an_old_editor_path() {
    let t = fixture();
    let root = t.path();
    fs::write(root.join("r/name.txt"), "original").unwrap();
    let snapshot = capture(root);
    let document = editing::open(&snapshot, &id(&snapshot, "name.txt"), Side::Right).unwrap();
    fs::rename(root.join("r/name.txt"), root.join("r/NAME.txt")).unwrap();
    assert!(editing::save(&document, "new", &root.join("editor")).is_err());
    assert!(exact(&root.join("r"), "NAME.txt"));
    assert!(!exact(&root.join("r"), "name.txt"));
    assert_eq!(fs::read(root.join("r/NAME.txt")).unwrap(), b"original");
}
#[cfg(unix)]
#[test]
fn symlink_substitution_is_rejected_and_native_permission_bits_survive_round_trip() {
    use std::os::unix::fs::PermissionsExt;
    let t = fixture();
    let root = t.path();
    fs::write(root.join("r/a"), "original").unwrap();
    fs::set_permissions(root.join("r/a"), fs::Permissions::from_mode(0o750)).unwrap();
    let snapshot = capture(root);
    let p = plan(&snapshot, root, "a", "b", Side::Right);
    fs::write(root.join("outside"), "outside").unwrap();
    std::os::unix::fs::symlink(root.join("outside"), root.join("r/b")).unwrap();
    assert_eq!(
        sync::execute(&snapshot, &p, &AtomicBool::new(false))
            .unwrap()
            .actions[0]
            .state,
        "failed"
    );
    assert_eq!(fs::read(root.join("outside")).unwrap(), b"outside");
    fs::remove_file(root.join("r/b")).unwrap();
    let result = execute(&snapshot, &plan(&snapshot, root, "a", "b", Side::Right));
    assert_eq!(
        fs::metadata(root.join("r/b")).unwrap().permissions().mode() & 0o7777,
        0o750
    );
    let undo = recovery::preview(
        Path::new(&result.journal),
        &root.join("recover"),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(
        recovery::execute(&undo, &AtomicBool::new(false))
            .unwrap()
            .actions[0]
            .state,
        "completed"
    );
    assert_eq!(
        fs::metadata(root.join("r/a")).unwrap().permissions().mode() & 0o7777,
        0o750
    );
}

#[test]
fn staged_source_can_be_restored_while_a_foreign_destination_is_left_untouched() {
    let t = fixture();
    let root = t.path();
    fs::write(root.join("r/a"), "original").unwrap();
    let snapshot = capture(root);
    let p = plan(&snapshot, root, "a", "b", Side::Right);
    let mut result = execute(&snapshot, &p);
    fs::rename(root.join("r/b"), stage(&root.join("r"), &p)).unwrap();
    fs::write(root.join("r/b"), "foreign").unwrap();
    result.actions[0].state = "failed".into();
    fs::write(&result.journal, serde_json::to_vec(&result).unwrap()).unwrap();
    let undo = recovery::preview(
        Path::new(&result.journal),
        &root.join("recover"),
        &AtomicBool::new(false),
    )
    .unwrap();
    let out = recovery::execute(&undo, &AtomicBool::new(false)).unwrap();
    assert_eq!(out.actions[0].state, "completed", "{out:?}");
    assert_eq!(fs::read(root.join("r/a")).unwrap(), b"original");
    assert_eq!(fs::read(root.join("r/b")).unwrap(), b"foreign");
}
#[cfg(target_os = "linux")]
#[test]
fn native_non_unicode_names_are_renamed_and_recovered_without_using_display_labels() {
    use std::os::unix::ffi::OsStringExt;
    let t = fixture();
    let root = t.path();
    let old = std::ffi::OsString::from_vec(vec![b'a', 255]);
    let new = std::ffi::OsString::from_vec(vec![b'b', 254]);
    fs::write(root.join("r").join(&old), "native").unwrap();
    let snapshot = capture(root);
    let row = snapshot
        .paths
        .iter()
        .find(|(_, p)| p.as_os_str() == old)
        .unwrap()
        .0
        .clone();
    let p = sync::plan_rename(
        &snapshot,
        &row,
        &root.join("b"),
        &AtomicBool::new(false),
        Side::Right,
        &new,
    )
    .unwrap();
    let result = execute(&snapshot, &p);
    assert_eq!(fs::read(root.join("r").join(&new)).unwrap(), b"native");
    let undo = recovery::preview(
        Path::new(&result.journal),
        &root.join("recover"),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(
        recovery::execute(&undo, &AtomicBool::new(false))
            .unwrap()
            .actions[0]
            .state,
        "completed"
    );
    assert_eq!(fs::read(root.join("r").join(&old)).unwrap(), b"native");
}
