use crate::{
    Error, Result,
    compare::{Snapshot, check_cancel, checked_path, hash_file, reject_overlap},
    unique_id,
};
use clear_contracts::{ActionResult, CopyAction, Kind, Status, SyncPlan, SyncResult};
use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
use tempfile::NamedTempFile;

fn regular_digest(path: &Path, cancel: &AtomicBool) -> Result<Option<String>> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {
            Ok(Some(hash_file(path, cancel)?))
        }
        Ok(_) => Err(Error::Invalid("Destination is not a regular file".into())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

pub fn plan(
    snapshot: &Snapshot,
    row_ids: &[String],
    backup_base: &Path,
    cancel: &AtomicBool,
) -> Result<SyncPlan> {
    if row_ids.is_empty() {
        return Err(Error::Invalid("Select files to synchronize".into()));
    }
    if !backup_base.is_dir() {
        return Err(Error::Invalid(
            "Backup base must be an existing directory".into(),
        ));
    }
    let base = fs::canonicalize(backup_base)?;
    reject_overlap(&base, &snapshot.left)?;
    reject_overlap(&base, &snapshot.right)?;
    let id = unique_id();
    let mut actions = Vec::new();
    let mut seen = BTreeSet::new();
    for row_id in row_ids {
        check_cancel(cancel)?;
        if !seen.insert(row_id) {
            continue;
        }
        let relative = snapshot
            .paths
            .get(row_id)
            .ok_or_else(|| Error::Invalid("Unknown entry".into()))?;
        let row = snapshot
            .report
            .rows
            .iter()
            .find(|r| &r.id == row_id)
            .ok_or_else(|| Error::Invalid("Unknown entry".into()))?;
        if !matches!(row.status, Status::Changed | Status::LeftOnly)
            || !row
                .left
                .as_ref()
                .is_some_and(|e| e.kind == Kind::File && e.error.is_none())
        {
            return Err(Error::Invalid(format!(
                "Not eligible for left-to-right copy: {}",
                row.path
            )));
        }
        if snapshot.rules.excludes(relative, false) {
            return Err(Error::Invalid("Excluded entry".into()));
        }
        let source = checked_path(&snapshot.left, relative, false)?;
        let target = checked_path(&snapshot.right, relative, true)?;
        let source_digest = hash_file(&source, cancel)?;
        let target_digest = regular_digest(&target, cancel)?;
        if row
            .left
            .as_ref()
            .and_then(|e| e.digest.as_ref())
            .is_some_and(|d| d != &source_digest)
            || row
                .right
                .as_ref()
                .and_then(|e| e.digest.as_ref())
                .is_some_and(|d| Some(d) != target_digest.as_ref())
            || (row.right.is_none() && target_digest.is_some())
        {
            return Err(Error::Invalid(
                "Files changed since comparison. Compare again.".into(),
            ));
        }
        actions.push(CopyAction {
            row_id: row_id.clone(),
            path: row.path.clone(),
            overwrite: target_digest.is_some(),
            bytes: fs::metadata(source)?.len().to_string(),
            source_digest,
            target_digest,
        });
    }
    Ok(SyncPlan {
        id: id.clone(),
        session: snapshot.report.session.clone(),
        actions,
        backup_root: base.join(id).to_string_lossy().into_owned(),
    })
}

fn atomic_json(path: &Path, data: &SyncResult) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Invalid("Invalid journal path".into()))?;
    let mut temp = NamedTempFile::new_in(parent)?;
    temp.write_all(&serde_json::to_vec_pretty(data)?)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

fn roots_unchanged(snapshot: &Snapshot) -> Result<()> {
    if fs::canonicalize(&snapshot.left)? != snapshot.left
        || fs::canonicalize(&snapshot.right)? != snapshot.right
    {
        return Err(Error::Invalid(
            "Directory identity changed. Compare again.".into(),
        ));
    }
    Ok(())
}

fn copy_one(
    snapshot: &Snapshot,
    plan: &SyncPlan,
    action: &CopyAction,
    cancel: &AtomicBool,
) -> Result<()> {
    check_cancel(cancel)?;
    roots_unchanged(snapshot)?;
    let relative = snapshot
        .paths
        .get(&action.row_id)
        .ok_or_else(|| Error::Invalid("Unknown entry".into()))?;
    if snapshot.rules.excludes(relative, false) {
        return Err(Error::Invalid("Excluded entry".into()));
    }
    let source = checked_path(&snapshot.left, relative, false)?;
    let destination = checked_path(&snapshot.right, relative, true)?;
    if hash_file(&source, cancel)? != action.source_digest
        || regular_digest(&destination, cancel)? != action.target_digest
    {
        return Err(Error::Invalid(
            "Source or target changed after preview. Compare again.".into(),
        ));
    }
    let parent = destination
        .parent()
        .ok_or_else(|| Error::Invalid("Missing parent".into()))?;
    fs::create_dir_all(parent)?;
    checked_path(&snapshot.right, relative, true)?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    // Copy in bounded chunks with cancellation, rather than reading the file into memory.
    let mut reader = File::open(&source)?;
    let mut chunk = [0u8; 65536];
    use std::io::Read;
    loop {
        check_cancel(cancel)?;
        let n = reader.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        temporary.write_all(&chunk[..n])?;
    }
    temporary
        .as_file()
        .set_permissions(fs::metadata(&source)?.permissions())?;
    temporary.as_file().sync_all()?;
    if hash_file(temporary.path(), cancel)? != action.source_digest {
        return Err(Error::Invalid("Source changed during copy".into()));
    }
    if let Some(expected) = &action.target_digest {
        let backup_root = Path::new(&plan.backup_root);
        let backup = checked_path(backup_root, &PathBuf::from("files").join(relative), true)?;
        fs::create_dir_all(
            backup
                .parent()
                .ok_or_else(|| Error::Invalid("Invalid backup path".into()))?,
        )?;
        // FlushFileBuffers on Windows requires a writable handle. Keep the
        // backup writer open until durable flush instead of reopening read-only.
        let mut backup_writer = File::create_new(&backup)?;
        let mut old = File::open(&destination)?;
        loop {
            check_cancel(cancel)?;
            let n = old.read(&mut chunk)?;
            if n == 0 {
                break;
            }
            backup_writer.write_all(&chunk[..n])?;
        }
        backup_writer.sync_all()?;
        drop(backup_writer);
        if hash_file(&backup, cancel)? != *expected {
            return Err(Error::Invalid("Backup verification failed".into()));
        }
    }
    check_cancel(cancel)?;
    roots_unchanged(snapshot)?;
    checked_path(&snapshot.right, relative, true)?;
    if regular_digest(&destination, cancel)? != action.target_digest {
        return Err(Error::Invalid("Destination changed during copy".into()));
    }
    if action.overwrite {
        temporary.persist(&destination).map_err(|e| e.error)?;
    } else {
        temporary
            .persist_noclobber(&destination)
            .map_err(|e| e.error)?;
    }
    // Verification is mandatory even after a successful filesystem replacement.
    // Cancellation after replacement must not misreport a completed write.
    if hash_file(&destination, &AtomicBool::new(false))? != action.source_digest {
        return Err(Error::Invalid("Copied file failed verification".into()));
    }
    Ok(())
}

pub fn execute(snapshot: &Snapshot, plan: &SyncPlan, cancel: &AtomicBool) -> Result<SyncResult> {
    if plan.session != snapshot.report.session {
        return Err(Error::Invalid("Plan belongs to another comparison".into()));
    }
    roots_unchanged(snapshot)?;
    let backup = Path::new(&plan.backup_root);
    let base = backup
        .parent()
        .ok_or_else(|| Error::Invalid("Invalid backup root".into()))?;
    if fs::canonicalize(base)? != base {
        return Err(Error::Invalid("Backup base changed after preview".into()));
    }
    reject_overlap(base, &snapshot.left)?;
    reject_overlap(base, &snapshot.right)?;
    // A unique plan directory is never reused: it protects journal and backup ownership.
    fs::create_dir(backup)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(backup, fs::Permissions::from_mode(0o700))?;
    }
    let journal = backup.join("journal.json");
    let mut result = SyncResult {
        plan: plan.clone(),
        actions: plan
            .actions
            .iter()
            .map(|a| ActionResult {
                path: a.path.clone(),
                state: "pending".into(),
                error: None,
            })
            .collect(),
        journal: journal.to_string_lossy().into_owned(),
    };
    atomic_json(&journal, &result)?;
    for (i, action) in plan.actions.iter().enumerate() {
        if let Err(e) = check_cancel(cancel) {
            result.actions[i].state = "cancelled".into();
            result.actions[i].error = Some(e.to_string());
            atomic_json(&journal, &result)?;
            break;
        }
        result.actions[i].state = "running".into();
        atomic_json(&journal, &result)?;
        match copy_one(snapshot, plan, action, cancel) {
            Ok(()) => result.actions[i].state = "completed".into(),
            Err(e) => {
                result.actions[i].state = "failed".into();
                result.actions[i].error = Some(e.to_string());
            }
        }
        atomic_json(&journal, &result)?;
        if result.actions[i].state != "completed" {
            break;
        }
    }
    Ok(result)
}
