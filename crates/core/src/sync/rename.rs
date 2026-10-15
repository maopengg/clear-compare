//! Journaled same-directory rename, including case-only names on insensitive filesystems.
use super::*;
use crate::{native_path, no_replace};
use clear_contracts::RenamePaths;
use std::ffi::OsStr;
fn relative(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|p| matches!(p, std::path::Component::Normal(_)))
}
pub(crate) fn validated_paths(
    plan: &SyncPlan,
    action: &FileAction,
) -> Result<(PathBuf, PathBuf, PathBuf)> {
    if plan.id.is_empty()
        || plan.id.len() > 96
        || !plan.id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
    {
        return Err(Error::Invalid("Invalid rename plan identity".into()));
    }
    let old = native_path::decode(
        action
            .relative_native
            .as_ref()
            .ok_or_else(|| Error::Invalid("Rename source path missing".into()))?,
    )?;
    let spec = action
        .rename_to
        .as_ref()
        .ok_or_else(|| Error::Invalid("Rename paths missing".into()))?;
    let new = native_path::decode(&spec.destination_native)?;
    let temporary = native_path::decode(&spec.temporary_native)?;
    if !relative(&old)
        || !relative(&new)
        || !relative(&temporary)
        || old.parent() != new.parent()
        || old.parent() != temporary.parent()
        || old == new
        || old == temporary
        || new == temporary
    {
        return Err(Error::Invalid(
            "Rename requires distinct file names within one directory".into(),
        ));
    }
    native_path::single_name(
        new.file_name()
            .ok_or_else(|| Error::Invalid("Rename name missing".into()))?,
    )?;
    if temporary
        != old
            .parent()
            .unwrap_or(Path::new(""))
            .join(format!(".clear-compare-rename-{}", plan.id))
    {
        return Err(Error::Invalid(
            "Rename temporary path does not match its plan".into(),
        ));
    }
    Ok((old, new, temporary))
}
pub(crate) use crate::native_path::exact_leaf as exact_exists;
pub(crate) fn absent(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
        Ok(_) => Err(Error::Invalid(
            "Rename destination or temporary path already exists".into(),
        )),
    }
}
fn destination_available(root: &Path, old: &Path, new: &Path, cancel: &AtomicBool) -> Result<()> {
    let source = checked_path(root, old, false)?;
    let destination = checked_path(root, new, true)?;
    match fs::symlink_metadata(&destination) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
        Ok(meta)
            if meta.is_file()
                && !exact_exists(root, new, cancel)?
                && same_file::is_same_file(&source, &destination)? =>
        {
            Ok(())
        }
        Ok(_) => Err(Error::Invalid(
            "Rename does not overwrite an existing name".into(),
        )),
    }
}
pub fn plan_rename(
    snapshot: &Snapshot,
    row_id: &str,
    backup_base: &Path,
    cancel: &AtomicBool,
    target_side: Side,
    new_name: &OsStr,
) -> Result<SyncPlan> {
    native_path::single_name(new_name)?;
    let mut plan = super::plan_delete(
        snapshot,
        &[row_id.to_owned()],
        backup_base,
        cancel,
        target_side.clone(),
    )?;
    let old = native_path::decode(
        plan.actions[0]
            .relative_native
            .as_ref()
            .ok_or_else(|| Error::Invalid("Rename source path missing".into()))?,
    )?;
    let parent = old
        .parent()
        .ok_or_else(|| Error::Invalid("Rename parent missing".into()))?;
    let new = parent.join(new_name);
    let temporary = parent.join(format!(".clear-compare-rename-{}", plan.id));
    plan.version = 3;
    plan.policy = FilePlanPolicy::RenameSelected { target_side };
    let action = &mut plan.actions[0];
    action.operation = FileOperation::Rename;
    action.source_digest = action.target_digest.clone();
    action.rename_to = Some(RenamePaths {
        destination: crate::tree::display_path(&new),
        destination_native: native_path::encode(&new),
        temporary_native: native_path::encode(&temporary),
    });
    validate_plan(&plan)?;
    let root = target_root(snapshot, &plan);
    if snapshot.rules.excludes(&new, false) {
        return Err(Error::Invalid(
            "Rename destination is excluded by the active rules".into(),
        ));
    }
    if !exact_exists(root, &old, cancel)? {
        return Err(Error::Invalid(
            "Source filename changed. Compare again.".into(),
        ));
    }
    destination_available(root, &old, &new, cancel)?;
    absent(&checked_path(root, &temporary, true)?)?;
    Ok(plan)
}
pub(super) fn rename_one(
    snapshot: &Snapshot,
    plan: &SyncPlan,
    action: &FileAction,
    cancel: &AtomicBool,
) -> Result<()> {
    let (old, new, temporary) = validated_paths(plan, action)?;
    let root = target_root(snapshot, plan);
    if !exact_exists(root, &old, cancel)? {
        return Err(Error::Invalid(
            "Source filename changed after preview".into(),
        ));
    }
    destination_available(root, &old, &new, cancel)?;
    let stage = checked_path(root, &temporary, true)?;
    absent(&stage)?;
    let source = super::delete::backup_before_change(snapshot, plan, action, cancel)?;
    let _mutation = crate::task::mutation(cancel)?;
    crate::task::stage("revalidate_and_rename");
    checked_path(root, &old, false)?;
    if fs::canonicalize(root)? != root
        || Some(hash_file(&source, cancel)?) != action.target_digest
        || action.target_permissions.as_ref() != Some(&permissions(&fs::symlink_metadata(&source)?))
    {
        return Err(Error::Invalid("Rename source changed before commit".into()));
    }

    let backup = checked_path(
        Path::new(&plan.backup_root),
        &PathBuf::from("files").join(&old),
        false,
    )?;
    if Some(hash_file(&backup, cancel)?) != action.target_digest
        || action.target_permissions.as_ref() != Some(&permissions(&fs::symlink_metadata(&backup)?))
    {
        return Err(Error::Invalid("Rename backup changed before commit".into()));
    }
    destination_available(root, &old, &new, cancel)?;
    absent(&stage)?;
    check_cancel(cancel)?;
    if !exact_exists(root, &old, cancel)? {
        return Err(Error::Invalid(
            "Source filename changed during backup".into(),
        ));
    }
    no_replace::rename(&source, &stage)?;
    flush_directory(
        stage
            .parent()
            .ok_or_else(|| Error::Invalid("Rename parent missing".into()))?,
    )?;
    check_cancel(cancel)?;
    let expected = action
        .target_digest
        .as_deref()
        .ok_or_else(|| Error::Invalid("Rename digest missing".into()))?;
    let saved = action
        .target_permissions
        .as_ref()
        .ok_or_else(|| Error::Invalid("Rename permissions missing".into()))?;
    checked_path(root, &temporary, false)?;
    if hash_file(&stage, cancel)? != expected
        || permissions(&fs::symlink_metadata(&stage)?) != *saved
    {
        return Err(Error::Invalid("Staged rename source changed".into()));
    }
    let destination = checked_path(root, &new, true)?;
    absent(&destination)?;
    no_replace::rename(&stage, &destination)?;
    flush_directory(
        destination
            .parent()
            .ok_or_else(|| Error::Invalid("Rename parent missing".into()))?,
    )?;
    let settled = AtomicBool::new(false);
    if exact_exists(root, &old, &settled)?
        || exact_exists(root, &temporary, &settled)?
        || !exact_exists(root, &new, &settled)?
        || hash_file(&destination, &AtomicBool::new(false))? != expected
        || permissions(&fs::symlink_metadata(&destination)?) != *saved
    {
        return Err(Error::Invalid(
            "Renamed result could not be verified".into(),
        ));
    }
    Ok(())
}
