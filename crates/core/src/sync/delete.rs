//! Reviewed regular-file removal. Verified backups precede every unlink.
use super::*;
use std::{io::Read, time::UNIX_EPOCH};
pub(crate) fn permissions(meta: &fs::Metadata) -> FilePermissions {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        FilePermissions::Unix {
            mode: meta.permissions().mode() & 0o7777,
        }
    }
    #[cfg(not(unix))]
    {
        FilePermissions::ReadOnly {
            value: meta.permissions().readonly(),
        }
    }
}
pub(crate) fn set_permissions(file: &File, stored: &FilePermissions) -> Result<()> {
    match stored {
        #[cfg(unix)]
        FilePermissions::Unix { mode } if *mode <= 0o7777 => {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(*mode))?;
        }
        #[cfg(not(unix))]
        FilePermissions::ReadOnly { value } => {
            let mut p = file.metadata()?.permissions();
            p.set_readonly(*value);
            file.set_permissions(p)?;
        }
        _ => {
            return Err(Error::Invalid(
                "Recorded permissions are unsupported on this operating system".into(),
            ));
        }
    }
    Ok(())
}
pub fn plan_delete(
    snapshot: &Snapshot,
    row_ids: &[String],
    backup_base: &Path,
    cancel: &AtomicBool,
    target_side: Side,
) -> Result<SyncPlan> {
    check_cancel(cancel)?;
    if row_ids.is_empty() {
        return Err(Error::Invalid("Select regular files to delete".into()));
    }
    let base = fs::canonicalize(backup_base)?;
    if !base.is_dir() || base.to_str().is_none() {
        return Err(Error::Invalid(
            "Deletion backup base must be an existing Unicode directory".into(),
        ));
    }
    reject_overlap(&base, &snapshot.left)?;
    reject_overlap(&base, &snapshot.right)?;
    let root = if target_side == Side::Left {
        &snapshot.left
    } else {
        &snapshot.right
    };
    if fs::canonicalize(root)? != *root {
        return Err(Error::Invalid("Deletion target root changed".into()));
    }
    let mut actions = Vec::new();
    let mut seen = BTreeSet::new();
    let rows = snapshot
        .report
        .rows
        .iter()
        .map(|r| (r.id.as_str(), r))
        .collect::<BTreeMap<_, _>>();
    for row_id in row_ids {
        check_cancel(cancel)?;
        if !seen.insert(row_id) {
            continue;
        }
        let relative = snapshot
            .paths
            .get(row_id)
            .ok_or_else(|| Error::Invalid("Unknown deletion entry".into()))?;
        let row = rows
            .get(row_id.as_str())
            .copied()
            .ok_or_else(|| Error::Invalid("Unknown deletion row".into()))?;
        let entry = if target_side == Side::Left {
            row.left.as_ref()
        } else {
            row.right.as_ref()
        };
        let entry = entry
            .filter(|e| e.kind == Kind::File && e.error.is_none())
            .ok_or_else(|| {
                Error::Invalid(
                    "Deletion requires a readable regular file on the selected side".into(),
                )
            })?;
        if snapshot.rules.excludes(relative, false) {
            return Err(Error::Invalid("Excluded deletion entry".into()));
        }
        let path = checked_path(root, relative, false)?;
        let digest = regular_digest(&path, cancel)?
            .ok_or_else(|| Error::Invalid("Deletion target disappeared".into()))?;
        let meta = fs::symlink_metadata(&path)?;
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|t| t.as_millis().to_string());
        if entry.bytes != meta.len().to_string()
            || entry.modified_ms.is_some() && entry.modified_ms != modified
            || entry.digest.as_ref().is_some_and(|d| d != &digest)
        {
            return Err(Error::Invalid(
                "Deletion target changed since comparison. Compare again.".into(),
            ));
        }
        actions.push(FileAction {
            rename_to: None,
            operation: FileOperation::Delete,
            target_permissions: Some(permissions(&meta)),
            row_id: row_id.clone(),
            relative_native: Some(crate::native_path::encode(relative)),
            path: row.path.clone(),
            target_existed: true,
            bytes: meta.len().to_string(),
            source_digest: None,
            target_digest: Some(digest),
        });
    }
    let id = unique_id();
    let plan = SyncPlan {
        target: root.to_string_lossy().into(),
        version: 2,
        policy: FilePlanPolicy::DeleteSelected { target_side },
        id: id.clone(),
        target_native: Some(crate::native_path::encode(root)),
        source_side: Side::Left,
        session: snapshot.report.session.clone(),
        actions,
        backup_root: base.join(id).to_string_lossy().into(),
    };
    validate_plan(&plan)?;
    Ok(plan)
}
pub(super) fn backup_before_change(
    snapshot: &Snapshot,
    plan: &SyncPlan,
    action: &FileAction,
    cancel: &AtomicBool,
) -> Result<PathBuf> {
    check_cancel(cancel)?;
    let root = target_root(snapshot, plan);
    if fs::canonicalize(root)? != root {
        return Err(Error::Invalid("Deletion target root changed".into()));
    }
    let relative = snapshot
        .paths
        .get(&action.row_id)
        .ok_or_else(|| Error::Invalid("Unknown deletion entry".into()))?;
    if action.relative_native.as_ref() != Some(&crate::native_path::encode(relative))
        || snapshot.rules.excludes(relative, false)
    {
        return Err(Error::Invalid(
            "Deletion path differs from its reviewed snapshot".into(),
        ));
    }
    let path = checked_path(root, relative, false)?;
    if plan.policy == FilePlanPolicy::MirrorFiles {
        super::mirror::source_absent(snapshot, plan, relative)?;
    }
    let expected = action
        .target_digest
        .as_deref()
        .ok_or_else(|| Error::Invalid("Deletion digest missing".into()))?;
    let saved_permissions = action
        .target_permissions
        .as_ref()
        .ok_or_else(|| Error::Invalid("Deletion permissions missing".into()))?;
    if regular_digest(&path, cancel)?.as_deref() != Some(expected)
        || permissions(&fs::symlink_metadata(&path)?) != *saved_permissions
    {
        return Err(Error::Invalid(
            "Deletion target changed after preview".into(),
        ));
    }
    let backup_root = Path::new(&plan.backup_root);
    let backup = checked_path(backup_root, &PathBuf::from("files").join(relative), true)?;
    fs::create_dir_all(
        backup
            .parent()
            .ok_or_else(|| Error::Invalid("Invalid deletion backup path".into()))?,
    )?;
    let mut writer = File::create_new(&backup)?;
    let mut reader = File::open(&path)?;
    let mut buffer = [0u8; 65536];
    loop {
        check_cancel(cancel)?;
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        writer.write_all(&buffer[..n])?;
    }
    set_permissions(&writer, saved_permissions)?;
    writer.sync_all()?;
    drop(reader);
    drop(writer);
    if hash_file(&backup, cancel)? != expected
        || permissions(&fs::symlink_metadata(&backup)?) != *saved_permissions
    {
        return Err(Error::Invalid("Deletion backup verification failed".into()));
    }
    check_cancel(cancel)?;
    if fs::canonicalize(root)? != root {
        return Err(Error::Invalid(
            "Deletion target root changed during backup".into(),
        ));
    }
    checked_path(root, relative, false)?;
    if regular_digest(&path, cancel)?.as_deref() != Some(expected)
        || permissions(&fs::symlink_metadata(&path)?) != *saved_permissions
    {
        return Err(Error::Invalid(
            "Deletion target changed during backup".into(),
        ));
    }
    flush_ancestors(
        backup
            .parent()
            .ok_or_else(|| Error::Invalid("Invalid deletion backup parent".into()))?,
        backup_root,
    )?;
    check_cancel(cancel)?;
    checked_path(root, relative, false)?;
    if regular_digest(&path, cancel)?.as_deref() != Some(expected) {
        return Err(Error::Invalid(
            "Deletion target changed before removal".into(),
        ));
    }
    Ok(path)
}
pub(super) fn delete_one(
    snapshot: &Snapshot,
    plan: &SyncPlan,
    action: &FileAction,
    cancel: &AtomicBool,
) -> Result<()> {
    let path = backup_before_change(snapshot, plan, action, cancel)?;
    if plan.policy == FilePlanPolicy::MirrorFiles {
        let relative = snapshot
            .paths
            .get(&action.row_id)
            .ok_or_else(|| Error::Invalid("Missing mirror deletion path".into()))?;
        super::mirror::source_absent(snapshot, plan, relative)?;
        check_cancel(cancel)?;
    }
    fs::remove_file(&path)?;
    flush_directory(
        path.parent()
            .ok_or_else(|| Error::Invalid("Invalid deletion parent".into()))?,
    )?;
    if regular_digest(&path, &AtomicBool::new(false))?.is_some() {
        return Err(Error::Invalid(
            "Deletion target reappeared during verification".into(),
        ));
    }
    Ok(())
}
