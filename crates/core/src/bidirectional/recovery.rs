use super::execute::RunIntent;
use super::*;
pub struct RecoveryPlan {
    pub view: BidirectionalRecoveryPreview,
    root: PathBuf,
    journal_hash: String,
    baseline_path: PathBuf,
    baseline_hash: String,
    before_hash: String,
    children: Vec<crate::recovery::Plan>,
}
fn check_roots(base: &Baseline) -> Result<()> {
    if base.left_root.identity.volume == base.right_root.identity.volume
        && base.left_root.identity.index == base.right_root.identity.index
    {
        return Err(invalid("Bidirectional roots alias the same directory"));
    }
    for (root, pin) in [
        (&base.left, &base.left_root),
        (&base.right, &base.right_root),
    ] {
        let root = native_path::decode(root)?;
        if fs::canonicalize(&root)? != root
            || durable::Object::observe(&root, Path::new(""))?.as_ref() != Some(pin)
        {
            return Err(invalid("Original bidirectional root identity changed"));
        }
    }
    Ok(())
}
fn read_run(journal: &Path) -> Result<(RunIntent, String)> {
    let root = journal
        .parent()
        .ok_or_else(|| invalid("Run parent missing"))?;
    let (result, hash): (RunResult, String) = durable::read(journal, 128 * 1024 * 1024)?;
    let (intent, intent_hash): (RunIntent, String) = durable::read(
        &crate::compare::checked_path(root, Path::new("intent.json"), false)?,
        128 * 1024 * 1024,
    )?;
    if journal.file_name() != Some(std::ffi::OsStr::new("run.json"))
        || root.file_name().and_then(|n| n.to_str()) != Some(intent.id.as_str())
        || root.parent().and_then(|p| p.file_name()) != Some(std::ffi::OsStr::new(".bidirectional"))
        || intent.version != 1
        || result.id != intent.id
        || result.intent_digest != intent_hash
        || intent.operations.len() > 2
    {
        return Err(invalid(
            "Invalid bidirectional run placement or immutable intent",
        ));
    }
    durable::namespace(&intent.id)?;
    inventory(&intent.desired)?;
    inventory(&intent.baseline.entries)?;
    check_roots(&intent.baseline)?;
    let base = root
        .parent()
        .and_then(|p| p.parent())
        .ok_or_else(|| invalid("Run backup base missing"))?;
    for root in [&intent.baseline.left, &intent.baseline.right] {
        crate::compare::reject_overlap(base, &native_path::decode(root)?)?;
    }
    let mut sides = BTreeSet::new();
    for plan in &intent.operations {
        sync::validate_plan_metadata(plan)?;
        durable::namespace(&plan.id)?;
        let FilePlanPolicy::Reconcile { target_side } = &plan.policy else {
            return Err(invalid("Run contains an unrelated operation"));
        };
        let left = *target_side == Side::Left;
        if !sides.insert(left)
            || Path::new(&plan.backup_root) != base.join(&plan.id)
            || plan.target_native.as_ref()
                != Some(if left {
                    &intent.baseline.left
                } else {
                    &intent.baseline.right
                })
        {
            return Err(invalid("Run child scope or backup placement changed"));
        }
    }
    let before = crate::compare::checked_path(root, Path::new("baseline-before.json"), false)?;
    let (original, original_hash): (Baseline, String) = durable::read(&before, MAX_BASELINE)?;
    let (after, after_hash): (Baseline, String) = durable::read(
        &crate::compare::checked_path(root, Path::new("baseline-after.json"), false)?,
        MAX_BASELINE,
    )?;
    if original_hash != intent.baseline_hash
        || original != intent.baseline
        || after_hash != intent.after_hash
        || after.id != original.id
        || after.revision
            != original
                .revision
                .checked_add(1)
                .ok_or_else(|| invalid("Invalid revision"))?
        || after.last_run.as_deref() != Some(intent.id.as_str())
        || after.left != original.left
        || after.right != original.right
        || after.left_root != original.left_root
        || after.right_root != original.right_root
        || after.exclusions != original.exclusions
        || after.entries != intent.desired
    {
        return Err(invalid("Run baseline evidence changed"));
    }
    Ok((intent, hash))
}
pub fn preview_recovery(
    journal: &Path,
    backup: &Path,
    cancel: &AtomicBool,
) -> Result<RecoveryPlan> {
    check_cancel(cancel)?;
    let journal = baseline::canonical(journal, false)?;
    let root = journal
        .parent()
        .ok_or_else(|| invalid("Run parent missing"))?
        .to_owned();
    let (intent, _) = read_run(&journal)?;
    let baseline_path = native_path::decode(&intent.baseline_path)?;
    if baseline::canonical(&baseline_path, false)? != baseline_path {
        return Err(invalid("Baseline path changed"));
    }
    for native in [&intent.baseline.left, &intent.baseline.right] {
        if baseline_path.starts_with(native_path::decode(native)?) {
            return Err(invalid("Baseline moved inside a source tree"));
        }
    }
    let _baseline_lock = baseline::lock(&baseline_path, false)?;
    let _run_lock = durable::lock(&root, false)?;
    let (intent, journal_hash) = read_run(&journal)?;
    let baseline_hash = baseline::read(&baseline_path)?.1;
    let restore = if baseline_hash == intent.after_hash {
        true
    } else if baseline_hash == intent.baseline_hash {
        false
    } else {
        return Err(invalid(
            "Baseline advanced or changed since this run; newer work must be reviewed first",
        ));
    };
    let backup = fs::canonicalize(backup)?;
    for native in [&intent.baseline.left, &intent.baseline.right] {
        crate::compare::reject_overlap(&backup, &native_path::decode(native)?)?;
    }
    let (run, _): (RunResult, String) = durable::read(&journal, 128 * 1024 * 1024)?;
    let mut children = Vec::new();
    for child in intent.operations.iter().rev() {
        let path = Path::new(&child.backup_root).join("journal.json");
        if !path.try_exists()? {
            if restore
                || run
                    .started_journals
                    .contains(&path.to_string_lossy().to_string())
                || Path::new(&child.backup_root).try_exists()?
            {
                return Err(invalid(
                    "A started child journal is missing; retain the run for manual review",
                ));
            }
            continue;
        }
        let path = baseline::canonical(&path, false)?;
        let (report, _): (SyncResult, String) = durable::read(&path, 64 * 1024 * 1024)?;
        if !durable::intent_matches(&report.plan, child) {
            return Err(invalid("Child journal differs from the reviewed run"));
        }
        if let Some(plan) = crate::recovery::preview_optional(&path, &backup, cancel)? {
            children.push(plan);
        }
    }
    if children.is_empty() && !restore {
        return Err(invalid(
            "No applied bidirectional actions or baseline update to recover",
        ));
    }
    let view = BidirectionalRecoveryPreview {
        id: unique_id(),
        journal: journal.to_string_lossy().into(),
        baseline: baseline_path.to_string_lossy().into(),
        restore_baseline: restore,
        total: children.iter().map(|p| p.view.actions.len()).sum(),
        rows: children
            .iter()
            .flat_map(|p| {
                p.view.actions.iter().map(|a| BidirectionalRecoveryRow {
                    path: a.path.clone(),
                    operation: a.operation.clone(),
                    target: p.view.target.clone(),
                })
            })
            .take(500)
            .collect(),
        operations: children
            .iter()
            .map(|p| {
                let mut v = p.view.clone();
                v.actions.clear();
                v
            })
            .collect(),
    };
    Ok(RecoveryPlan {
        view,
        root,
        journal_hash,
        baseline_path,
        baseline_hash,
        before_hash: intent.baseline_hash,
        children,
    })
}
impl RecoveryPlan {
    pub fn page(&self, offset: usize) -> Vec<BidirectionalRecoveryRow> {
        self.children
            .iter()
            .flat_map(|p| p.view.actions.iter().map(move |a| (a, &p.view.target)))
            .skip(offset)
            .take(500)
            .map(|(a, target)| BidirectionalRecoveryRow {
                path: a.path.clone(),
                operation: a.operation.clone(),
                target: target.clone(),
            })
            .collect()
    }
    pub fn affected_paths(&self) -> Vec<PathBuf> {
        self.children
            .iter()
            .flat_map(|p| p.affected_paths())
            .collect()
    }
}
pub fn execute_recovery(
    plan: &RecoveryPlan,
    cancel: &AtomicBool,
) -> Result<BidirectionalRecoveryResult> {
    let (intent, _) = read_run(&plan.root.join("run.json"))?;
    write_guard(&intent.baseline, cancel, || {
        execute_recovery_inner(plan, cancel)
    })
}
fn execute_recovery_inner(
    plan: &RecoveryPlan,
    cancel: &AtomicBool,
) -> Result<BidirectionalRecoveryResult> {
    let _baseline_lock = baseline::lock(&plan.baseline_path, false)?;
    let _run_lock = durable::lock(&plan.root, false)?;
    let _guard = crate::task::validated_mutation(cancel, || {
        let (_, hash) = read_run(&plan.root.join("run.json"))?;
        if hash != plan.journal_hash || baseline::read(&plan.baseline_path)?.1 != plan.baseline_hash
        {
            return Err(invalid("Bidirectional recovery preview expired"));
        }
        Ok(())
    })?;
    let recovery_parent = crate::compare::checked_path(&plan.root, Path::new("recoveries"), true)?;
    fs::create_dir_all(&recovery_parent)?;
    crate::compare::checked_path(&plan.root, Path::new("recoveries"), false)?;
    let root = recovery_parent.join(&plan.view.id);
    durable::namespace(&plan.view.id)?;
    fs::create_dir(&root)?;
    sync::flush_ancestors(&root, &plan.root)?;
    let current = durable::bytes(&plan.baseline_path, MAX_BASELINE)?;
    let mut backup = File::create_new(root.join("baseline-current.json"))?;
    backup.write_all(&current)?;
    backup.sync_all()?;
    drop(backup);
    let journal = root.join("recovery.json");
    let mut result = BidirectionalRecoveryResult {
        details_in_journal: false,
        journal: journal.to_string_lossy().into(),
        baseline_restored: false,
        operations: Vec::new(),
        error: None,
    };
    sync::atomic_json(&journal, &result)?;
    drop(_guard);
    for child in &plan.children {
        match crate::recovery::execute(child, cancel) {
            Ok(out) => {
                let complete = out.actions.iter().all(|a| a.state == "completed");
                result.error = out.actions.iter().find_map(|a| a.error.clone());
                result.operations.push(out);
                sync::atomic_json(&journal, &result)?;
                if !complete {
                    result
                        .error
                        .get_or_insert("Child rollback did not complete".into());
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
        let restored = (|| -> Result<()> {
            let _guard = crate::task::mutation(cancel)?;
            let (intent, _) = read_run(&plan.root.join("run.json"))?;
            check_roots(&intent.baseline)?;
            if baseline::read(&plan.baseline_path)?.1 != plan.baseline_hash {
                return Err(invalid("Baseline changed during rollback"));
            }
            let bytes = durable::bytes(
                &crate::compare::checked_path(
                    &plan.root,
                    Path::new("baseline-before.json"),
                    false,
                )?,
                MAX_BASELINE,
            )?;
            if durable::digest(&bytes) != plan.before_hash {
                return Err(invalid("Original baseline backup changed"));
            }
            if plan.view.restore_baseline {
                baseline::replace(&plan.baseline_path, &bytes)?;
            }
            result.baseline_restored = true;
            crate::task::completed();
            Ok(())
        })();
        if let Err(e) = restored {
            result.error = Some(sync::failure_message(&e));
        }
    }
    sync::atomic_json(&journal, &result)?;
    Ok(result)
}
