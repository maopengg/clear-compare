//! Whole-tree mirror under captured exclusions; never recursive-unlink a directory.
use super::*;
use crate::native_path;
struct Layout {
    actions: Vec<(String, FileOperation)>,
    warnings: Vec<String>,
}
fn layout(snapshot: &Snapshot, side: &Side, cancel: &AtomicBool) -> Result<Layout> {
    let files = mirror::expected(snapshot, side, cancel)?;
    let mut create = Vec::new();
    let mut remove = Vec::new();
    let target = if *side == Side::Left {
        &snapshot.right
    } else {
        &snapshot.left
    };
    let mut removable = BTreeSet::new();
    for row in &snapshot.report.rows {
        check_cancel(cancel)?;
        let path = snapshot
            .paths
            .get(&row.id)
            .ok_or_else(|| Error::Invalid("Missing native tree-mirror path".into()))?;
        let (source, destination) = mirror::sides(row, side);
        if source.is_some_and(|e| e.kind == Kind::Directory) && destination.is_none() {
            create.push((path.clone(), row.id.clone()));
        }
        if source.is_none() && destination.is_some_and(|e| e.kind == Kind::Directory) {
            remove.push((path.clone(), row.id.clone()));
        }
        if files.get(&row.id) == Some(&FileOperation::Delete) {
            removable.insert(path.clone());
        }
    }
    create.sort_by(|a, b| {
        a.0.components()
            .count()
            .cmp(&b.0.components().count())
            .then(a.0.cmp(&b.0))
    });
    remove.sort_by(|a, b| {
        b.0.components()
            .count()
            .cmp(&a.0.components().count())
            .then(a.0.cmp(&b.0))
    });
    let mut removed = Vec::new();
    let mut warnings = Vec::new();
    let mut visited = 0;
    for (relative, id) in remove {
        check_cancel(cancel)?;
        let path = checked_path(target, &relative, false)?;
        let mut can_remove = true;
        for entry in fs::read_dir(&path)? {
            check_cancel(cancel)?;
            visited += 1;
            if visited > crate::compare::MAX_ENTRIES {
                return Err(Error::Invalid(
                    "Directory mirror preflight exceeds 200000 direct entries".into(),
                ));
            }
            let child = relative.join(entry?.file_name());
            if !removable.contains(&child) {
                can_remove = false;
                break;
            }
        }
        if can_remove {
            removable.insert(relative);
            removed.push((id, FileOperation::RemoveDirectory));
        } else {
            warnings.push(format!(
                "Retain directory containing excluded or unplanned entries: {}",
                crate::tree::display_path(&relative)
            ));
        }
    }
    let mut actions = create
        .into_iter()
        .map(|(_, id)| (id, FileOperation::CreateDirectory))
        .collect::<Vec<_>>();
    actions.extend(
        files
            .iter()
            .filter(|(_, op)| **op == FileOperation::Copy)
            .map(|(id, op)| (id.clone(), *op)),
    );
    actions.extend(
        files
            .iter()
            .filter(|(_, op)| **op == FileOperation::Delete)
            .map(|(id, op)| (id.clone(), *op)),
    );
    actions.extend(removed);
    Ok(Layout { actions, warnings })
}
pub fn plan_mirror_tree(
    snapshot: &Snapshot,
    backup_base: &Path,
    cancel: &AtomicBool,
    source_side: Side,
) -> Result<SyncPlan> {
    roots_unchanged(snapshot)?;
    let Layout {
        actions: layout,
        warnings,
    } = layout(snapshot, &source_side, cancel)?;
    if layout.is_empty() {
        return Err(Error::Invalid(
            "No included tree changes to mirror; excluded-entry ancestors are retained".into(),
        ));
    }
    let mut plan = if let Some(plan) =
        mirror::file_plan(snapshot, backup_base, cancel, source_side.clone())?
    {
        plan
    } else {
        let base = fs::canonicalize(backup_base)?;
        if !base.is_dir() || base.to_str().is_none() {
            return Err(Error::Invalid(
                "Tree-mirror backup base must be an existing Unicode directory".into(),
            ));
        }
        reject_overlap(&base, &snapshot.left)?;
        reject_overlap(&base, &snapshot.right)?;
        let target = if source_side == Side::Left {
            &snapshot.right
        } else {
            &snapshot.left
        };
        let id = unique_id();
        SyncPlan {
            warnings: Vec::new(),
            target: target.to_string_lossy().into(),
            version: 5,
            policy: FilePlanPolicy::MirrorTree,
            id: id.clone(),
            target_native: Some(native_path::encode(target)),
            source_side: source_side.clone(),
            session: snapshot.report.session.clone(),
            actions: Vec::new(),
            backup_root: base.join(id).to_string_lossy().into(),
        }
    };
    let mut files = std::mem::take(&mut plan.actions)
        .into_iter()
        .map(|a| (a.row_id.clone(), a))
        .collect::<BTreeMap<_, _>>();
    let target = if source_side == Side::Left {
        &snapshot.right
    } else {
        &snapshot.left
    };
    for (id, operation) in layout {
        if let Some(action) = files.remove(&id) {
            plan.actions.push(action);
            continue;
        }
        let relative = snapshot
            .paths
            .get(&id)
            .ok_or_else(|| Error::Invalid("Missing native directory path".into()))?;
        let recorded = if operation == FileOperation::RemoveDirectory {
            Some(
                directory::observe(target, relative)?
                    .ok_or_else(|| Error::Invalid("Directory disappeared".into()))?,
            )
        } else {
            None
        };
        plan.actions.push(FileAction {
            rename_to: None,
            operation,
            directory_identity: recorded.as_ref().map(|r| r.identity.clone()),
            target_permissions: recorded.map(|r| r.permissions),
            row_id: id,
            relative_native: Some(native_path::encode(relative)),
            path: crate::tree::display_path(relative),
            target_existed: operation == FileOperation::RemoveDirectory,
            bytes: "0".into(),
            source_digest: None,
            target_digest: None,
        });
    }
    plan.version = 5;
    plan.policy = FilePlanPolicy::MirrorTree;
    plan.source_side = source_side;
    plan.warnings = warnings;
    validate_plan(&plan)?;
    revalidate(snapshot, &plan, cancel)?;
    Ok(plan)
}
pub(super) fn revalidate(snapshot: &Snapshot, plan: &SyncPlan, cancel: &AtomicBool) -> Result<()> {
    let expected = layout(snapshot, &plan.source_side, cancel)?.actions;
    let actual = plan
        .actions
        .iter()
        .map(|a| (a.row_id.clone(), a.operation))
        .collect::<Vec<_>>();
    if actual != expected {
        return Err(Error::Invalid(
            "Tree-mirror action scope or dependency order changed; compare again".into(),
        ));
    }
    let target = target_root(snapshot, plan);
    for action in &plan.actions {
        if action.operation == FileOperation::CreateDirectory {
            let path = snapshot
                .paths
                .get(&action.row_id)
                .ok_or_else(|| Error::Invalid("Missing tree-mirror directory".into()))?;
            if directory::observe(target, path)?.is_some() {
                return Err(Error::Invalid(
                    "Directory creation path is occupied or aliases an existing name".into(),
                ));
            }
        }
        if action.operation == FileOperation::RemoveDirectory {
            let path = snapshot
                .paths
                .get(&action.row_id)
                .ok_or_else(|| Error::Invalid("Missing tree-mirror directory".into()))?;
            let now = directory::observe(target, path)?
                .ok_or_else(|| Error::Invalid("Tree-mirror directory disappeared".into()))?;
            if action.directory_identity.as_ref() != Some(&now.identity)
                || action.target_permissions.as_ref() != Some(&now.permissions)
            {
                return Err(Error::Invalid(
                    "Tree-mirror directory identity or permissions changed after preview".into(),
                ));
            }
        }
    }
    mirror::revalidate_inventory(snapshot, cancel)
}
