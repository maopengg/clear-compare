use super::*;
pub type RunResult = BidirectionalResult;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RunIntent {
    pub version: u32,
    pub id: String,
    pub baseline_path: NativePath,
    pub baseline_hash: String,
    pub baseline: Baseline,
    pub desired: Vec<Stored>,
    pub operations: Vec<SyncPlan>,
    pub after_hash: String,
}
fn persist(path: &Path, value: &RunResult) -> Result<()> {
    sync::atomic_json(path, value)
}
pub fn execute(plan: &Plan, cancel: &AtomicBool) -> Result<RunResult> {
    write_guard(&plan.baseline, cancel, || execute_inner(plan, cancel))
}
fn execute_inner(plan: &Plan, cancel: &AtomicBool) -> Result<RunResult> {
    if plan.view.conflicts != 0 || plan.view.blocked != 0 {
        return Err(invalid(
            "Resolve every bidirectional conflict before execution",
        ));
    }
    let _baseline_lock = baseline::lock(&plan.baseline_path, false)?;
    let setup = crate::task::validated_mutation(cancel, || {
        if baseline::canonical(&plan.baseline_path, false)? != plan.baseline_path
            || baseline::read(&plan.baseline_path)?.1 != plan.hash
        {
            return Err(invalid("Baseline changed after preview"));
        }
        roots(&plan.baseline, &plan.snapshot)?;
        sync::revalidate_scope(&plan.snapshot, cancel)
    })?;
    let root = &plan.run_root;
    if plan.view.backup_root != root.to_string_lossy() {
        return Err(invalid("Run backup path changed"));
    }
    let id = root
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| invalid("Invalid run id"))?
        .to_owned();
    durable::namespace(&id)?;
    let parent = root.parent().ok_or_else(|| invalid("Run parent missing"))?;
    if parent != plan.backup.join(".bidirectional")
        || fs::canonicalize(&plan.backup)? != plan.backup
    {
        return Err(invalid("Run backup base changed"));
    }
    let parent = crate::compare::checked_path(&plan.backup, Path::new(".bidirectional"), true)?;
    fs::create_dir_all(&parent)?;
    crate::compare::checked_path(&plan.backup, Path::new(".bidirectional"), false)?;
    fs::create_dir(root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
    }
    sync::flush_ancestors(root, &plan.backup)?;
    let _run_lock = durable::lock(root, true)?;
    let before = durable::bytes(&plan.baseline_path, MAX_BASELINE)?;
    if durable::digest(&before) != plan.hash {
        return Err(invalid("Baseline changed during setup"));
    }
    let mut original = File::create_new(root.join("baseline-before.json"))?;
    original.write_all(&before)?;
    original.sync_all()?;
    drop(original);
    let mut next = baseline::read(&plan.baseline_path)?.0;
    next.revision = next
        .revision
        .checked_add(1)
        .ok_or_else(|| invalid("Baseline revision exhausted"))?;
    next.last_run = Some(id.clone());
    next.entries = store(&plan.desired);
    let after_hash = durable::write(&root.join("baseline-after.json"), &next, MAX_BASELINE)?;
    let intent = RunIntent {
        version: 1,
        id: id.clone(),
        baseline_path: native_path::encode(&plan.baseline_path),
        baseline_hash: plan.hash.clone(),
        baseline: baseline::read(&plan.baseline_path)?.0,
        desired: store(&plan.desired),
        operations: plan.operations.clone(),
        after_hash: after_hash.clone(),
    };
    let intent_digest = durable::write(&root.join("intent.json"), &intent, 128 * 1024 * 1024)?;
    let journal = root.join("run.json");
    let mut result = RunResult {
        details_in_journal: false,
        baseline_info: baseline::info(&plan.baseline_path, &plan.baseline),
        started_journals: Vec::new(),
        intent_digest,
        id,
        journal: journal.to_string_lossy().into(),
        baseline: plan.baseline_path.to_string_lossy().into(),
        baseline_updated: false,
        phase: "running".into(),
        planned_journals: plan
            .operations
            .iter()
            .map(|p| {
                Path::new(&p.backup_root)
                    .join("journal.json")
                    .to_string_lossy()
                    .into()
            })
            .collect(),
        operations: Vec::new(),
        error: None,
    };
    persist(&journal, &result)?;
    drop(setup);
    for operation in &plan.operations {
        if let Err(e) = check_cancel(cancel) {
            result.error = Some(sync::failure_message(&e));
            break;
        }
        result.started_journals.push(
            Path::new(&operation.backup_root)
                .join("journal.json")
                .to_string_lossy()
                .into(),
        );
        persist(&journal, &result)?;
        match sync::execute(&plan.snapshot, operation, cancel) {
            Ok(out) => {
                let complete = out.actions.iter().all(|a| a.state == "completed");
                let error = out.actions.iter().find_map(|a| a.error.clone());
                result.operations.push(out);
                persist(&journal, &result)?;
                if !complete {
                    result.error =
                        error.or_else(|| Some("A child operation did not complete".into()));
                    break;
                }
            }
            Err(e) => {
                result.error = Some(sync::failure_message(&e));
                break;
            }
        }
    }
    if result.error.is_none() {
        crate::task::stage("verify_bidirectional_convergence");
        let finalise = (|| -> Result<()> {
            let _guard = crate::task::validated_mutation(cancel, || {
                roots(&plan.baseline, &plan.snapshot)?;
                if baseline::read(&plan.baseline_path)?.1 != plan.hash {
                    return Err(invalid("Baseline changed before commit"));
                }
                for root in [&plan.snapshot.left, &plan.snapshot.right] {
                    if scan(root, &plan.snapshot.rules, cancel)? != plan.desired {
                        return Err(invalid(
                            "Trees did not converge to the reviewed result; baseline was not advanced",
                        ));
                    }
                }
                Ok(())
            })?;
            result.phase = "baseline_commit_pending".into();
            persist(&journal, &result)?;
            sync::atomic_json(&plan.baseline_path, &next)?;
            if baseline::read(&plan.baseline_path)?.1 != after_hash {
                return Err(invalid("Committed baseline verification failed"));
            }
            result.baseline_updated = true;
            result.baseline_info = baseline::info(&plan.baseline_path, &next);
            crate::task::completed();
            Ok(())
        })();
        if let Err(e) = finalise {
            result.error = Some(sync::failure_message(&e));
        }
    }
    result.phase = if result.error.is_none() {
        "completed"
    } else if cancel.load(std::sync::atomic::Ordering::Relaxed) {
        "cancelled"
    } else {
        "failed"
    }
    .into();
    persist(&journal, &result)?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cancellation_after_one_side_completed_keeps_the_baseline_and_supports_coordinated_rollback()
    {
        use std::{
            sync::atomic::Ordering,
            time::{Duration, Instant},
        };
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for directory in ["l", "r", "b", "undo"] {
            fs::create_dir(root.join(directory)).unwrap();
        }
        let capture = || {
            Arc::new(
                crate::compare::compare(
                    &CompareRequest {
                        left: root.join("l").display().to_string(),
                        right: root.join("r").display().to_string(),
                        exclusions: String::new(),
                        content: true,
                    },
                    &AtomicBool::new(false),
                )
                .unwrap(),
            )
        };
        let baseline = root.join("common.ccbase");
        super::super::capture(&capture(), &baseline, true, &AtomicBool::new(false)).unwrap();
        let original = fs::read(&baseline).unwrap();
        fs::create_dir(root.join("r/empty")).unwrap();
        fs::write(root.join("l/file"), vec![b'x'; 256 * 1024]).unwrap();
        let plan = super::super::prepare(
            capture(),
            &baseline,
            &root.join("b"),
            &AtomicBool::new(false),
        )
        .unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let control =
            crate::task::Control::new(OperationKind::File, plan.view.actions + 1, cancel.clone());
        let observer = control.clone();
        let result = std::thread::scope(|scope| {
            let observer = scope.spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(20);
                loop {
                    if observer.status().phase == TaskPhase::Paused {
                        observer.cancel();
                        return;
                    }
                    if Instant::now() >= deadline {
                        observer.cancel();
                        panic!("bidirectional worker did not pause");
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            });
            crate::task::pause_after_first_stream_write();
            let result = control.run(|| execute(&plan, &cancel)).unwrap();
            observer.join().unwrap();
            result
        });
        assert!(cancel.load(Ordering::Relaxed));
        assert_eq!(result.phase, "cancelled", "{result:?}");
        assert!(!result.baseline_updated);
        assert_eq!(fs::read(&baseline).unwrap(), original);
        assert!(root.join("l/empty").is_dir());
        assert!(!root.join("r/file").exists());
        let undo = super::super::preview_recovery(
            Path::new(&result.journal),
            &root.join("undo"),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert!(!undo.view.restore_baseline);
        let done = super::super::execute_recovery(&undo, &AtomicBool::new(false)).unwrap();
        assert!(done.error.is_none(), "{done:?}");
        assert!(!root.join("l/empty").exists());
        assert!(root.join("r/empty").is_dir());
        assert_eq!(fs::read(&baseline).unwrap(), original);
    }
}

#[cfg(all(test, unix))]
mod root_change_tests {
    use super::*;
    use std::time::{Duration, Instant};
    #[test]
    fn replacing_a_root_while_paused_blocks_both_forward_and_rollback_commits() {
        for undo in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path();
            for d in ["l", "r", "b", "undo"] {
                fs::create_dir(root.join(d)).unwrap();
            }
            for side in ["l", "r"] {
                fs::write(root.join(side).join("a"), b"old").unwrap();
            }
            let snapshot = || {
                Arc::new(
                    crate::compare::compare(
                        &CompareRequest {
                            left: root.join("l").display().to_string(),
                            right: root.join("r").display().to_string(),
                            exclusions: String::new(),
                            content: true,
                        },
                        &AtomicBool::new(false),
                    )
                    .unwrap(),
                )
            };
            let path = root.join("common.ccbase");
            super::super::capture(&snapshot(), &path, true, &AtomicBool::new(false)).unwrap();
            fs::write(root.join("r/a"), vec![b'x'; 256 * 1024]).unwrap();
            let plan =
                super::super::prepare(snapshot(), &path, &root.join("b"), &AtomicBool::new(false))
                    .unwrap();
            let recovery = if undo {
                let out = execute(&plan, &AtomicBool::new(false)).unwrap();
                Some(
                    super::super::preview_recovery(
                        Path::new(&out.journal),
                        &root.join("undo"),
                        &AtomicBool::new(false),
                    )
                    .unwrap(),
                )
            } else {
                None
            };
            let baseline_bytes = fs::read(&path).unwrap();
            let expected = if undo {
                vec![b'x'; 256 * 1024]
            } else {
                b"old".to_vec()
            };
            let cancel = Arc::new(AtomicBool::new(false));
            let control = crate::task::Control::new(OperationKind::File, 2, cancel.clone());
            let observer = control.clone();
            let observer_root = root.to_owned();
            let replacement = expected.clone();
            std::thread::scope(|scope| {
                let observing = scope.spawn(move || {
                    let deadline = Instant::now() + Duration::from_secs(20);
                    loop {
                        if observer.status().phase == TaskPhase::Paused {
                            fs::rename(observer_root.join("l"), observer_root.join("parked"))
                                .unwrap();
                            fs::create_dir(observer_root.join("l")).unwrap();
                            fs::write(observer_root.join("l/a"), replacement).unwrap();
                            observer.resume().unwrap();
                            return;
                        }
                        if Instant::now() >= deadline {
                            observer.cancel();
                            panic!("root-change test did not pause");
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    }
                });
                crate::task::pause_after_first_stream_write();
                if let Some(recovery) = &recovery {
                    let out = control
                        .run(|| super::super::execute_recovery(recovery, &cancel))
                        .unwrap();
                    assert!(out.error.is_some(), "{out:?}");
                    assert!(!out.baseline_restored);
                } else {
                    let out = control.run(|| execute(&plan, &cancel)).unwrap();
                    assert!(out.error.is_some(), "{out:?}");
                    assert!(!out.baseline_updated);
                }
                observing.join().unwrap();
            });
            assert_eq!(fs::read(root.join("l/a")).unwrap(), expected);
            assert_eq!(fs::read(&path).unwrap(), baseline_bytes);
        }
    }
}
