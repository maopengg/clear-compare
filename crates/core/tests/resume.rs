use clear_contracts::{CompareRequest, Side};
use clear_core::{compare, recovery, sync};
use std::{fs, path::Path, sync::atomic::AtomicBool};
fn fixture() -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    for p in ["l", "r", "b", "undo"] {
        fs::create_dir(t.path().join(p)).unwrap();
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
#[test]
fn cancelled_copy_restarts_from_durable_intent_and_original_history_can_roll_back() {
    let t = fixture();
    let root = t.path();
    fs::write(root.join("l/a"), b"new a").unwrap();
    fs::write(root.join("l/b"), b"new b").unwrap();
    fs::write(root.join("r/a"), b"old a").unwrap();
    let s = capture(root);
    let ids = s
        .report
        .rows
        .iter()
        .map(|r| r.id.clone())
        .collect::<Vec<_>>();
    let p = sync::plan(&s, &ids, &root.join("b"), &AtomicBool::new(false)).unwrap();
    let stopped = sync::execute(&s, &p, &AtomicBool::new(true)).unwrap();
    drop(s);
    let view = sync::resume::preview(Path::new(&stopped.journal), &AtomicBool::new(false)).unwrap();
    assert_eq!(view.view.pending, 2);
    assert_eq!(view.view.satisfied, 0);
    let done = sync::resume::execute(&view, &AtomicBool::new(false)).unwrap();
    assert!(
        done.actions.iter().all(|a| a.state == "completed"),
        "{done:?}"
    );
    assert_eq!(fs::read(root.join("r/a")).unwrap(), b"new a");
    assert_eq!(fs::read(root.join("r/b")).unwrap(), b"new b");
    assert_eq!(done.attempts.len(), 1);
    let undo = recovery::preview(
        Path::new(&done.journal),
        &root.join("undo"),
        &AtomicBool::new(false),
    )
    .unwrap();
    let restored = recovery::execute(&undo, &AtomicBool::new(false)).unwrap();
    assert!(
        restored.actions.iter().all(|a| a.state == "completed"),
        "{restored:?}"
    );
    assert_eq!(fs::read(root.join("r/a")).unwrap(), b"old a");
    assert!(!root.join("r/b").exists());
}
#[test]
fn interrupted_mirror_can_reconcile_finished_data_and_finish_directory_actions() {
    let t = fixture();
    let root = t.path();
    fs::create_dir_all(root.join("l/new/empty")).unwrap();
    fs::write(root.join("l/new/file"), b"source").unwrap();
    fs::create_dir_all(root.join("r/old/empty")).unwrap();
    fs::write(root.join("r/old/file"), b"original").unwrap();
    let s = capture(root);
    let p =
        sync::plan_mirror_tree(&s, &root.join("b"), &AtomicBool::new(false), Side::Left).unwrap();
    let mut out = sync::execute(&s, &p, &AtomicBool::new(false)).unwrap();
    assert!(out.actions.iter().all(|a| a.state == "completed"));
    for a in &mut out.actions {
        a.state = "running".into();
    }
    fs::write(&out.journal, serde_json::to_vec_pretty(&out).unwrap()).unwrap();
    drop(s);
    let preview = sync::resume::preview(Path::new(&out.journal), &AtomicBool::new(false)).unwrap();
    assert_eq!(preview.view.pending, 0);
    assert_eq!(preview.view.satisfied, p.actions.len());
    let done = sync::resume::execute(&preview, &AtomicBool::new(false)).unwrap();
    assert!(done.actions.iter().all(|a| a.state == "completed"));
    let undo = recovery::preview(
        Path::new(&done.journal),
        &root.join("undo"),
        &AtomicBool::new(false),
    )
    .unwrap();
    let restored = recovery::execute(&undo, &AtomicBool::new(false)).unwrap();
    assert!(
        restored.actions.iter().all(|a| a.state == "completed"),
        "{restored:?}"
    );
    assert!(root.join("r/old/empty").is_dir());
    assert!(!root.join("r/new").exists());
}

#[test]
fn staged_native_rename_is_finished_with_its_verified_original_backup() {
    let t = fixture();
    let root = t.path();
    fs::write(root.join("r/old"), b"native").unwrap();
    let s = capture(root);
    let row = s.report.rows[0].id.clone();
    let p = sync::plan_rename(
        &s,
        &row,
        &root.join("b"),
        &AtomicBool::new(false),
        Side::Right,
        std::ffi::OsStr::new("new"),
    )
    .unwrap();
    let mut result = sync::execute(&s, &p, &AtomicBool::new(false)).unwrap();
    let stage =
        clear_core::native_path::decode(&p.actions[0].rename_to.as_ref().unwrap().temporary_native)
            .unwrap();
    fs::rename(root.join("r/new"), root.join("r").join(stage)).unwrap();
    result.actions[0].state = "cancelled".into();
    fs::write(&result.journal, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
    let preview =
        sync::resume::preview(Path::new(&result.journal), &AtomicBool::new(false)).unwrap();
    assert_eq!(preview.view.actions[0].state, "finish_staged_rename");
    let done = sync::resume::execute(&preview, &AtomicBool::new(false)).unwrap();
    assert_eq!(done.actions[0].state, "completed");
    assert_eq!(fs::read(root.join("r/new")).unwrap(), b"native");
    let undo = recovery::preview(
        Path::new(&done.journal),
        &root.join("undo"),
        &AtomicBool::new(false),
    )
    .unwrap();
    let restored = recovery::execute(&undo, &AtomicBool::new(false)).unwrap();
    assert_eq!(restored.actions[0].state, "completed");
    assert_eq!(fs::read(root.join("r/old")).unwrap(), b"native");
}
#[test]
fn changed_sources_targets_and_replaced_roots_refuse_forward_restart_without_writes() {
    for change in [
        "source bytes",
        "target bytes",
        "source identity",
        "target identity",
        "root identity",
    ] {
        let t = fixture();
        let root = t.path();
        fs::write(root.join("l/a"), b"source").unwrap();
        fs::write(root.join("r/a"), b"target").unwrap();
        let s = capture(root);
        let p = sync::plan(
            &s,
            &[s.report.rows[0].id.clone()],
            &root.join("b"),
            &AtomicBool::new(false),
        )
        .unwrap();
        let stopped = sync::execute(&s, &p, &AtomicBool::new(true)).unwrap();
        match change {
            "source bytes" => fs::write(root.join("l/a"), b"external").unwrap(),
            "target bytes" => fs::write(root.join("r/a"), b"external").unwrap(),
            "source identity" => {
                fs::rename(root.join("l/a"), root.join("source-parked")).unwrap();
                fs::write(root.join("l/a"), b"source").unwrap();
            }
            "target identity" => {
                fs::rename(root.join("r/a"), root.join("target-parked")).unwrap();
                fs::write(root.join("r/a"), b"target").unwrap();
            }
            _ => {
                fs::rename(root.join("r"), root.join("root-parked")).unwrap();
                fs::create_dir(root.join("r")).unwrap();
                fs::write(root.join("r/a"), b"target").unwrap();
            }
        }
        let before = fs::read(&stopped.journal).unwrap();
        assert!(
            sync::resume::preview(Path::new(&stopped.journal), &AtomicBool::new(false)).is_err(),
            "{change}"
        );
        assert_eq!(fs::read(&stopped.journal).unwrap(), before);
        assert!(!Path::new(&p.backup_root).join("attempts").exists());
    }
}
#[test]
fn immutable_intent_and_journal_changes_expire_reviewed_restart_and_rollback_tokens() {
    let t = fixture();
    let root = t.path();
    fs::write(root.join("l/a"), b"source").unwrap();
    let s = capture(root);
    let p = sync::plan(
        &s,
        &[s.report.rows[0].id.clone()],
        &root.join("b"),
        &AtomicBool::new(false),
    )
    .unwrap();
    let stopped = sync::execute(&s, &p, &AtomicBool::new(true)).unwrap();
    let preview =
        sync::resume::preview(Path::new(&stopped.journal), &AtomicBool::new(false)).unwrap();
    let mut bytes = fs::read(&stopped.journal).unwrap();
    bytes.push(b' ');
    fs::write(&stopped.journal, &bytes).unwrap();
    assert!(sync::resume::execute(&preview, &AtomicBool::new(false)).is_err());
    assert!(!root.join("r/a").exists());
    let preview =
        sync::resume::preview(Path::new(&stopped.journal), &AtomicBool::new(false)).unwrap();
    let done = sync::resume::execute(&preview, &AtomicBool::new(false)).unwrap();
    let undo = recovery::preview(
        Path::new(&done.journal),
        &root.join("undo"),
        &AtomicBool::new(false),
    )
    .unwrap();
    let mut bytes = fs::read(&done.journal).unwrap();
    bytes.push(b' ');
    fs::write(&done.journal, &bytes).unwrap();
    assert!(recovery::execute(&undo, &AtomicBool::new(false)).is_err());
    assert!(root.join("r/a").exists());
}
#[test]
fn operation_lock_excludes_other_forward_and_rollback_coordinators() {
    let t = fixture();
    let root = t.path();
    fs::write(root.join("l/a"), b"source").unwrap();
    let s = capture(root);
    let p = sync::plan(
        &s,
        &[s.report.rows[0].id.clone()],
        &root.join("b"),
        &AtomicBool::new(false),
    )
    .unwrap();
    let stopped = sync::execute(&s, &p, &AtomicBool::new(true)).unwrap();
    let lock = fs::File::options()
        .read(true)
        .write(true)
        .open(Path::new(&p.backup_root).join("operation.lock"))
        .unwrap();
    lock.try_lock().unwrap();
    assert!(sync::resume::preview(Path::new(&stopped.journal), &AtomicBool::new(false)).is_err());
    assert!(
        recovery::preview(
            Path::new(&stopped.journal),
            &root.join("undo"),
            &AtomicBool::new(false)
        )
        .is_err()
    );
    drop(lock);
    assert!(sync::resume::preview(Path::new(&stopped.journal), &AtomicBool::new(false)).is_ok());
}
#[test]
fn pending_rename_and_deletion_restart_in_either_selected_side_without_touching_counterparts() {
    for side in [Side::Left, Side::Right] {
        for rename in [false, true] {
            let t = fixture();
            let root = t.path();
            fs::write(root.join("l/a"), b"left").unwrap();
            fs::write(root.join("r/a"), b"right").unwrap();
            let s = capture(root);
            let id = s.report.rows[0].id.clone();
            let p = if rename {
                sync::plan_rename(
                    &s,
                    &id,
                    &root.join("b"),
                    &AtomicBool::new(false),
                    side.clone(),
                    std::ffi::OsStr::new("renamed"),
                )
                .unwrap()
            } else {
                sync::plan_delete(
                    &s,
                    &[id],
                    &root.join("b"),
                    &AtomicBool::new(false),
                    side.clone(),
                )
                .unwrap()
            };
            let stopped = sync::execute(&s, &p, &AtomicBool::new(true)).unwrap();
            let preview =
                sync::resume::preview(Path::new(&stopped.journal), &AtomicBool::new(false))
                    .unwrap();
            let done = sync::resume::execute(&preview, &AtomicBool::new(false)).unwrap();
            assert_eq!(done.actions[0].state, "completed");
            let (target, other) = if side == Side::Left {
                ("l", "r")
            } else {
                ("r", "l")
            };
            assert!(!root.join(target).join("a").exists());
            assert!(root.join(other).join("a").exists());
            if rename {
                assert!(root.join(target).join("renamed").exists());
            }
            let undo = recovery::preview(
                Path::new(&done.journal),
                &root.join("undo"),
                &AtomicBool::new(false),
            )
            .unwrap();
            let restored = recovery::execute(&undo, &AtomicBool::new(false)).unwrap();
            assert_eq!(restored.actions[0].state, "completed");
            assert!(root.join(target).join("a").exists());
        }
    }
}
#[test]
fn a_committed_overwrite_with_an_unconfirmed_journal_is_reconciled_by_native_identity() {
    let t = fixture();
    let root = t.path();
    fs::write(root.join("l/a"), b"source").unwrap();
    fs::write(root.join("r/a"), b"target").unwrap();
    let s = capture(root);
    let p = sync::plan(
        &s,
        &[s.report.rows[0].id.clone()],
        &root.join("b"),
        &AtomicBool::new(false),
    )
    .unwrap();
    let mut out = sync::execute(&s, &p, &AtomicBool::new(false)).unwrap();
    out.actions[0].state = "running".into();
    fs::write(&out.journal, serde_json::to_vec_pretty(&out).unwrap()).unwrap();
    let preview = sync::resume::preview(Path::new(&out.journal), &AtomicBool::new(false)).unwrap();
    assert_eq!(preview.view.satisfied, 1);
    let done = sync::resume::execute(&preview, &AtomicBool::new(false)).unwrap();
    assert_eq!(done.actions[0].state, "completed");
}

#[test]
fn an_unregistered_lookalike_temporary_is_preserved_and_blocks_mirror_restart() {
    let t = fixture();
    let root = t.path();
    fs::write(root.join("l/a"), b"source").unwrap();
    let s = capture(root);
    let p =
        sync::plan_mirror_files(&s, &root.join("b"), &AtomicBool::new(false), Side::Left).unwrap();
    let mut out = sync::execute(&s, &p, &AtomicBool::new(false)).unwrap();
    out.actions[0].state = "running".into();
    fs::write(&out.journal, serde_json::to_vec_pretty(&out).unwrap()).unwrap();
    let foreign = root
        .join("r")
        .join(format!(".clear-compare-{}-foreign", p.id));
    fs::write(&foreign, b"foreign data").unwrap();
    assert!(sync::resume::preview(Path::new(&out.journal), &AtomicBool::new(false)).is_err());
    assert_eq!(fs::read(foreign).unwrap(), b"foreign data");
}
#[test]
fn replacing_committed_output_with_identical_bytes_does_not_satisfy_ownership_evidence() {
    let t = fixture();
    let root = t.path();
    fs::write(root.join("l/a"), b"source").unwrap();
    let s = capture(root);
    let p = sync::plan(
        &s,
        &[s.report.rows[0].id.clone()],
        &root.join("b"),
        &AtomicBool::new(false),
    )
    .unwrap();
    let mut out = sync::execute(&s, &p, &AtomicBool::new(false)).unwrap();
    out.actions[0].state = "running".into();
    fs::write(&out.journal, serde_json::to_vec_pretty(&out).unwrap()).unwrap();
    fs::rename(root.join("r/a"), root.join("parked")).unwrap();
    fs::write(root.join("r/a"), b"source").unwrap();
    assert!(sync::resume::preview(Path::new(&out.journal), &AtomicBool::new(false)).is_err());
    assert_eq!(fs::read(root.join("r/a")).unwrap(), b"source");
}
#[test]
fn changing_immutable_context_bytes_and_escaping_retry_namespaces_is_rejected() {
    for context in [true, false] {
        let t = fixture();
        let root = t.path();
        fs::write(root.join("l/a"), b"source").unwrap();
        let s = capture(root);
        let p = sync::plan(
            &s,
            &[s.report.rows[0].id.clone()],
            &root.join("b"),
            &AtomicBool::new(false),
        )
        .unwrap();
        let out = sync::execute(&s, &p, &AtomicBool::new(true)).unwrap();
        if context {
            let intent = Path::new(&p.backup_root).join("intent.json");
            let mut bytes = fs::read(&intent).unwrap();
            bytes.push(b' ');
            fs::write(intent, bytes).unwrap();
        } else {
            let mut json = serde_json::to_value(&out).unwrap();
            json["plan"]["actions"][0]["backup_namespace"] = "../outside".into();
            fs::write(&out.journal, serde_json::to_vec(&json).unwrap()).unwrap();
        }
        assert!(sync::resume::preview(Path::new(&out.journal), &AtomicBool::new(false)).is_err());
        assert!(!root.join("r/a").exists());
    }
}
#[cfg(unix)]
#[test]
fn original_copy_permission_bits_are_restored_from_the_immutable_run_context() {
    use std::os::unix::fs::PermissionsExt;
    let t = fixture();
    let root = t.path();
    fs::write(root.join("l/a"), b"source").unwrap();
    fs::write(root.join("r/a"), b"target").unwrap();
    fs::set_permissions(root.join("l/a"), fs::Permissions::from_mode(0o750)).unwrap();
    fs::set_permissions(root.join("r/a"), fs::Permissions::from_mode(0o600)).unwrap();
    let s = capture(root);
    let p = sync::plan(
        &s,
        &[s.report.rows[0].id.clone()],
        &root.join("b"),
        &AtomicBool::new(false),
    )
    .unwrap();
    let out = sync::execute(&s, &p, &AtomicBool::new(true)).unwrap();
    let preview = sync::resume::preview(Path::new(&out.journal), &AtomicBool::new(false)).unwrap();
    let done = sync::resume::execute(&preview, &AtomicBool::new(false)).unwrap();
    assert_eq!(
        fs::metadata(root.join("r/a")).unwrap().permissions().mode() & 0o7777,
        0o750
    );
    let undo = recovery::preview(
        Path::new(&done.journal),
        &root.join("undo"),
        &AtomicBool::new(false),
    )
    .unwrap();
    let restored = recovery::execute(&undo, &AtomicBool::new(false)).unwrap();
    assert_eq!(restored.actions[0].state, "completed");
    assert_eq!(
        fs::metadata(root.join("r/a")).unwrap().permissions().mode() & 0o7777,
        0o600
    );
}

#[test]
fn expired_target_or_backup_evidence_prevents_writes_after_review() {
    for change_backup in [false, true] {
        let t = fixture();
        let root = t.path();
        fs::write(root.join("l/a"), b"new").unwrap();
        fs::write(root.join("r/a"), b"old").unwrap();
        let s = capture(root);
        let p = sync::plan(
            &s,
            &[s.report.rows[0].id.clone()],
            &root.join("b"),
            &AtomicBool::new(false),
        )
        .unwrap();
        let mut result = sync::execute(&s, &p, &AtomicBool::new(!change_backup)).unwrap();
        if change_backup {
            result.actions[0].state = "running".into();
            fs::write(&result.journal, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
        }
        let preview =
            sync::resume::preview(Path::new(&result.journal), &AtomicBool::new(false)).unwrap();
        let journal = fs::read(&result.journal).unwrap();
        if change_backup {
            fs::write(Path::new(&p.backup_root).join("files/a"), b"external").unwrap();
        } else {
            fs::write(root.join("r/a"), b"external").unwrap();
        }
        assert!(sync::resume::execute(&preview, &AtomicBool::new(false)).is_err());
        assert_eq!(fs::read(&result.journal).unwrap(), journal);
        assert!(!Path::new(&preview.view.backup_root).exists());
        assert_eq!(
            fs::read(root.join("r/a")).unwrap(),
            if change_backup {
                b"new".as_slice()
            } else {
                b"external".as_slice()
            }
        );
    }
}
#[test]
fn case_only_staged_rename_finishes_and_rolls_back() {
    let t = fixture();
    let root = t.path();
    fs::write(root.join("r/name"), b"original").unwrap();
    let s = capture(root);
    let p = sync::plan_rename(
        &s,
        &s.report.rows[0].id,
        &root.join("b"),
        &AtomicBool::new(false),
        Side::Right,
        std::ffi::OsStr::new("NAME"),
    )
    .unwrap();
    let mut result = sync::execute(&s, &p, &AtomicBool::new(false)).unwrap();
    assert_eq!(result.actions[0].state, "completed");
    let stage =
        clear_core::native_path::decode(&p.actions[0].rename_to.as_ref().unwrap().temporary_native)
            .unwrap();
    fs::rename(root.join("r/NAME"), root.join("r").join(stage)).unwrap();
    result.actions[0].state = "running".into();
    fs::write(&result.journal, serde_json::to_vec_pretty(&result).unwrap()).unwrap();
    let preview =
        sync::resume::preview(Path::new(&result.journal), &AtomicBool::new(false)).unwrap();
    assert_eq!(preview.view.actions[0].state, "finish_staged_rename");
    sync::resume::execute(&preview, &AtomicBool::new(false)).unwrap();
    assert_eq!(fs::read(root.join("r/NAME")).unwrap(), b"original");
    let undo = recovery::preview(
        Path::new(&result.journal),
        &root.join("undo"),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(
        recovery::execute(&undo, &AtomicBool::new(false))
            .unwrap()
            .actions
            .iter()
            .all(|a| a.state == "completed")
    );
    let names = fs::read_dir(root.join("r"))
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect::<Vec<_>>();
    assert_eq!(names, [std::ffi::OsString::from("name")]);
}
#[cfg(unix)]
#[test]
fn non_unicode_native_names_survive_restart_and_namespaced_rollback() {
    use std::os::unix::ffi::OsStringExt;
    let t = fixture();
    let root = t.path();
    let name = std::ffi::OsString::from_vec(vec![b'a', 0xff]);
    // Some filesystems reject these names; Linux CI exercises the lossless path format.
    if fs::write(root.join("l").join(&name), b"new").is_err() {
        return;
    }
    fs::write(root.join("r").join(&name), b"old").unwrap();
    let s = capture(root);
    let p = sync::plan(
        &s,
        &[s.report.rows[0].id.clone()],
        &root.join("b"),
        &AtomicBool::new(false),
    )
    .unwrap();
    let result = sync::execute(&s, &p, &AtomicBool::new(true)).unwrap();
    let preview =
        sync::resume::preview(Path::new(&result.journal), &AtomicBool::new(false)).unwrap();
    let done = sync::resume::execute(&preview, &AtomicBool::new(false)).unwrap();
    assert_eq!(fs::read(root.join("r").join(&name)).unwrap(), b"new");
    let undo = recovery::preview(
        Path::new(&done.journal),
        &root.join("undo"),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert!(
        recovery::execute(&undo, &AtomicBool::new(false))
            .unwrap()
            .actions
            .iter()
            .all(|a| a.state == "completed")
    );
    assert_eq!(fs::read(root.join("r").join(&name)).unwrap(), b"old");
}
