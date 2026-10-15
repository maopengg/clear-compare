//! Reviewed rollback of journaled file copy/deletion, with backups and optimistic checks.
use crate::{
    Error, Result,
    compare::{check_cancel, checked_path, hash_file, reject_overlap},
    native_path,
    sync::{
        atomic_json, failure_message, flush_ancestors, flush_directory, permissions,
        set_permissions, validate_plan_metadata,
    },
    unique_id,
};
use clear_contracts::{ActionResult, RecoveryAction, RecoveryPreview, RecoveryResult, SyncResult};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
use tempfile::NamedTempFile;
const MAX_JOURNAL: u64 = 64 * 1024 * 1024;
struct Action {
    relative: PathBuf,
    current_digest: Option<String>,
    original_permissions: Option<clear_contracts::FilePermissions>,
    original_digest: Option<String>,
    original_backup: Option<PathBuf>,
}
pub struct Plan {
    pub view: RecoveryPreview,
    target: PathBuf,
    original_root: PathBuf,
    actions: Vec<Action>,
}
fn current(path: &Path, cancel: &AtomicBool) -> Result<Option<String>> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_file() && !m.file_type().is_symlink() => Ok(Some(hash_file(path, cancel)?)),
        Ok(_) => Err(Error::Invalid(
            "Recovery target is not a regular file".into(),
        )),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
fn root_valid(root: &Path) -> Result<()> {
    if !fs::symlink_metadata(root)?.is_dir() || fs::canonicalize(root)? != root {
        return Err(Error::Invalid("Recovery target root changed".into()));
    }
    Ok(())
}

pub fn preview(journal: &Path, backup_base: &Path, cancel: &AtomicBool) -> Result<Plan> {
    check_cancel(cancel)?;
    if !fs::symlink_metadata(journal)?.is_file()
        || fs::symlink_metadata(journal)?.file_type().is_symlink()
    {
        return Err(Error::Invalid("Select a regular journal file".into()));
    }
    let mut bytes = Vec::new();
    File::open(journal)?
        .take(MAX_JOURNAL + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_JOURNAL {
        return Err(Error::Invalid("Journal exceeds 64 MiB".into()));
    }
    let report: SyncResult = serde_json::from_slice(&bytes)?;
    validate_plan_metadata(&report.plan)?;
    if report.actions.len() != report.plan.actions.len() {
        return Err(Error::Invalid("Journal action counts differ".into()));
    }
    let target =
        native_path::decode(report.plan.target_native.as_ref().ok_or_else(|| {
            Error::Invalid("This older journal lacks durable native paths".into())
        })?)?;
    root_valid(&target)?;
    let source = fs::canonicalize(journal)?;
    let old_root = source
        .parent()
        .ok_or_else(|| Error::Invalid("Invalid journal path".into()))?;
    if old_root != Path::new(&report.plan.backup_root) {
        return Err(Error::Invalid(
            "Journal backup root does not match its location".into(),
        ));
    }
    reject_overlap(old_root, &target)?;
    let base = fs::canonicalize(backup_base)?;
    reject_overlap(&base, &target)?;
    if base.to_str().is_none() {
        return Err(Error::Invalid(
            "Recovery backup path must be Unicode".into(),
        ));
    }
    let mut actions = Vec::new();
    let mut labels = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for (copied, state) in report.plan.actions.iter().zip(&report.actions) {
        check_cancel(cancel)?;
        if copied.target_existed != copied.target_digest.is_some() {
            return Err(Error::Invalid(
                "Journal target-presence metadata differs".into(),
            ));
        }
        if copied.path != state.path {
            return Err(Error::Invalid("Journal action labels differ".into()));
        }
        if !matches!(state.state.as_str(), "completed" | "running" | "failed") {
            continue;
        }
        let relative = native_path::decode(
            copied
                .relative_native
                .as_ref()
                .ok_or_else(|| Error::Invalid("Missing durable entry path".into()))?,
        )?;
        if relative.as_os_str().is_empty()
            || relative
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err(Error::Invalid(
                "Recovery requires a nonempty relative file path".into(),
            ));
        }
        if !seen.insert(relative.clone()) {
            return Err(Error::Invalid("Duplicate journal target".into()));
        }
        let label = crate::tree::display_path(&relative);
        let path = checked_path(&target, &relative, true)?;
        let digest = current(&path, cancel)?;
        if digest == copied.target_digest && state.state != "completed" {
            continue;
        }
        let expected_current = match copied.operation {
            clear_contracts::FileOperation::Copy => copied.source_digest.clone(),
            clear_contracts::FileOperation::Delete => None,
        };
        if digest != expected_current {
            return Err(Error::Invalid(format!(
                "Recovery target changed: {}",
                label
            )));
        }
        let original_backup = if let Some(expected) = &copied.target_digest {
            let backup = checked_path(old_root, &PathBuf::from("files").join(&relative), false)?;
            if hash_file(&backup, cancel)? != *expected {
                return Err(Error::Invalid(format!(
                    "Original backup changed: {}",
                    label
                )));
            }
            if let Some(expected) = &copied.target_permissions
                && permissions(&fs::symlink_metadata(&backup)?) != *expected
            {
                return Err(Error::Invalid("Original backup permissions changed".into()));
            }
            Some(backup)
        } else {
            None
        };
        labels.push(RecoveryAction {
            path: label,
            operation: if expected_current.is_none() {
                "restore_deleted"
            } else if original_backup.is_some() {
                "restore"
            } else {
                "remove_created"
            }
            .into(),
        });
        actions.push(Action {
            relative,
            current_digest: expected_current,
            original_permissions: copied.target_permissions.clone(),
            original_digest: copied.target_digest.clone(),
            original_backup,
        });
    }
    if actions.is_empty() {
        return Err(Error::Invalid(
            "No applied file operations to recover".into(),
        ));
    }
    let id = unique_id();
    Ok(Plan {
        view: RecoveryPreview {
            id: id.clone(),
            source_journal: source.to_string_lossy().into(),
            target: target.to_string_lossy().into(),
            backup_root: base.join(id).to_string_lossy().into(),
            actions: labels,
        },
        target,
        original_root: old_root.to_path_buf(),
        actions,
    })
}
fn copy_to(source: &Path, destination: &mut File, cancel: &AtomicBool) -> Result<()> {
    let mut reader = File::open(source)?;
    let mut chunk = [0u8; 65536];
    loop {
        check_cancel(cancel)?;
        let n = reader.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        destination.write_all(&chunk[..n])?;
    }
    destination.sync_all()?;
    Ok(())
}
fn recover_one(plan: &Plan, index: usize, cancel: &AtomicBool) -> Result<()> {
    root_valid(&plan.target)?;
    let action = &plan.actions[index];
    let target = checked_path(&plan.target, &action.relative, true)?;
    if current(&target, cancel)? != action.current_digest {
        return Err(Error::Invalid(
            "Recovery target changed after preview".into(),
        ));
    }
    if let Some(expected) = &action.current_digest {
        let backup = checked_path(
            Path::new(&plan.view.backup_root),
            &PathBuf::from("files").join(&action.relative),
            true,
        )?;
        fs::create_dir_all(
            backup
                .parent()
                .ok_or_else(|| Error::Invalid("Invalid recovery backup".into()))?,
        )?;
        let mut writer = File::create_new(&backup)?;
        copy_to(&target, &mut writer, cancel)?;
        drop(writer);
        if hash_file(&backup, cancel)? != *expected {
            return Err(Error::Invalid("Recovery backup verification failed".into()));
        }
        flush_ancestors(
            backup
                .parent()
                .ok_or_else(|| Error::Invalid("Invalid recovery backup parent".into()))?,
            Path::new(&plan.view.backup_root),
        )?;
    }
    let replacement = if let Some(original) = &action.original_backup {
        root_valid(&plan.original_root)?;
        checked_path(
            &plan.original_root,
            &PathBuf::from("files").join(&action.relative),
            false,
        )?;
        if Some(hash_file(original, cancel)?) != action.original_digest {
            return Err(Error::Invalid(
                "Original backup changed after recovery preview".into(),
            ));
        }
        if let Some(expected) = &action.original_permissions
            && permissions(&fs::symlink_metadata(original)?) != *expected
        {
            return Err(Error::Invalid("Original backup permissions changed".into()));
        }
        let parent = target
            .parent()
            .ok_or_else(|| Error::Invalid("Invalid recovery parent".into()))?;
        fs::create_dir_all(parent)?;
        checked_path(&plan.target, &action.relative, true)?;
        let mut temp = NamedTempFile::new_in(parent)?;
        copy_to(original, temp.as_file_mut(), cancel)?;
        if let Some(saved) = &action.original_permissions {
            set_permissions(temp.as_file(), saved)?;
        } else {
            temp.as_file()
                .set_permissions(fs::metadata(&target)?.permissions())?;
        }
        temp.as_file().sync_all()?;
        if Some(hash_file(temp.path(), cancel)?) != action.original_digest {
            return Err(Error::Invalid(
                "Original backup changed during recovery".into(),
            ));
        }
        Some(temp)
    } else {
        None
    };
    check_cancel(cancel)?;
    root_valid(&plan.target)?;
    checked_path(&plan.target, &action.relative, true)?;
    if current(&target, cancel)? != action.current_digest {
        return Err(Error::Invalid(
            "Recovery target changed during backup".into(),
        ));
    }
    if let Some(temp) = replacement {
        if action.current_digest.is_some() {
            temp.persist(&target).map_err(|e| e.error)?;
        } else {
            temp.persist_noclobber(&target).map_err(|e| e.error)?;
        }
    } else {
        fs::remove_file(&target)?;
    }
    flush_ancestors(
        target
            .parent()
            .ok_or_else(|| Error::Invalid("Invalid recovery parent".into()))?,
        &plan.target,
    )?;
    if current(&target, &AtomicBool::new(false))? != action.original_digest {
        return Err(Error::Invalid(
            "Recovered target failed verification".into(),
        ));
    }
    Ok(())
}
pub fn execute(plan: &Plan, cancel: &AtomicBool) -> Result<RecoveryResult> {
    root_valid(&plan.target)?;
    let root = Path::new(&plan.view.backup_root);
    let base = root
        .parent()
        .ok_or_else(|| Error::Invalid("Invalid recovery backup root".into()))?;
    if fs::canonicalize(base)? != base {
        return Err(Error::Invalid("Recovery backup base changed".into()));
    }
    reject_overlap(base, &plan.target)?;
    fs::create_dir(root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
    }
    flush_directory(base)?;
    let journal = root.join("recovery.json");
    let mut result = RecoveryResult {
        plan: plan.view.clone(),
        actions: plan
            .view
            .actions
            .iter()
            .map(|a| ActionResult {
                path: a.path.clone(),
                state: "pending".into(),
                error: None,
            })
            .collect(),
        journal: journal.to_string_lossy().into(),
    };
    atomic_json(&journal, &result)?;
    for index in 0..plan.actions.len() {
        if let Err(e) = check_cancel(cancel) {
            result.actions[index].state = "cancelled".into();
            result.actions[index].error = Some(failure_message(&e));
            atomic_json(&journal, &result)?;
            break;
        }
        result.actions[index].state = "running".into();
        atomic_json(&journal, &result)?;
        match recover_one(plan, index, cancel) {
            Ok(()) => result.actions[index].state = "completed".into(),
            Err(e) => {
                result.actions[index].state = "failed".into();
                result.actions[index].error = Some(failure_message(&e));
            }
        }
        atomic_json(&journal, &result)?;
        if result.actions[index].state != "completed" {
            break;
        }
    }
    Ok(result)
}
