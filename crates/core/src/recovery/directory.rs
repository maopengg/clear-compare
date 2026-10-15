//! Reverse directory actions in dependency order, requiring durable ownership.
use super::*;
use crate::sync::directory::{self as native, Record};
use clear_contracts::{FileAction, FileOperation};
pub(super) struct Undo {
    pub relative: PathBuf,
    remove: bool,
    record: Record,
}
impl Undo {
    pub fn operation(&self) -> &'static str {
        if self.remove {
            "remove_created_directory"
        } else {
            "restore_removed_directory"
        }
    }
    pub fn execute(
        &self,
        root: &Path,
        old_root: &Path,
        backup: &Path,
        cancel: &AtomicBool,
    ) -> Result<()> {
        let _mutation = crate::task::mutation(cancel)?;
        crate::task::stage("directory_recovery");
        root_valid(root)?;
        let original = Record::read(old_root, &self.relative, self.remove)?;
        if original != self.record {
            return Err(Error::Invalid(
                "Directory recovery record changed after preview".into(),
            ));
        }
        let path = checked_path(root, &self.relative, true)?;
        let parent = path
            .parent()
            .ok_or_else(|| Error::Invalid("Directory recovery parent missing".into()))?;
        if self.remove {
            if native::observe(root, &self.relative)?.as_ref() != Some(&self.record) {
                return Err(Error::Invalid(
                    "Created directory identity or permissions changed".into(),
                ));
            }
            native::empty(&path)?;
            self.record.write(backup, &self.relative, false)?;
            check_cancel(cancel)?;
            root_valid(root)?;
            if native::observe(root, &self.relative)?.as_ref() != Some(&self.record) {
                return Err(Error::Invalid(
                    "Directory changed during recovery backup".into(),
                ));
            }
            native::empty(&path)?;
            fs::remove_dir(&path)?;
            flush_directory(parent)?;
            if native::observe(root, &self.relative)?.is_some() {
                return Err(Error::Invalid(
                    "Removed created-directory path was recreated".into(),
                ));
            }
        } else {
            if native::observe(root, &self.relative)?.is_some() {
                return Err(Error::Invalid(
                    "Removed directory path has been recreated; it will not be overwritten".into(),
                ));
            }
            // Preserve the verified original metadata in this recovery set as well.
            self.record.write(backup, &self.relative, false)?;
            check_cancel(cancel)?;
            fs::create_dir(&path)?;
            native::restore_permissions(&path, &self.record.permissions)?;
            flush_directory(&path)?;
            flush_directory(parent)?;
            let created = native::observe(root, &self.relative)?
                .ok_or_else(|| Error::Invalid("Restored directory disappeared".into()))?;
            if created.permissions != self.record.permissions {
                return Err(Error::Invalid(
                    "Restored directory permissions differ".into(),
                ));
            }
            created.write(backup, &self.relative, true)?;
        }
        Ok(())
    }
}
pub(super) fn preview(
    root: &Path,
    old_root: &Path,
    action: &FileAction,
    relative: &Path,
    state: &str,
) -> Result<Option<Undo>> {
    let current = native::observe(root, relative)?;
    let remove = action.operation == FileOperation::CreateDirectory;
    if remove {
        let Some(current) = current else {
            if state != "completed" {
                return Ok(None);
            }
            return Err(Error::Invalid("Created directory is missing".into()));
        };
        let record = Record::read(old_root, relative, true).map_err(|e| {
            Error::Invalid(format!(
                "Cannot prove created directory ownership; retain it for manual review: {e}"
            ))
        })?;
        if current != record {
            return Err(Error::Invalid(
                "Created directory was replaced or its permissions changed".into(),
            ));
        }
        Ok(Some(Undo {
            relative: relative.to_owned(),
            remove,
            record,
        }))
    } else {
        if let Some(current) = current {
            if state != "completed"
                && action.directory_identity.as_ref() == Some(&current.identity)
                && action.target_permissions.as_ref() == Some(&current.permissions)
            {
                return Ok(None);
            }
            return Err(Error::Invalid(
                "Original removed-directory path is occupied".into(),
            ));
        }
        let record = Record::read(old_root, relative, false)?;
        if action.directory_identity.as_ref() != Some(&record.identity)
            || action.target_permissions.as_ref() != Some(&record.permissions)
        {
            return Err(Error::Invalid(
                "Original directory metadata backup differs from its plan".into(),
            ));
        }
        Ok(Some(Undo {
            relative: relative.to_owned(),
            remove,
            record,
        }))
    }
}
pub(super) fn validate_cleanup(root: &Path, actions: &[Action], cancel: &AtomicBool) -> Result<()> {
    let removable = actions
        .iter()
        .filter_map(|a| match a {
            Action::Directory(d) if d.remove => Some(d.relative.clone()),
            Action::File(f) if f.original_digest.is_none() && f.current_digest.is_some() => {
                Some(f.relative.clone())
            }
            _ => None,
        })
        .collect::<std::collections::BTreeSet<_>>();
    let mut visited = 0;
    for action in actions {
        if let Action::Directory(directory) = action
            && !directory.remove
            && native::observe(root, &directory.relative)?.is_some()
        {
            return Err(Error::Invalid(
                "Removed-directory recovery path is now occupied".into(),
            ));
        }
        if let Action::Directory(directory) = action
            && directory.remove
        {
            if native::observe(root, &directory.relative)?.as_ref() != Some(&directory.record) {
                return Err(Error::Invalid(
                    "Created directory identity or permissions changed during recovery preview"
                        .into(),
                ));
            }
            let path = checked_path(root, &directory.relative, false)?;
            for entry in fs::read_dir(path)? {
                check_cancel(cancel)?;
                visited += 1;
                if visited > crate::compare::MAX_ENTRIES {
                    return Err(Error::Invalid(
                        "Recovery directory preflight exceeds 200000 entries".into(),
                    ));
                }
                let child = directory.relative.join(entry?.file_name());
                if !removable.contains(&child) {
                    return Err(Error::Invalid(
                        "External entries block removal of a created directory during recovery"
                            .into(),
                    ));
                }
            }
        }
    }
    Ok(())
}
