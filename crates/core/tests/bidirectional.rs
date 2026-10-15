use clear_contracts::{CompareRequest, Side};
use clear_core::{bidirectional as bidi, compare, recovery};
use std::{
    fs,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};
fn cancel() -> AtomicBool {
    AtomicBool::new(false)
}
fn fixture() -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    for d in ["l", "r", "b", "undo"] {
        fs::create_dir(t.path().join(d)).unwrap();
    }
    t
}
fn snap(root: &Path) -> Arc<compare::Snapshot> {
    Arc::new(
        compare::compare(
            &CompareRequest {
                left: root.join("l").display().to_string(),
                right: root.join("r").display().to_string(),
                exclusions: "-keep".into(),
                content: true,
            },
            &cancel(),
        )
        .unwrap(),
    )
}
fn both(root: &Path, path: &str, bytes: &[u8]) {
    for side in ["l", "r"] {
        let p = root.join(side).join(path);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, bytes).unwrap();
    }
}
fn initialise(root: &Path) {
    bidi::capture(&snap(root), &root.join("baseline.json"), true, &cancel()).unwrap();
}
#[test]
fn independent_edits_additions_deletions_and_empty_directories_converge_with_original_backups() {
    let t = fixture();
    let r = t.path();
    both(r, "a", b"old a");
    both(r, "b", b"old b");
    both(r, "gone/sub/file", b"old");
    for side in ["l", "r"] {
        fs::create_dir(r.join(side).join("gone/empty")).unwrap();
    }
    initialise(r);
    fs::write(r.join("l/a"), b"left a").unwrap();
    fs::write(r.join("r/b"), b"right b").unwrap();
    fs::remove_dir_all(r.join("l/gone")).unwrap();
    fs::create_dir_all(r.join("r/new/empty")).unwrap();
    fs::write(r.join("r/new/file"), b"new").unwrap();
    let plan = bidi::prepare(snap(r), &r.join("baseline.json"), &r.join("b"), &cancel()).unwrap();
    assert_eq!(plan.view.conflicts, 0);
    assert_eq!(plan.view.blocked, 0);
    assert!(plan.view.actions >= 9);
    let done = bidi::execute(&plan, &cancel()).unwrap();
    assert_eq!(done.phase, "completed", "{done:?}");
    assert!(done.baseline_updated);
    assert_eq!(done.operations.len(), 2);
    for side in ["l", "r"] {
        assert_eq!(fs::read(r.join(side).join("a")).unwrap(), b"left a");
        assert_eq!(fs::read(r.join(side).join("b")).unwrap(), b"right b");
        assert!(!r.join(side).join("gone").exists());
        assert!(r.join(side).join("new/empty").is_dir());
    }
    assert_eq!(
        bidi::inspect(&snap(r), &r.join("baseline.json"), &cancel())
            .unwrap()
            .revision,
        "2"
    );
    for operation in done.operations.iter().rev() {
        let undo =
            recovery::preview(Path::new(&operation.journal), &r.join("undo"), &cancel()).unwrap();
        let out = recovery::execute(&undo, &cancel()).unwrap();
        assert!(
            out.actions.iter().all(|a| a.state == "completed"),
            "{out:?}"
        );
    }
    assert_eq!(fs::read(r.join("r/a")).unwrap(), b"old a");
    assert_eq!(fs::read(r.join("l/b")).unwrap(), b"old b");
    assert!(r.join("r/gone/empty").is_dir());
    assert!(!r.join("l/new").exists());
}
#[test]
fn divergent_edits_and_delete_modify_need_reviewed_winners() {
    let t = fixture();
    let r = t.path();
    both(r, "a", b"old");
    both(r, "b", b"old");
    initialise(r);
    fs::write(r.join("l/a"), b"left").unwrap();
    fs::write(r.join("r/a"), b"right").unwrap();
    fs::remove_file(r.join("l/b")).unwrap();
    fs::write(r.join("r/b"), b"modified").unwrap();
    let mut plan =
        bidi::prepare(snap(r), &r.join("baseline.json"), &r.join("b"), &cancel()).unwrap();
    assert_eq!(plan.view.conflicts, 2);
    assert!(bidi::execute(&plan, &cancel()).is_err());
    let rows = plan.page(0);
    for row in rows {
        if row.state == "conflict" {
            plan.resolve(
                &row.id,
                if row.path == "a" {
                    Side::Left
                } else {
                    Side::Right
                },
                &cancel(),
            )
            .unwrap();
        }
    }
    let done = bidi::execute(&plan, &cancel()).unwrap();
    assert_eq!(done.phase, "completed", "{done:?}");
    assert_eq!(fs::read(r.join("r/a")).unwrap(), b"left");
    assert_eq!(fs::read(r.join("l/b")).unwrap(), b"modified");
}
#[test]
fn directory_delete_modify_is_one_branch_conflict_and_preserving_it_restores_empty_children() {
    let t = fixture();
    let r = t.path();
    both(r, "tree/a", b"old");
    for side in ["l", "r"] {
        fs::create_dir(r.join(side).join("tree/empty")).unwrap();
    }
    initialise(r);
    fs::remove_dir_all(r.join("l/tree")).unwrap();
    fs::write(r.join("r/tree/a"), b"modified").unwrap();
    let mut plan =
        bidi::prepare(snap(r), &r.join("baseline.json"), &r.join("b"), &cancel()).unwrap();
    assert_eq!(plan.view.conflicts, 1);
    let row = plan
        .page(0)
        .into_iter()
        .find(|r| r.state == "conflict")
        .unwrap();
    assert_eq!(row.path, "tree");
    assert!(row.subtree);
    plan.resolve(&row.id, Side::Right, &cancel()).unwrap();
    let done = bidi::execute(&plan, &cancel()).unwrap();
    assert_eq!(done.phase, "completed", "{done:?}");
    assert!(r.join("l/tree/empty").is_dir());
    assert_eq!(fs::read(r.join("l/tree/a")).unwrap(), b"modified");
}
#[test]
fn excluded_children_prevent_deleting_their_ancestor_but_can_preserve_the_visible_branch() {
    let t = fixture();
    let r = t.path();
    both(r, "tree/a", b"old");
    fs::write(r.join("r/tree/keep"), b"excluded").unwrap();
    initialise(r);
    fs::remove_dir_all(r.join("l/tree")).unwrap();
    let mut plan =
        bidi::prepare(snap(r), &r.join("baseline.json"), &r.join("b"), &cancel()).unwrap();
    assert_eq!(plan.view.conflicts, 1);
    let row = plan
        .page(0)
        .into_iter()
        .find(|r| r.state == "conflict")
        .unwrap();
    assert!(!row.can_left);
    assert!(row.can_right);
    assert!(plan.resolve(&row.id, Side::Left, &cancel()).is_err());
    plan.resolve(&row.id, Side::Right, &cancel()).unwrap();
    let done = bidi::execute(&plan, &cancel()).unwrap();
    assert_eq!(done.phase, "completed", "{done:?}");
    assert_eq!(fs::read(r.join("r/tree/keep")).unwrap(), b"excluded");
    assert!(!r.join("l/tree/keep").exists());
    assert_eq!(fs::read(r.join("l/tree/a")).unwrap(), b"old");
}
#[test]
fn changed_scope_or_baseline_after_preview_prevents_all_target_writes() {
    for baseline in [false, true] {
        let t = fixture();
        let r = t.path();
        both(r, "a", b"old");
        initialise(r);
        fs::write(r.join("l/a"), b"new").unwrap();
        let plan =
            bidi::prepare(snap(r), &r.join("baseline.json"), &r.join("b"), &cancel()).unwrap();
        if baseline {
            let mut bytes = fs::read(r.join("baseline.json")).unwrap();
            bytes.push(b' ');
            fs::write(r.join("baseline.json"), bytes).unwrap();
        } else {
            fs::write(r.join("r/external"), b"new").unwrap();
        }
        assert!(bidi::execute(&plan, &cancel()).is_err());
        assert_eq!(fs::read(r.join("r/a")).unwrap(), b"old");
        assert!(!Path::new(&plan.view.backup_root).exists());
    }
}

#[test]
fn coordinated_rollback_restores_both_original_trees_and_the_exact_previous_baseline() {
    let t = fixture();
    let r = t.path();
    both(r, "a", b"old a");
    both(r, "b", b"old b");
    initialise(r);
    let original = fs::read(r.join("baseline.json")).unwrap();
    fs::write(r.join("l/a"), b"left").unwrap();
    fs::write(r.join("r/b"), b"right").unwrap();
    let plan = bidi::prepare(snap(r), &r.join("baseline.json"), &r.join("b"), &cancel()).unwrap();
    let done = bidi::execute(&plan, &cancel()).unwrap();
    assert!(done.baseline_updated);
    let undo =
        bidi::preview_recovery(Path::new(&done.journal), &r.join("undo"), &cancel()).unwrap();
    assert!(undo.view.restore_baseline);
    assert_eq!(undo.view.operations.len(), 2);
    let out = bidi::execute_recovery(&undo, &cancel()).unwrap();
    assert!(out.error.is_none(), "{out:?}");
    assert!(out.baseline_restored);
    assert_eq!(fs::read(r.join("baseline.json")).unwrap(), original);
    assert_eq!(fs::read(r.join("l/a")).unwrap(), b"left");
    assert_eq!(fs::read(r.join("r/a")).unwrap(), b"old a");
    assert_eq!(fs::read(r.join("l/b")).unwrap(), b"old b");
    assert_eq!(fs::read(r.join("r/b")).unwrap(), b"right");
}
#[test]
fn newer_baseline_revisions_block_rolling_back_an_older_run() {
    let t = fixture();
    let r = t.path();
    both(r, "a", b"old");
    initialise(r);
    fs::write(r.join("l/a"), b"new").unwrap();
    let p = bidi::prepare(snap(r), &r.join("baseline.json"), &r.join("b"), &cancel()).unwrap();
    let done = bidi::execute(&p, &cancel()).unwrap();
    let p = bidi::prepare(snap(r), &r.join("baseline.json"), &r.join("b"), &cancel()).unwrap();
    assert!(bidi::execute(&p, &cancel()).unwrap().baseline_updated);
    assert!(bidi::preview_recovery(Path::new(&done.journal), &r.join("undo"), &cancel()).is_err());
    assert_eq!(fs::read(r.join("r/a")).unwrap(), b"new");
}

#[test]
fn type_replacements_remain_explicit_blockers_and_do_not_drop_children() {
    let t = fixture();
    let r = t.path();
    both(r, "item", b"old");
    initialise(r);
    fs::remove_file(r.join("l/item")).unwrap();
    fs::create_dir(r.join("l/item")).unwrap();
    fs::write(r.join("l/item/child"), b"new child").unwrap();
    let mut plan =
        bidi::prepare(snap(r), &r.join("baseline.json"), &r.join("b"), &cancel()).unwrap();
    assert_eq!(plan.view.blocked, 1);
    assert!(plan.view.rows.iter().any(|r| r.state == "type_conflict"));
    assert!(plan.prefer(Side::Left, &cancel()).is_ok());
    assert!(bidi::execute(&plan, &cancel()).is_err());
    assert_eq!(fs::read(r.join("r/item")).unwrap(), b"old");
    assert_eq!(fs::read(r.join("l/item/child")).unwrap(), b"new child");
}
#[test]
fn initial_baselines_require_equal_content_and_cannot_be_written_inside_compared_roots() {
    let t = fixture();
    let r = t.path();
    fs::write(r.join("l/a"), b"left only").unwrap();
    assert!(bidi::capture(&snap(r), &r.join("initial.ccbase"), true, &cancel()).is_err());
    assert!(!r.join("initial.ccbase").exists());
    assert!(!r.join("initial.ccbase.lock").exists());
    fs::write(r.join("r/a"), b"left only").unwrap();
    assert!(bidi::capture(&snap(r), &r.join("l/unsafe.ccbase"), true, &cancel()).is_err());
    assert!(!r.join("l/unsafe.ccbase").exists());
}
#[test]
fn new_excluded_branches_propagate_the_visible_directory_without_a_spurious_conflict() {
    let t = fixture();
    let r = t.path();
    initialise(r);
    fs::create_dir(r.join("r/new")).unwrap();
    fs::write(r.join("r/new/keep"), b"excluded").unwrap();
    let plan = bidi::prepare(snap(r), &r.join("baseline.json"), &r.join("b"), &cancel()).unwrap();
    assert_eq!(plan.view.conflicts, 0);
    assert_eq!(plan.view.actions, 1);
    let out = bidi::execute(&plan, &cancel()).unwrap();
    assert_eq!(out.phase, "completed", "{out:?}");
    assert!(r.join("l/new").is_dir());
    assert!(!r.join("l/new/keep").exists());
    assert_eq!(fs::read(r.join("r/new/keep")).unwrap(), b"excluded");
}
#[test]
fn conflict_deletion_winner_removes_only_the_reviewed_branch_with_verified_backups() {
    let t = fixture();
    let r = t.path();
    both(r, "tree/file", b"old");
    initialise(r);
    fs::remove_dir_all(r.join("l/tree")).unwrap();
    fs::write(r.join("r/tree/file"), b"changed").unwrap();
    let mut p = bidi::prepare(snap(r), &r.join("baseline.json"), &r.join("b"), &cancel()).unwrap();
    p.prefer(Side::Left, &cancel()).unwrap();
    let out = bidi::execute(&p, &cancel()).unwrap();
    assert_eq!(out.phase, "completed", "{out:?}");
    assert!(!r.join("r/tree").exists());
    let undo = bidi::preview_recovery(Path::new(&out.journal), &r.join("undo"), &cancel()).unwrap();
    assert!(
        bidi::execute_recovery(&undo, &cancel())
            .unwrap()
            .error
            .is_none()
    );
    assert_eq!(fs::read(r.join("r/tree/file")).unwrap(), b"changed");
    assert!(!r.join("l/tree").exists());
}
#[test]
fn bulk_conflict_resolution_and_paging_cover_the_whole_scope() {
    let t = fixture();
    let r = t.path();
    for i in 0..510 {
        both(r, &format!("file-{i:04}"), b"base");
    }
    initialise(r);
    for i in 0..510 {
        fs::write(r.join("l").join(format!("file-{i:04}")), b"left").unwrap();
        fs::write(r.join("r").join(format!("file-{i:04}")), b"right").unwrap();
    }
    let mut p = bidi::prepare(snap(r), &r.join("baseline.json"), &r.join("b"), &cancel()).unwrap();
    assert_eq!(p.view.conflicts, 510);
    assert_eq!(p.page(0).len(), 500);
    assert_eq!(p.page(500).len(), 10);
    let previous = p.view.id.clone();
    p.prefer(Side::Right, &cancel()).unwrap();
    assert_ne!(p.view.id, previous);
    assert_eq!(p.view.conflicts, 0);
    assert_eq!(p.view.actions, 510);
    assert!(p.page(500).iter().all(|r| r.winner == Some(Side::Right)));
}
#[test]
fn replacing_roots_changing_rules_and_lock_contention_reject_stale_baselines() {
    let t = fixture();
    let r = t.path();
    both(r, "a", b"old");
    initialise(r);
    let lock = fs::File::options()
        .read(true)
        .write(true)
        .open(r.join("baseline.json.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    assert!(bidi::prepare(snap(r), &r.join("baseline.json"), &r.join("b"), &cancel()).is_err());
    drop(lock);
    let other = compare::compare(
        &CompareRequest {
            left: r.join("l").display().to_string(),
            right: r.join("r").display().to_string(),
            exclusions: String::new(),
            content: true,
        },
        &cancel(),
    )
    .unwrap();
    assert!(bidi::inspect(&other, &r.join("baseline.json"), &cancel()).is_err());
    fs::rename(r.join("r"), r.join("parked")).unwrap();
    fs::create_dir(r.join("r")).unwrap();
    fs::write(r.join("r/a"), b"old").unwrap();
    assert!(bidi::inspect(&snap(r), &r.join("baseline.json"), &cancel()).is_err());
    assert_eq!(fs::read(r.join("l/a")).unwrap(), b"old");
}

#[cfg(unix)]
#[test]
fn native_non_unicode_entry_and_profile_names_are_never_reconstructed_from_display_strings() {
    use std::{ffi::OsString, os::unix::ffi::OsStringExt};
    let t = fixture();
    let r = t.path();
    let name = OsString::from_vec(vec![b'f', 0xff]);
    if fs::write(r.join("l").join(&name), b"old").is_err() {
        return;
    }
    fs::write(r.join("r").join(&name), b"old").unwrap();
    let profile = r.join(OsString::from_vec(vec![b'p', 0xfe]));
    if fs::write(&profile, b"probe").is_err() {
        return;
    }
    fs::remove_file(&profile).unwrap();
    bidi::capture(&snap(r), &profile, true, &cancel()).unwrap();
    fs::write(r.join("l").join(&name), b"new").unwrap();
    let p = bidi::prepare(snap(r), &profile, &r.join("b"), &cancel()).unwrap();
    let done = bidi::execute(&p, &cancel()).unwrap();
    assert_eq!(done.phase, "completed", "{done:?}");
    assert_eq!(fs::read(r.join("r").join(&name)).unwrap(), b"new");
    let undo =
        bidi::preview_recovery(Path::new(&done.journal), &r.join("undo"), &cancel()).unwrap();
    assert!(
        bidi::execute_recovery(&undo, &cancel())
            .unwrap()
            .error
            .is_none()
    );
    assert_eq!(fs::read(r.join("r").join(&name)).unwrap(), b"old");
}
