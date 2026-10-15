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
mod directory;
mod rename;
enum Action {
    File(FileUndo),
    Rename(rename::Undo),
    Directory(directory::Undo),
}
struct FileUndo {
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
    source_digest: String,
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
    preview_optional(journal, backup_base, cancel)?
        .ok_or_else(|| Error::Invalid("No applied file operations to recover".into()))
}
pub(crate) fn preview_optional(
    journal: &Path,
    backup_base: &Path,
    cancel: &AtomicBool,
) -> Result<Option<Plan>> {
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
    let source_digest = crate::sync::durable::digest(&bytes);
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
    let lock_path = checked_path(old_root, Path::new("operation.lock"), true)?;
    let _operation_lock = if lock_path.exists() {
        Some(crate::sync::durable::lock(old_root, false)?)
    } else {
        None
    };
    let (_, locked_hash): (SyncResult, String) = crate::sync::durable::read(&source, MAX_JOURNAL)?;
    if locked_hash != source_digest {
        return Err(Error::Invalid(
            "Operation journal changed while preparing recovery".into(),
        ));
    }
    let context = if let Some(expected) = &report.intent_digest {
        let (intent, hash): (crate::sync::durable::Intent, String) = crate::sync::durable::read(
            &checked_path(old_root, Path::new("intent.json"), false)?,
            crate::sync::durable::MAX_INTENT,
        )?;
        if &hash != expected
            || !crate::sync::durable::intent_matches(&report.plan, &intent.plan)
            || intent.pins.len() != report.plan.actions.len()
        {
            return Err(Error::Invalid("Recovery restart intent changed".into()));
        }
        Some(intent)
    } else {
        None
    };

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
    for (action_index, (copied, state)) in
        report.plan.actions.iter().zip(&report.actions).enumerate()
    {
        check_cancel(cancel)?;
        if copied.operation != clear_contracts::FileOperation::RemoveDirectory
            && copied.target_existed != copied.target_digest.is_some()
        {
            return Err(Error::Invalid(
                "Journal target-presence metadata differs".into(),
            ));
        }
        if copied.path != state.path {
            return Err(Error::Invalid("Journal action labels differ".into()));
        }
        if !matches!(
            state.state.as_str(),
            "completed" | "running" | "failed" | "cancelled"
        ) {
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
        let artifact_root = crate::sync::durable::action_root(&report.plan, copied)?;
        let label = crate::tree::display_path(&relative);
        if matches!(
            copied.operation,
            clear_contracts::FileOperation::CreateDirectory
                | clear_contracts::FileOperation::RemoveDirectory
        ) {
            if let Some(undo) =
                directory::preview(&target, &artifact_root, copied, &relative, &state.state)?
            {
                labels.push(RecoveryAction {
                    path: label,
                    operation: undo.operation().into(),
                });
                actions.push(Action::Directory(undo));
            }
            continue;
        }
        if copied.operation == clear_contracts::FileOperation::Rename {
            if let Some(undo) = rename::preview(
                &target,
                &artifact_root,
                &report.plan,
                copied,
                &state.state,
                cancel,
            )? {
                labels.push(RecoveryAction {
                    path: undo.label(),
                    operation: "rename_back".into(),
                });
                actions.push(Action::Rename(undo));
            }
            continue;
        }
        let path = checked_path(&target, &relative, true)?;
        let digest = current(&path, cancel)?;
        if digest == copied.target_digest && state.state != "completed" {
            continue;
        }
        let expected_current = match copied.operation {
            clear_contracts::FileOperation::Copy => copied.source_digest.clone(),
            clear_contracts::FileOperation::Delete => None,
            clear_contracts::FileOperation::Rename
            | clear_contracts::FileOperation::CreateDirectory
            | clear_contracts::FileOperation::RemoveDirectory => {
                return Err(Error::Invalid("Rename recovery was not dispatched".into()));
            }
        };
        if digest != expected_current {
            return Err(Error::Invalid(format!(
                "Recovery target changed: {}",
                label
            )));
        }
        let original_permissions = copied.target_permissions.clone().or_else(|| {
            context
                .as_ref()
                .and_then(|c| c.pins.get(action_index))
                .and_then(|p| p.target.as_ref())
                .map(|p| p.permissions.clone())
        });
        let original_backup = if let Some(expected) = &copied.target_digest {
            let backup = checked_path(
                &artifact_root,
                &PathBuf::from("files").join(&relative),
                false,
            )?;
            if hash_file(&backup, cancel)? != *expected {
                return Err(Error::Invalid(format!(
                    "Original backup changed: {}",
                    label
                )));
            }
            if let Some(expected) = &original_permissions
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
        actions.push(Action::File(FileUndo {
            relative,
            current_digest: expected_current,
            original_permissions,
            original_digest: copied.target_digest.clone(),
            original_backup,
        }));
    }
    if actions.is_empty() {
        return Ok(None);
    }
    if report.plan.version >= 5 {
        actions.reverse();
        labels.reverse();
        directory::validate_cleanup(&target, &actions, cancel)?;
    }
    let id = unique_id();
    Ok(Some(Plan {
        view: RecoveryPreview {
            id: id.clone(),
            source_journal: source.to_string_lossy().into(),
            target: target.to_string_lossy().into(),
            backup_root: base.join(id).to_string_lossy().into(),
            actions: labels,
        },
        target,
        original_root: old_root.to_path_buf(),
        source_digest,
        actions,
    }))
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
        crate::task::read(n);
        destination.write_all(&chunk[..n])?;
        crate::task::written(n);
    }
    destination.sync_all()?;
    Ok(())
}
fn recover_one(plan: &Plan, index: usize, cancel: &AtomicBool) -> Result<()> {
    root_valid(&plan.target)?;
    let action = match &plan.actions[index] {
        Action::File(action) => action,
        Action::Directory(action) => {
            return action.execute(
                &plan.target,
                &plan.original_root,
                Path::new(&plan.view.backup_root),
                cancel,
            );
        }
        Action::Rename(action) => {
            return action.execute(
                &plan.target,
                &plan.original_root,
                Path::new(&plan.view.backup_root),
                cancel,
            );
        }
    };
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
            original
                .strip_prefix(&plan.original_root)
                .map_err(|_| Error::Invalid("Original backup escaped its root".into()))?,
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
    let _mutation = crate::task::mutation(cancel)?;
    crate::task::stage("revalidate_and_restore");
    root_valid(&plan.target)?;
    checked_path(&plan.target, &action.relative, true)?;
    if current(&target, cancel)? != action.current_digest {
        return Err(Error::Invalid(
            "Recovery target changed during backup".into(),
        ));
    }
    if let Some(temp) = replacement.as_ref()
        && Some(hash_file(temp.path(), cancel)?) != action.original_digest
    {
        return Err(Error::Invalid(
            "Prepared restoration changed before commit".into(),
        ));
    }
    if let Some(original) = &action.original_backup {
        root_valid(&plan.original_root)?;
        checked_path(
            &plan.original_root,
            original
                .strip_prefix(&plan.original_root)
                .map_err(|_| Error::Invalid("Original backup escaped its root".into()))?,
            false,
        )?;
        if Some(hash_file(original, cancel)?) != action.original_digest {
            return Err(Error::Invalid(
                "Original restoration backup changed before commit".into(),
            ));
        }
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
    let lock_path = checked_path(&plan.original_root, Path::new("operation.lock"), true)?;
    let _operation_lock = if lock_path.exists() {
        Some(crate::sync::durable::lock(&plan.original_root, false)?)
    } else {
        None
    };
    let (_, hash): (SyncResult, String) =
        crate::sync::durable::read(Path::new(&plan.view.source_journal), MAX_JOURNAL)?;
    if hash != plan.source_digest {
        return Err(Error::Invalid(
            "Operation journal changed after recovery preview".into(),
        ));
    }

    let setup = crate::task::validated_mutation(cancel, || {
        root_valid(&plan.target)?;
        directory::validate_cleanup(&plan.target, &plan.actions, cancel)
    })?;
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
    drop(setup);
    for index in 0..plan.actions.len() {
        crate::task::action(&plan.view.actions[index].path);
        if let Err(e) = check_cancel(cancel) {
            result.actions[index].state = "cancelled".into();
            result.actions[index].error = Some(failure_message(&e));
            atomic_json(&journal, &result)?;
            break;
        }
        result.actions[index].state = "running".into();
        atomic_json(&journal, &result)?;
        match recover_one(plan, index, cancel) {
            Ok(()) => {
                result.actions[index].state = "completed".into();
                crate::task::completed();
            }
            Err(e) => {
                result.actions[index].state = if matches!(e, Error::Cancelled) {
                    "cancelled"
                } else {
                    "failed"
                }
                .into();
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

impl Plan {
    pub fn affected_paths(&self) -> Vec<PathBuf> {
        self.actions
            .iter()
            .flat_map(|action| match action {
                Action::File(a) => vec![self.target.join(&a.relative)],
                Action::Rename(a) => a.affected(&self.target),
                Action::Directory(a) => vec![self.target.join(&a.relative)],
            })
            .collect()
    }
    pub fn relocations(&self, result: &RecoveryResult) -> Vec<crate::editing::Relocation> {
        self.actions
            .iter()
            .zip(&result.actions)
            .flat_map(|(action, state)| match action {
                Action::Rename(a) if state.state == "completed" => a.relocations(&self.target),
                _ => Vec::new(),
            })
            .collect()
    }
}
