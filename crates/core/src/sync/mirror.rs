//! Explicit whole-comparison regular-file mirror. Empty directories are retained.
use super::*;
use clear_contracts::{Entry, VerificationBasis};

fn sides<'a>(row: &'a clear_contracts::Row, side: &Side) -> (Option<&'a Entry>, Option<&'a Entry>) {
    if *side == Side::Left {
        (row.left.as_ref(), row.right.as_ref())
    } else {
        (row.right.as_ref(), row.left.as_ref())
    }
}
fn expected(
    snapshot: &Snapshot,
    side: &Side,
    cancel: &AtomicBool,
) -> Result<BTreeMap<String, FileOperation>> {
    if !snapshot
        .report
        .policy
        .as_ref()
        .is_some_and(|p| matches!(p.verification, VerificationBasis::Sha256))
    {
        return Err(Error::Invalid(
            "File mirror requires a content-verified folder comparison".into(),
        ));
    }
    let mut actions = BTreeMap::new();
    for row in &snapshot.report.rows {
        check_cancel(cancel)?;
        let (source, target) = sides(row, side);
        if source.into_iter().chain(target).any(|e| {
            e.error.is_some()
                || !matches!(e.kind, Kind::File | Kind::Directory)
                || e.kind == Kind::File && e.digest.is_none()
        }) || matches!((source,target),(Some(a),Some(b)) if a.kind!=b.kind)
        {
            return Err(Error::Invalid(format!(
                "File mirror requires readable files/directories without links or type conflicts: {}",
                row.path
            )));
        }
        match (source, target) {
            (Some(s), t) if s.kind == Kind::File && t.is_none_or(|t| s.digest != t.digest) => {
                actions.insert(row.id.clone(), FileOperation::Copy);
            }
            (None, Some(t)) if t.kind == Kind::File => {
                actions.insert(row.id.clone(), FileOperation::Delete);
            }
            _ => {}
        }
    }
    Ok(actions)
}
pub fn plan_mirror_files(
    snapshot: &Snapshot,
    backup_base: &Path,
    cancel: &AtomicBool,
    source_side: Side,
) -> Result<SyncPlan> {
    check_cancel(cancel)?;
    let expected = expected(snapshot, &source_side, cancel)?;
    let copies = expected
        .iter()
        .filter(|(_, op)| **op == FileOperation::Copy)
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    let deletions = expected
        .iter()
        .filter(|(_, op)| **op == FileOperation::Delete)
        .map(|(id, _)| id.clone())
        .collect::<Vec<_>>();
    let mut plan = if copies.is_empty() {
        if deletions.is_empty() {
            return Err(Error::Invalid(
                "No regular-file changes to mirror; empty-directory differences are retained"
                    .into(),
            ));
        }
        plan_delete(
            snapshot,
            &deletions,
            backup_base,
            cancel,
            if source_side == Side::Left {
                Side::Right
            } else {
                Side::Left
            },
        )?
    } else {
        let mut plan = plan_direction(snapshot, &copies, backup_base, cancel, source_side.clone())?;
        if !deletions.is_empty() {
            let deleted = plan_delete(
                snapshot,
                &deletions,
                backup_base,
                cancel,
                if source_side == Side::Left {
                    Side::Right
                } else {
                    Side::Left
                },
            )?;
            plan.actions.extend(deleted.actions);
        }
        plan
    };
    plan.version = 4;
    plan.policy = FilePlanPolicy::MirrorFiles;
    plan.source_side = source_side;
    validate_plan(&plan)?;
    revalidate(snapshot, &plan, cancel)?;
    Ok(plan)
}
pub(super) fn source_absent(snapshot: &Snapshot, plan: &SyncPlan, relative: &Path) -> Result<()> {
    roots_unchanged(snapshot)?;
    let root = if plan.source_side == Side::Left {
        &snapshot.left
    } else {
        &snapshot.right
    };
    let source = checked_path(root, relative, true)?;
    match fs::symlink_metadata(source) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
        Ok(_) => Err(Error::Invalid(
            "A mirror deletion now has a source counterpart; compare again".into(),
        )),
    }
}
pub(super) fn revalidate(snapshot: &Snapshot, plan: &SyncPlan, cancel: &AtomicBool) -> Result<()> {
    roots_unchanged(snapshot)?;
    let expected = expected(snapshot, &plan.source_side, cancel)?;
    let actual = plan
        .actions
        .iter()
        .map(|a| (a.row_id.clone(), a.operation))
        .collect::<BTreeMap<_, _>>();
    if actual != expected {
        return Err(Error::Invalid(
            "Mirror actions differ from the complete reviewed file scope".into(),
        ));
    }
    // Re-scan the complete captured scope, including equal files. A new entry or
    // stale equal file must not be silently omitted by an old destructive plan.
    for (root, side) in [(&snapshot.left, Side::Left), (&snapshot.right, Side::Right)] {
        let current = crate::compare::scan(root, &snapshot.rules, true, cancel)?;
        let mut seen = 0;
        for row in &snapshot.report.rows {
            check_cancel(cancel)?;
            let (entry, _) = sides(row, &side);
            if let Some(entry) = entry {
                let path = snapshot
                    .paths
                    .get(&row.id)
                    .ok_or_else(|| Error::Invalid("Missing native mirror entry".into()))?;
                let now = current
                    .get(path)
                    .ok_or_else(|| Error::Invalid("Mirror scope changed; compare again".into()))?;
                if now.kind != entry.kind || now.error.is_some() || now.digest != entry.digest {
                    return Err(Error::Invalid(
                        "Mirror contents or types changed; compare again".into(),
                    ));
                }
                seen += 1;
            }
        }
        if seen != current.len() {
            return Err(Error::Invalid(
                "Mirror scope gained entries; compare again".into(),
            ));
        }
    }
    Ok(())
}
