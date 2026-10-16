use crate::{
    Error, Result,
    compare::{Snapshot, check_cancel, checked_path, hash_file, reject_overlap},
    unique_id,
};
use clear_contracts::{
    ActionResult, FileAction, FileOperation, FilePermissions, FilePlanPolicy, Kind, Side, Status,
    SyncPlan, SyncResult,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
use tempfile::NamedTempFile;

mod delete;
pub(crate) mod directory;
pub(crate) mod durable;
mod mirror;
mod mirror_tree;
pub(crate) mod rename;
pub mod resume;
pub use delete::plan_delete;
pub(crate) use delete::{permissions, set_permissions};
pub use mirror::plan_mirror_files;
pub use mirror_tree::plan_mirror_tree;
pub use rename::plan_rename;
const MAX_PREVIEW_JOURNAL: usize = 32 * 1024 * 1024;
fn initial_result(plan: &SyncPlan) -> SyncResult {
    SyncResult {
        intent_digest: None,
        attempts: Vec::new(),
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
        journal: Path::new(&plan.backup_root)
            .join("journal.json")
            .to_string_lossy()
            .into(),
    }
}
pub(crate) fn validate_plan_metadata(plan: &SyncPlan) -> Result<()> {
    if let FilePlanPolicy::Reconcile { target_side } = &plan.policy
        && *target_side == plan.source_side
    {
        return Err(Error::Invalid(
            "Reconciliation direction contradicts its target".into(),
        ));
    }
    if !matches!(plan.version, 1..=6)
        || plan.actions.is_empty()
        || plan.actions.len() > crate::compare::MAX_ENTRIES
    {
        return Err(Error::Invalid(
            "Unsupported or empty file-operation plan".into(),
        ));
    }
    let digest = |s: &str| s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit());
    let mut seen = BTreeSet::new();
    for action in &plan.actions {
        if let Some(namespace) = &action.backup_namespace {
            durable::namespace(namespace)?;
        }
        if !seen.insert(&action.row_id)
            || action.operation != FileOperation::RemoveDirectory
                && action.directory_identity.is_some()
            || action.operation != FileOperation::RemoveDirectory
                && action.target_existed != action.target_digest.is_some()
            || action.bytes.parse::<u64>().is_err()
            || action.target_digest.as_deref().is_some_and(|s| !digest(s))
        {
            return Err(Error::Invalid(
                "Invalid file-action metadata or duplicate entry".into(),
            ));
        }
        match (&plan.policy, action.operation) {
            (
                FilePlanPolicy::SelectedCopy
                | FilePlanPolicy::MirrorFiles
                | FilePlanPolicy::MirrorTree
                | FilePlanPolicy::Reconcile { .. },
                FileOperation::Copy,
            ) if (plan.policy == FilePlanPolicy::SelectedCopy && matches!(plan.version, 1 | 2)
                || plan.policy == FilePlanPolicy::MirrorFiles && plan.version == 4
                || plan.policy == FilePlanPolicy::MirrorTree && plan.version == 5
                || matches!(plan.policy, FilePlanPolicy::Reconcile { .. })
                    && plan.version == 6)
                && action.source_digest.as_deref().is_some_and(digest)
                && action.target_permissions.is_none()
                && action.rename_to.is_none() => {}
            (
                FilePlanPolicy::DeleteSelected { .. }
                | FilePlanPolicy::MirrorFiles
                | FilePlanPolicy::MirrorTree
                | FilePlanPolicy::Reconcile { .. },
                FileOperation::Delete,
            ) if (plan.version == 2
                && matches!(plan.policy, FilePlanPolicy::DeleteSelected { .. })
                || plan.version == 4 && plan.policy == FilePlanPolicy::MirrorFiles
                || plan.version == 5 && plan.policy == FilePlanPolicy::MirrorTree
                || plan.version == 6
                    && matches!(plan.policy, FilePlanPolicy::Reconcile { .. }))
                && action.source_digest.is_none()
                && action.target_existed
                && action.target_permissions.is_some()
                && action.rename_to.is_none() => {}
            (FilePlanPolicy::RenameSelected { .. }, FileOperation::Rename)
                if plan.version == 3
                    && plan.actions.len() == 1
                    && action.target_existed
                    && action.source_digest == action.target_digest
                    && action.source_digest.as_deref().is_some_and(digest)
                    && action.target_permissions.is_some()
                    && action.rename_to.is_some() =>
            {
                rename::validated_paths(plan, action)?;
            }
            (
                FilePlanPolicy::MirrorTree | FilePlanPolicy::Reconcile { .. },
                FileOperation::CreateDirectory | FileOperation::RemoveDirectory,
            ) if (plan.version == 5 && plan.policy == FilePlanPolicy::MirrorTree
                || plan.version == 6
                    && matches!(plan.policy, FilePlanPolicy::Reconcile { .. }))
                && action.rename_to.is_none()
                && action.source_digest.is_none()
                && action.target_digest.is_none()
                && action.bytes == "0"
                && (action.operation == FileOperation::CreateDirectory
                    && !action.target_existed
                    && action.target_permissions.is_none()
                    || action.operation == FileOperation::RemoveDirectory
                        && action.target_existed
                        && action.target_permissions.is_some()
                        && action.directory_identity.is_some()) =>
            {
                if let Some(identity) = &action.directory_identity {
                    directory::validate_identity(identity)?;
                }
            }
            _ => {
                return Err(Error::Invalid(
                    "File action does not match its reviewed policy".into(),
                ));
            }
        }
        if action
            .target_permissions
            .as_ref()
            .is_some_and(|p| matches!(p,FilePermissions::Unix{mode} if *mode>0o7777))
        {
            return Err(Error::Invalid("Invalid recorded permission bits".into()));
        }
    }
    Ok(())
}
pub(crate) fn validate_plan(plan: &SyncPlan) -> Result<()> {
    validate_plan_metadata(plan)?;
    struct Bound(usize);
    impl Write for Bound {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() > MAX_PREVIEW_JOURNAL.saturating_sub(self.0) {
                return Err(std::io::Error::other(
                    "File-operation preview journal exceeds 32 MiB",
                ));
            }
            self.0 += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut sized = initial_result(plan);
    sized.intent_digest = Some("0".repeat(64));
    serde_json::to_writer_pretty(Bound(0), &sized)
        .map_err(|_| Error::Invalid("File-operation preview journal exceeds 32 MiB".into()))?;
    Ok(())
}
fn target_root<'a>(snapshot: &'a Snapshot, plan: &SyncPlan) -> &'a Path {
    match &plan.policy {
        FilePlanPolicy::SelectedCopy | FilePlanPolicy::MirrorFiles | FilePlanPolicy::MirrorTree => {
            if plan.source_side == Side::Left {
                &snapshot.right
            } else {
                &snapshot.left
            }
        }
        FilePlanPolicy::DeleteSelected { target_side }
        | FilePlanPolicy::RenameSelected { target_side }
        | FilePlanPolicy::Reconcile { target_side } => {
            if *target_side == Side::Left {
                &snapshot.left
            } else {
                &snapshot.right
            }
        }
    }
}
pub(crate) fn failure_message(error: &Error) -> String {
    let mut value = error.to_string();
    if value.len() > 65536 {
        let mut end = 65536;
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        value.truncate(end);
        value.push_str(" [truncated]");
    }
    value
}

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
    plan_direction(snapshot, row_ids, backup_base, cancel, Side::Left)
}

pub fn plan_direction(
    snapshot: &Snapshot,
    row_ids: &[String],
    backup_base: &Path,
    cancel: &AtomicBool,
    source_side: Side,
) -> Result<SyncPlan> {
    let (source_root, target_root) = if source_side == Side::Left {
        (&snapshot.left, &snapshot.right)
    } else {
        (&snapshot.right, &snapshot.left)
    };
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
    if base.to_str().is_none() {
        return Err(Error::Invalid(
            "Backup directory must have a Unicode path".into(),
        ));
    }
    let id = unique_id();
    let mut actions = Vec::new();
    let mut seen = BTreeSet::new();
    let requested = row_ids.iter().map(String::as_str).collect::<BTreeSet<_>>();
    let rows = snapshot
        .report
        .rows
        .iter()
        .filter(|r| requested.contains(r.id.as_str()))
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
            .ok_or_else(|| Error::Invalid("Unknown entry".into()))?;
        let row = rows
            .get(row_id.as_str())
            .copied()
            .ok_or_else(|| Error::Invalid("Unknown entry".into()))?;
        let (source_entry, target_entry) = if source_side == Side::Left {
            (&row.left, &row.right)
        } else {
            (&row.right, &row.left)
        };
        let eligible = row.status == Status::Changed
            || row.status
                == if source_side == Side::Left {
                    Status::LeftOnly
                } else {
                    Status::RightOnly
                };
        if !eligible
            || !source_entry
                .as_ref()
                .is_some_and(|e| e.kind == Kind::File && e.error.is_none())
        {
            return Err(Error::Invalid(format!(
                "Not eligible for selected copy direction: {}",
                row.path
            )));
        }
        if snapshot.rules.excludes(relative, false) {
            return Err(Error::Invalid("Excluded entry".into()));
        }
        let source = checked_path(source_root, relative, false)?;
        let target = checked_path(target_root, relative, true)?;
        let source_digest = hash_file(&source, cancel)?;
        let target_digest = regular_digest(&target, cancel)?;
        if source_entry
            .as_ref()
            .and_then(|e| e.digest.as_ref())
            .is_some_and(|d| d != &source_digest)
            || target_entry
                .as_ref()
                .and_then(|e| e.digest.as_ref())
                .is_some_and(|d| Some(d) != target_digest.as_ref())
            || (target_entry.is_none() && target_digest.is_some())
        {
            return Err(Error::Invalid(
                "Files changed since comparison. Compare again.".into(),
            ));
        }
        actions.push(FileAction {
            backup_namespace: None,
            directory_identity: None,
            rename_to: None,
            operation: FileOperation::Copy,
            target_permissions: None,
            row_id: row_id.clone(),
            relative_native: Some(crate::native_path::encode(relative)),
            path: row.path.clone(),
            target_existed: target_digest.is_some(),
            bytes: fs::metadata(source)?.len().to_string(),
            source_digest: Some(source_digest),
            target_digest,
        });
    }
    let plan = SyncPlan {
        warnings: Vec::new(),
        target: target_root.to_string_lossy().into(),
        version: 2,
        policy: FilePlanPolicy::SelectedCopy,
        id: id.clone(),
        target_native: Some(crate::native_path::encode(target_root)),
        source_side,
        session: snapshot.report.session.clone(),
        actions,
        backup_root: base.join(id).to_string_lossy().into_owned(),
    };
    validate_plan(&plan)?;
    Ok(plan)
}

pub(crate) fn flush_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        File::open(path)?.sync_all()?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}
pub(crate) fn flush_ancestors(path: &Path, root: &Path) -> Result<()> {
    if !path.starts_with(root) {
        return Err(Error::Invalid("Directory flush escaped its root".into()));
    }
    let mut at = path;
    loop {
        flush_directory(at)?;
        if at == root {
            break;
        }
        at = at
            .parent()
            .ok_or_else(|| Error::Invalid("Invalid directory flush path".into()))?;
    }
    Ok(())
}

pub(crate) fn atomic_json<T: serde::Serialize>(path: &Path, data: &T) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| Error::Invalid("Invalid journal path".into()))?;
    let mut temp = NamedTempFile::new_in(parent)?;
    temp.write_all(&serde_json::to_vec_pretty(data)?)?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|e| e.error)?;
    flush_directory(parent)?;
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
    action: &FileAction,
    cancel: &AtomicBool,
) -> Result<()> {
    check_cancel(cancel)?;
    let source_digest = action
        .source_digest
        .as_deref()
        .ok_or_else(|| Error::Invalid("Copy source digest missing".into()))?;
    roots_unchanged(snapshot)?;
    let (source_root, target_root) = if plan.source_side == Side::Left {
        (&snapshot.left, &snapshot.right)
    } else {
        (&snapshot.right, &snapshot.left)
    };
    let relative = snapshot
        .paths
        .get(&action.row_id)
        .ok_or_else(|| Error::Invalid("Unknown entry".into()))?;
    if action.relative_native.as_ref() != Some(&crate::native_path::encode(relative)) {
        return Err(Error::Invalid(
            "Plan entry path metadata differs from its snapshot".into(),
        ));
    }
    if snapshot.rules.excludes(relative, false) {
        return Err(Error::Invalid("Excluded entry".into()));
    }
    let source = checked_path(source_root, relative, false)?;
    let destination = checked_path(target_root, relative, true)?;
    if hash_file(&source, cancel)? != source_digest
        || regular_digest(&destination, cancel)? != action.target_digest
    {
        return Err(Error::Invalid(
            "Source or target changed after preview. Compare again.".into(),
        ));
    }
    crate::task::stage("copy_and_backup");
    let parent = destination
        .parent()
        .ok_or_else(|| Error::Invalid("Missing parent".into()))?;
    fs::create_dir_all(parent)?;
    checked_path(target_root, relative, true)?;
    let mut temporary = tempfile::Builder::new()
        .prefix(&format!(".clear-compare-{}-", plan.id))
        .tempfile_in(parent)?;
    durable::register_temporary(snapshot, plan, action, temporary.path())?;
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
        crate::task::read(n);
        temporary.write_all(&chunk[..n])?;
        crate::task::written(n);
    }
    temporary
        .as_file()
        .set_permissions(fs::metadata(&source)?.permissions())?;
    temporary.as_file().sync_all()?;
    if hash_file(temporary.path(), cancel)? != source_digest {
        return Err(Error::Invalid("Source changed during copy".into()));
    }
    if let Some(expected) = &action.target_digest {
        let artifact_root = durable::action_root(plan, action)?;
        let backup_root = artifact_root.as_path();
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
            crate::task::read(n);
            backup_writer.write_all(&chunk[..n])?;
            crate::task::written(n);
        }
        backup_writer.set_permissions(fs::metadata(&destination)?.permissions())?;
        backup_writer.sync_all()?;
        drop(backup_writer);
        if hash_file(&backup, cancel)? != *expected {
            return Err(Error::Invalid("Backup verification failed".into()));
        }
        flush_ancestors(
            backup
                .parent()
                .ok_or_else(|| Error::Invalid("Invalid backup parent".into()))?,
            backup_root,
        )?;
    }
    let _mutation = crate::task::mutation(cancel)?;
    crate::task::stage("revalidate_and_commit");
    durable::verify_restart(snapshot, plan, action)?;
    roots_unchanged(snapshot)?;
    checked_path(source_root, relative, false)?;
    checked_path(target_root, relative, true)?;
    if hash_file(&source, cancel)? != source_digest {
        return Err(Error::Invalid("Copy source changed before commit".into()));
    }
    if regular_digest(&destination, cancel)? != action.target_digest {
        return Err(Error::Invalid("Destination changed during copy".into()));
    }
    if hash_file(temporary.path(), cancel)? != source_digest {
        return Err(Error::Invalid("Prepared copy changed before commit".into()));
    }
    if let Some(expected) = &action.target_digest {
        let backup = checked_path(
            &durable::action_root(plan, action)?,
            &PathBuf::from("files").join(relative),
            false,
        )?;
        if hash_file(&backup, cancel)? != *expected {
            return Err(Error::Invalid("Copy backup changed before commit".into()));
        }
    }
    if action.target_existed {
        temporary.persist(&destination).map_err(|e| e.error)?;
    } else {
        temporary
            .persist_noclobber(&destination)
            .map_err(|e| e.error)?;
    }
    flush_ancestors(parent, target_root)?;
    crate::task::stage("verify_committed_file");
    // Verification is mandatory even after a successful filesystem replacement.
    // Cancellation after replacement must not misreport a completed write.
    if hash_file(&destination, &AtomicBool::new(false))? != source_digest {
        return Err(Error::Invalid("Copied file failed verification".into()));
    }
    Ok(())
}

pub fn execute(snapshot: &Snapshot, plan: &SyncPlan, cancel: &AtomicBool) -> Result<SyncResult> {
    crate::task::stage("validate_plan_and_inventory");
    validate_plan(plan)?;
    if plan.session != snapshot.report.session {
        return Err(Error::Invalid("Plan belongs to another comparison".into()));
    }
    let setup = crate::task::validated_mutation(cancel, || {
        if matches!(
            plan.policy,
            FilePlanPolicy::SelectedCopy | FilePlanPolicy::MirrorFiles | FilePlanPolicy::MirrorTree
        ) {
            roots_unchanged(snapshot)?;
        }
        if plan.policy == FilePlanPolicy::MirrorFiles {
            mirror::revalidate(snapshot, plan, cancel)?;
        }
        if plan.policy == FilePlanPolicy::MirrorTree {
            mirror_tree::revalidate(snapshot, plan, cancel)?;
        }
        Ok(())
    })?;
    let target = target_root(snapshot, plan);
    if plan.version >= 2 && plan.target != target.to_string_lossy() {
        return Err(Error::Invalid(
            "Plan target label differs from its snapshot".into(),
        ));
    }
    if fs::canonicalize(target)? != target {
        return Err(Error::Invalid("File-operation target root changed".into()));
    }
    if plan.target_native.as_ref() != Some(&crate::native_path::encode(target)) {
        return Err(Error::Invalid(
            "Plan destination metadata differs from its snapshot".into(),
        ));
    }
    let backup = Path::new(&plan.backup_root);
    if backup.file_name().and_then(|s| s.to_str()) != Some(plan.id.as_str()) {
        return Err(Error::Invalid(
            "Plan backup directory does not match its ID".into(),
        ));
    }
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
    flush_directory(base)?;
    let _lock = durable::lock(backup, true)?;
    let intent = durable::capture(snapshot, plan)?;
    let intent_digest = durable::write(&backup.join("intent.json"), &intent, durable::MAX_INTENT)?;
    let journal = backup.join("journal.json");
    let mut result = initial_result(plan);
    result.intent_digest = Some(intent_digest);
    let runtime = if intent.left_root.is_some()
        && intent.right_root.is_some()
        && intent.pins.iter().zip(&plan.actions).all(|(pin, a)| {
            (!a.target_existed || pin.target.is_some())
                && (!matches!(
                    a.operation,
                    FileOperation::Copy | FileOperation::CreateDirectory
                ) || pin.source.is_some())
        }) {
        Some(intent.snapshot()?)
    } else {
        None
    };
    let snapshot = runtime.as_ref().unwrap_or(snapshot);
    atomic_json(&journal, &result)?;
    drop(setup);
    for (i, action) in plan.actions.iter().enumerate() {
        crate::task::action(&action.path);
        if let Err(e) = check_cancel(cancel) {
            result.actions[i].state = "cancelled".into();
            result.actions[i].error = Some(failure_message(&e));
            atomic_json(&journal, &result)?;
            break;
        }
        result.actions[i].state = "running".into();
        atomic_json(&journal, &result)?;
        let applied = match action.operation {
            FileOperation::Copy => copy_one(snapshot, plan, action, cancel),
            FileOperation::Delete => delete::delete_one(snapshot, plan, action, cancel),
            FileOperation::Rename => rename::rename_one(snapshot, plan, action, cancel),
            FileOperation::CreateDirectory | FileOperation::RemoveDirectory => {
                directory::execute(snapshot, plan, action, cancel)
            }
        };
        match applied {
            Ok(()) => {
                result.actions[i].state = "completed".into();
                crate::task::completed();
            }
            Err(e) => {
                result.actions[i].state = if matches!(e, Error::Cancelled) {
                    "cancelled"
                } else {
                    "failed"
                }
                .into();
                result.actions[i].error = Some(failure_message(&e));
            }
        }
        atomic_json(&journal, &result)?;
        if result.actions[i].state != "completed" {
            break;
        }
    }
    Ok(result)
}

pub fn affected_paths(snapshot: &Snapshot, plan: &SyncPlan) -> Result<Vec<PathBuf>> {
    let root = target_root(snapshot, plan);
    let mut paths = Vec::new();
    for action in &plan.actions {
        let relative = snapshot
            .paths
            .get(&action.row_id)
            .ok_or_else(|| Error::Invalid("Unknown file-plan entry".into()))?;
        paths.push(root.join(relative));
        if action.operation == FileOperation::Rename {
            let (_, new, temp) = rename::validated_paths(plan, action)?;
            paths.push(root.join(new));
            paths.push(root.join(temp));
        }
    }
    Ok(paths)
}
pub fn relocations(
    snapshot: &Snapshot,
    result: &SyncResult,
) -> Result<Vec<crate::editing::Relocation>> {
    let mut changes = Vec::new();
    let root = target_root(snapshot, &result.plan);
    for (action, state) in result.plan.actions.iter().zip(&result.actions) {
        if action.operation == FileOperation::Rename && state.state == "completed" {
            let (old, new, temp) = rename::validated_paths(&result.plan, action)?;
            for from in [old, temp] {
                changes.push(crate::editing::Relocation {
                    root: root.to_owned(),
                    from,
                    to: new.clone(),
                });
            }
        }
    }
    Ok(changes)
}

/// Complete captured-scope revalidation for multi-root coordination.
pub(crate) fn revalidate_scope(snapshot: &Snapshot, cancel: &AtomicBool) -> Result<()> {
    roots_unchanged(snapshot)?;
    mirror::revalidate_inventory(snapshot, cancel)
}

/// Mixed reviewed actions for one target of a baseline-coordinated reconciliation.
pub(crate) fn plan_reconcile(
    snapshot: &Snapshot,
    items: &[(String, FileOperation)],
    backup: &Path,
    target_side: Side,
    cancel: &AtomicBool,
) -> Result<SyncPlan> {
    let source_side = if target_side == Side::Left {
        Side::Right
    } else {
        Side::Left
    };
    let ids = |operation| {
        items
            .iter()
            .filter(|(_, op)| *op == operation)
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>()
    };
    let mut files = BTreeMap::new();
    let copies = ids(FileOperation::Copy);
    if !copies.is_empty() {
        for action in
            plan_direction(snapshot, &copies, backup, cancel, source_side.clone())?.actions
        {
            files.insert(action.row_id.clone(), action);
        }
    }
    let deletes = ids(FileOperation::Delete);
    if !deletes.is_empty() {
        for action in plan_delete(snapshot, &deletes, backup, cancel, target_side.clone())?.actions
        {
            files.insert(action.row_id.clone(), action);
        }
    }
    let root = if target_side == Side::Left {
        &snapshot.left
    } else {
        &snapshot.right
    };
    let base = fs::canonicalize(backup)?;
    reject_overlap(&base, &snapshot.left)?;
    reject_overlap(&base, &snapshot.right)?;
    let mut actions = Vec::new();
    for (id, operation) in items {
        if let Some(action) = files.remove(id) {
            actions.push(action);
            continue;
        }
        if !matches!(
            operation,
            FileOperation::CreateDirectory | FileOperation::RemoveDirectory
        ) {
            return Err(Error::Invalid("Unsupported reconciliation action".into()));
        }
        let relative = snapshot
            .paths
            .get(id)
            .ok_or_else(|| Error::Invalid("Unknown reconciliation directory".into()))?;
        let observed = directory::observe(root, relative)?;
        if (*operation == FileOperation::CreateDirectory && observed.is_some())
            || (*operation == FileOperation::RemoveDirectory && observed.is_none())
        {
            return Err(Error::Invalid(
                "Reconciliation directory changed or aliases an existing name".into(),
            ));
        }
        actions.push(FileAction {
            backup_namespace: None,
            directory_identity: observed.as_ref().map(|r| r.identity.clone()),
            rename_to: None,
            operation: *operation,
            target_permissions: observed.map(|r| r.permissions),
            row_id: id.clone(),
            relative_native: Some(crate::native_path::encode(relative)),
            path: crate::tree::display_path(relative),
            target_existed: *operation == FileOperation::RemoveDirectory,
            bytes: "0".into(),
            source_digest: None,
            target_digest: None,
        });
    }
    let id = unique_id();
    let plan = SyncPlan {
        warnings: Vec::new(),
        target: root.to_string_lossy().into(),
        version: 6,
        policy: FilePlanPolicy::Reconcile { target_side },
        id: id.clone(),
        target_native: Some(crate::native_path::encode(root)),
        source_side,
        session: snapshot.report.session.clone(),
        actions,
        backup_root: base.join(id).to_string_lossy().into(),
    };
    validate_plan(&plan)?;
    Ok(plan)
}
