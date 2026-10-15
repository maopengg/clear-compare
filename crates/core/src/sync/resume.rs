//! Reviewed, locked forward restart. The immutable intent remains separate from retry artifacts.
use super::durable::{self, Intent, Object, Temporary};
use super::*;
use crate::native_path;
use clear_contracts::{Entry, ResumeAction, ResumePreview};
use std::io::{Read, Write};
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Done,
    Pending,
    Staged,
}
#[derive(Clone)]
struct Cleanup {
    record: Temporary,
    current: Object,
    digest: String,
    bytes: u64,
}
pub struct Plan {
    pub view: ResumePreview,
    journal: PathBuf,
    hash: String,
    modes: Vec<Mode>,
    cleanup: Vec<Cleanup>,
    rows: Vec<ResumeAction>,
}
struct Evidence {
    report: SyncResult,
    snapshot: Snapshot,
    modes: Vec<Mode>,
    cleanup: Vec<Cleanup>,
}
fn file(root: &Path, path: &Path, cancel: &AtomicBool) -> Result<Option<(Object, String)>> {
    let absolute = checked_path(root, path, true)?;
    match fs::symlink_metadata(&absolute) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
        Ok(_) => {}
    }
    if !crate::native_path::exact_leaf(root, path, cancel)? {
        return Ok(None);
    }
    let Some(object) = Object::observe(root, path)? else {
        return Ok(None);
    };
    if object.kind != Kind::File {
        return Err(Error::Invalid("Restart file changed type".into()));
    }
    let digest = hash_file(&checked_path(root, path, false)?, cancel)?;
    Ok(Some((object, digest)))
}
fn backup(
    plan: &SyncPlan,
    action: &FileAction,
    cancel: &AtomicBool,
    pin: Option<&Object>,
) -> Result<()> {
    let root = durable::action_root(plan, action)?;
    let relative = native_path::decode(
        action
            .relative_native
            .as_ref()
            .ok_or_else(|| Error::Invalid("Missing action path".into()))?,
    )?;
    if let Some(expected) = &action.target_digest {
        let path = checked_path(&root, &PathBuf::from("files").join(&relative), false)?;
        let actual_permissions = permissions(&fs::symlink_metadata(&path)?);
        if hash_file(&path, cancel)? != *expected
            || action
                .target_permissions
                .as_ref()
                .or_else(|| pin.map(|p| &p.permissions))
                .is_some_and(|p| *p != actual_permissions)
        {
            return Err(Error::Invalid(
                "Original backup changed before restart".into(),
            ));
        }
    }
    if action.operation == FileOperation::RemoveDirectory {
        let record = directory::Record::read(&root, &relative, false)?;
        if action.directory_identity.as_ref() != Some(&record.identity)
            || action.target_permissions.as_ref() != Some(&record.permissions)
        {
            return Err(Error::Invalid("Removed directory record changed".into()));
        }
    }
    Ok(())
}
fn inspect(journal: &Path, cancel: &AtomicBool) -> Result<(Evidence, String)> {
    check_cancel(cancel)?;
    let (mut report, hash): (SyncResult, String) = durable::read(journal, 64 * 1024 * 1024)?;
    validate_plan_metadata(&report.plan)?;
    durable::namespace(&report.plan.id)?;
    if report.actions.len() != report.plan.actions.len() || report.attempts.len() >= 1000 {
        return Err(Error::Invalid("Invalid restart journal counts".into()));
    }
    let root = journal
        .parent()
        .ok_or_else(|| Error::Invalid("Journal parent missing".into()))?;
    if root.file_name().and_then(|n| n.to_str()) != Some(report.plan.id.as_str())
        || fs::canonicalize(root)? != root
        || Path::new(&report.plan.backup_root) != root
        || journal.file_name() != Some(std::ffi::OsStr::new("journal.json"))
    {
        return Err(Error::Invalid(
            "Restart journal moved from its original backup set".into(),
        ));
    }
    let expected = report.intent_digest.as_ref().ok_or_else(|| {
        Error::Invalid(
            "This older operation has no forward-restart context; use reviewed rollback".into(),
        )
    })?;
    let (intent, intent_hash): (Intent, String) = durable::read(
        &checked_path(root, Path::new("intent.json"), false)?,
        durable::MAX_INTENT,
    )?;
    if &intent_hash != expected {
        return Err(Error::Invalid("Immutable restart intent changed".into()));
    }
    if !durable::intent_matches(&report.plan, &intent.plan) {
        return Err(Error::Invalid(
            "Effective operation differs from immutable intent".into(),
        ));
    }
    let mut snapshot = intent.snapshot()?;
    if report.plan.target_native.as_ref()
        != Some(&native_path::encode(target_root(&snapshot, &report.plan)))
    {
        return Err(Error::Invalid(
            "Restart target differs from its original roots".into(),
        ));
    }
    let target = target_root(&snapshot, &report.plan).to_owned();
    report.journal = journal.to_string_lossy().into();
    report.plan.target = target.to_string_lossy().into();
    for (action, state) in report.plan.actions.iter_mut().zip(&mut report.actions) {
        let relative = native_path::decode(
            action
                .relative_native
                .as_ref()
                .ok_or_else(|| Error::Invalid("Missing restart label".into()))?,
        )?;
        action.path = crate::tree::display_path(&relative);
        state.path = action.path.clone();
        if let Some(rename) = &mut action.rename_to {
            rename.destination =
                crate::tree::display_path(&native_path::decode(&rename.destination_native)?);
        }
    }

    let source = if target == snapshot.left {
        snapshot.right.clone()
    } else {
        snapshot.left.clone()
    };
    reject_overlap(root, &snapshot.left)?;
    reject_overlap(root, &snapshot.right)?;
    let temporary = durable::temporaries(root, &report.plan, cancel)?;
    let mut cleanup = Vec::new();
    let mut ignore = BTreeSet::new();
    for record in &temporary {
        let relative = native_path::decode(&record.relative)?;
        if let Some((current, digest)) = file(&target, &relative, cancel)? {
            if current.identity != record.object.identity {
                return Err(Error::Invalid(
                    "Owned work file was replaced; retain it for manual review".into(),
                ));
            }
            if !ignore.insert(relative.clone()) {
                return Err(Error::Invalid(
                    "Duplicate work-file ownership records".into(),
                ));
            }
            cleanup.push(Cleanup {
                record: record.clone(),
                current,
                digest,
                bytes: fs::symlink_metadata(target.join(relative))?.len(),
            });
        }
    }
    let row_indices = snapshot
        .report
        .rows
        .iter()
        .enumerate()
        .map(|(i, r)| (r.id.clone(), i))
        .collect::<BTreeMap<_, _>>();
    let copy_owners = temporary
        .iter()
        .map(|t| {
            (
                (
                    t.row_id.clone(),
                    t.namespace.clone(),
                    t.object.identity.index.clone(),
                    t.object.identity.volume.clone(),
                ),
                t,
            )
        })
        .collect::<BTreeMap<_, _>>();
    cleanup.sort_by(|a, b| a.record.relative.cmp(&b.record.relative));
    let mut modes = Vec::new();
    for (i, action) in report.plan.actions.iter().enumerate() {
        check_cancel(cancel)?;
        let state = &report.actions[i];
        if state.path != action.path
            || !matches!(
                state.state.as_str(),
                "pending" | "running" | "completed" | "failed" | "cancelled"
            )
        {
            return Err(Error::Invalid(
                "Invalid action state in restart journal".into(),
            ));
        }
        let touched = state.state != "pending";
        let completed = state.state == "completed";
        let relative = snapshot
            .paths
            .get(&action.row_id)
            .ok_or_else(|| Error::Invalid("Missing restart action".into()))?
            .clone();
        let pin = &intent.pins[i];
        if matches!(
            action.operation,
            FileOperation::Copy | FileOperation::CreateDirectory
        ) {
            let now = Object::observe(&source, &relative)?;
            if pin.source.is_none() || now != pin.source {
                return Err(Error::Invalid(format!(
                    "Original source identity/permissions changed: {}",
                    action.path
                )));
            }
            if action.operation == FileOperation::Copy
                && file(&source, &relative, cancel)?.map(|(_, h)| h) != action.source_digest
            {
                return Err(Error::Invalid("Restart source contents changed".into()));
            }
        }
        let mode = match action.operation {
            FileOperation::Copy => {
                let current = file(&target, &relative, cancel)?;
                let original = match (&current, &action.target_digest, &pin.target) {
                    (None, None, None) => true,
                    (Some((o, h)), Some(d), Some(p)) => h == d && o == p,
                    _ => false,
                };
                let applied = current.as_ref().is_some_and(|(o, h)| {
                    Some(h) == action.source_digest.as_ref()
                        && pin
                            .source
                            .as_ref()
                            .is_some_and(|p| p.permissions == o.permissions)
                        && copy_owners
                            .get(&(
                                action.row_id.clone(),
                                action.backup_namespace.clone(),
                                o.identity.index.clone(),
                                o.identity.volume.clone(),
                            ))
                            .is_some_and(|t| t.object.identity == o.identity)
                });
                if completed || !original && touched && applied {
                    if !applied {
                        return Err(Error::Invalid(
                            "Completed copy changed or lost its ownership proof".into(),
                        ));
                    }
                    backup(&report.plan, action, cancel, pin.target.as_ref())?;
                    Mode::Done
                } else if original {
                    Mode::Pending
                } else {
                    return Err(Error::Invalid(
                        "Copy target conflicts with restart intent".into(),
                    ));
                }
            }
            FileOperation::Delete => {
                let current = file(&target, &relative, cancel)?;
                if current.is_none() && touched {
                    backup(&report.plan, action, cancel, pin.target.as_ref())?;
                    Mode::Done
                } else if !completed
                    && current.as_ref().is_some_and(|(o, h)| {
                        Some(h) == action.target_digest.as_ref() && pin.target.as_ref() == Some(o)
                    })
                {
                    Mode::Pending
                } else {
                    return Err(Error::Invalid(
                        "Deletion target changed before restart".into(),
                    ));
                }
            }
            FileOperation::CreateDirectory => {
                let current = Object::observe(&target, &relative)?;
                if let Some(current) = current {
                    if !touched {
                        return Err(Error::Invalid(
                            "Directory creation target is occupied".into(),
                        ));
                    }
                    let record = directory::Record::read(
                        &durable::action_root(&report.plan, action)?,
                        &relative,
                        true,
                    )?;
                    if current.kind != Kind::Directory
                        || current.identity != record.identity
                        || current.permissions != record.permissions
                    {
                        return Err(Error::Invalid(
                            "Created directory identity changed before restart".into(),
                        ));
                    }
                    Mode::Done
                } else if completed {
                    return Err(Error::Invalid(
                        "Completed directory creation disappeared".into(),
                    ));
                } else {
                    Mode::Pending
                }
            }
            FileOperation::RemoveDirectory => {
                let current = Object::observe(&target, &relative)?;
                if current.is_none() && touched {
                    backup(&report.plan, action, cancel, pin.target.as_ref())?;
                    Mode::Done
                } else if !completed
                    && current.as_ref().is_some_and(|o| {
                        action.directory_identity.as_ref() == Some(&o.identity)
                            && action.target_permissions.as_ref() == Some(&o.permissions)
                    })
                {
                    Mode::Pending
                } else {
                    return Err(Error::Invalid(
                        "Directory removal target was recreated or changed".into(),
                    ));
                }
            }
            FileOperation::Rename => {
                let (old, new, stage) = rename::validated_paths(&report.plan, action)?;
                let old_file = file(&target, &old, cancel)?;
                let new_file = file(&target, &new, cancel)?;
                let staged = file(&target, &stage, cancel)?;
                let matches = |v: &Option<(Object, String)>| {
                    v.as_ref().is_some_and(|(o, h)| {
                        Some(h) == action.target_digest.as_ref() && pin.target.as_ref() == Some(o)
                    })
                };
                if old_file.is_none() && staged.is_none() && touched && matches(&new_file) {
                    backup(&report.plan, action, cancel, pin.target.as_ref())?;
                    Mode::Done
                } else if old_file.is_none() && new_file.is_none() && touched && matches(&staged) {
                    backup(&report.plan, action, cancel, pin.target.as_ref())?;
                    Mode::Staged
                } else if !completed && matches(&old_file) && new_file.is_none() && staged.is_none()
                {
                    Mode::Pending
                } else {
                    return Err(Error::Invalid(
                        "Rename names or staged contents conflict with restart intent".into(),
                    ));
                }
            }
        };
        if mode == Mode::Done {
            let entry = if matches!(
                action.operation,
                FileOperation::Copy | FileOperation::CreateDirectory
            ) {
                Some(Entry {
                    kind: if action.operation == FileOperation::Copy {
                        Kind::File
                    } else {
                        Kind::Directory
                    },
                    bytes: action.bytes.clone(),
                    modified_ms: None,
                    digest: action.source_digest.clone(),
                    error: None,
                })
            } else {
                None
            };
            if let Some(row) = row_indices
                .get(&action.row_id)
                .map(|i| &mut snapshot.report.rows[*i])
            {
                if target == snapshot.left {
                    row.left = entry;
                } else {
                    row.right = entry;
                }
            }
        }
        modes.push(mode);
    }
    if matches!(
        report.plan.policy,
        FilePlanPolicy::MirrorFiles | FilePlanPolicy::MirrorTree
    ) {
        let mut remaining = report.plan.clone();
        remaining.actions = remaining
            .actions
            .into_iter()
            .enumerate()
            .filter_map(|(i, a)| (modes[i] != Mode::Done).then_some(a))
            .collect();
        if report.plan.policy == FilePlanPolicy::MirrorTree {
            mirror_tree::revalidate_layout(&snapshot, &remaining, cancel)?;
        } else {
            let expected = mirror::expected(&snapshot, &report.plan.source_side, cancel)?;
            let actual = remaining
                .actions
                .iter()
                .map(|a| (a.row_id.clone(), a.operation))
                .collect::<BTreeMap<_, _>>();
            if actual != expected {
                return Err(Error::Invalid("Remaining mirror scope changed".into()));
            }
        }
        mirror::revalidate_inventory_ignoring(&snapshot, cancel, &target, &ignore)?;
    }
    Ok((
        Evidence {
            report,
            snapshot,
            modes,
            cleanup,
        },
        hash,
    ))
}
fn operation(op: FileOperation) -> String {
    match op {
        FileOperation::Copy => "copy",
        FileOperation::Delete => "delete",
        FileOperation::Rename => "rename",
        FileOperation::CreateDirectory => "create_directory",
        FileOperation::RemoveDirectory => "remove_directory",
    }
    .into()
}
pub fn preview(journal: &Path, cancel: &AtomicBool) -> Result<Plan> {
    if !fs::symlink_metadata(journal)?.is_file() {
        return Err(Error::Invalid("Select a regular operation journal".into()));
    }
    let journal = fs::canonicalize(journal)?;
    let root = journal
        .parent()
        .ok_or_else(|| Error::Invalid("Journal root missing".into()))?;
    let (header, _): (SyncResult, String) = durable::read(&journal, 64 * 1024 * 1024)?;
    if header.intent_digest.is_none() {
        return Err(Error::Invalid(
            "This older operation has no forward-restart context; use reviewed rollback".into(),
        ));
    }
    let _lock = durable::lock(root, false)?;
    let (evidence, hash) = inspect(&journal, cancel)?;
    let satisfied = evidence.modes.iter().filter(|m| **m == Mode::Done).count();
    if satisfied == evidence.modes.len()
        && evidence.cleanup.is_empty()
        && evidence
            .report
            .actions
            .iter()
            .all(|a| a.state == "completed")
    {
        return Err(Error::Invalid("Operation is already complete".into()));
    }
    let id = unique_id();
    let mut rows = evidence
        .report
        .plan
        .actions
        .iter()
        .zip(&evidence.modes)
        .map(|(a, m)| {
            let relative = native_path::decode(
                a.relative_native
                    .as_ref()
                    .ok_or_else(|| Error::Invalid("Missing restart native label".into()))?,
            )?;
            let mut path = crate::tree::display_path(&relative);
            if a.operation == FileOperation::Rename {
                let (_, new, _) = rename::validated_paths(&evidence.report.plan, a)?;
                path.push_str(" → ");
                path.push_str(&crate::tree::display_path(&new));
            }
            Ok(ResumeAction {
                path,
                operation: operation(a.operation),
                state: match m {
                    Mode::Done => "verified_complete",
                    Mode::Pending => "retry",
                    Mode::Staged => "finish_staged_rename",
                }
                .into(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    for c in &evidence.cleanup {
        rows.push(ResumeAction {
            path: crate::tree::display_path(&native_path::decode(&c.record.relative)?),
            operation: "retain_work_file".into(),
            state: "backup_then_remove_internal_temporary".into(),
        });
    }
    let target = target_root(&evidence.snapshot, &evidence.report.plan);
    let source = if target == evidence.snapshot.left {
        &evidence.snapshot.right
    } else {
        &evidence.snapshot.left
    };
    let view=ResumePreview {id:id.clone(),journal:journal.to_string_lossy().into(),source:source.to_string_lossy().into(),target:target.to_string_lossy().into(),backup_root:root.join("attempts").join(&id).to_string_lossy().into(),satisfied,pending:evidence.modes.len()-satisfied,cleanup_files:evidence.cleanup.len(),total:rows.len(),actions:rows.iter().take(500).cloned().collect(),warnings:vec!["Existing backups remain in their original attempt; owned partial work files are retained before cleanup.".into()]};
    Ok(Plan {
        view,
        journal,
        hash,
        modes: evidence.modes,
        cleanup: evidence.cleanup,
        rows,
    })
}
impl Plan {
    pub fn page(&self, offset: usize) -> Vec<ResumeAction> {
        self.rows.iter().skip(offset).take(500).cloned().collect()
    }
}

#[derive(serde::Serialize)]
struct Retained<'a> {
    relative: &'a clear_contracts::NativePath,
    digest: &'a str,
    object: &'a Object,
    backup: String,
    bytes: String,
}
fn same_cleanup(a: &[Cleanup], b: &[Cleanup]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(a, b)| {
            a.record.relative == b.record.relative
                && a.current == b.current
                && a.digest == b.digest
                && a.bytes == b.bytes
        })
}
fn retain(
    snapshot: &Snapshot,
    plan: &SyncPlan,
    entry: &Cleanup,
    destination: &Path,
    cancel: &AtomicBool,
) -> Result<()> {
    crate::task::stage("retain_internal_work_file");
    durable::verify_roots(snapshot)?;
    let relative = native_path::decode(&entry.record.relative)?;
    let root = target_root(snapshot, plan);
    let source = checked_path(root, &relative, false)?;
    if file(root, &relative, cancel)?.as_ref()
        != Some(&(entry.current.clone(), entry.digest.clone()))
    {
        return Err(Error::Invalid("Reviewed work file changed".into()));
    }
    let mut input = File::open(&source)?;
    let mut output = File::create_new(destination)?;
    let mut buffer = [0u8; 65536];
    loop {
        check_cancel(cancel)?;
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        crate::task::read(n);
        output.write_all(&buffer[..n])?;
        crate::task::written(n);
    }
    set_permissions(&output, &entry.current.permissions)?;
    output.sync_all()?;
    drop(input);
    drop(output);
    let _mutation = crate::task::mutation(cancel)?;
    durable::verify_roots(snapshot)?;
    if hash_file(destination, cancel)? != entry.digest
        || permissions(&fs::symlink_metadata(destination)?) != entry.current.permissions
        || file(root, &relative, cancel)?.as_ref()
            != Some(&(entry.current.clone(), entry.digest.clone()))
    {
        return Err(Error::Invalid(
            "Work-file retention verification failed".into(),
        ));
    }
    flush_directory(
        destination
            .parent()
            .ok_or_else(|| Error::Invalid("Retention parent missing".into()))?,
    )?;
    fs::remove_file(&source)?;
    flush_directory(
        source
            .parent()
            .ok_or_else(|| Error::Invalid("Work-file parent missing".into()))?,
    )?;
    crate::task::completed();
    Ok(())
}
fn staged(
    snapshot: &Snapshot,
    plan: &SyncPlan,
    action: &FileAction,
    cancel: &AtomicBool,
) -> Result<()> {
    let _mutation = crate::task::mutation(cancel)?;
    crate::task::stage("finish_staged_rename");
    durable::verify_roots(snapshot)?;
    let root = target_root(snapshot, plan);
    let (old, new, temporary) = rename::validated_paths(plan, action)?;
    let pin = snapshot
        .restart
        .as_ref()
        .and_then(|c| c.pins.get(&action.row_id))
        .and_then(|p| p.target.as_ref())
        .ok_or_else(|| Error::Invalid("Staged rename identity missing".into()))?;
    if file(root, &old, cancel)?.is_some()
        || file(root, &new, cancel)?.is_some()
        || file(root, &temporary, cancel)?.as_ref()
            != Some(&(
                pin.clone(),
                action
                    .target_digest
                    .clone()
                    .ok_or_else(|| Error::Invalid("Staged digest missing".into()))?,
            ))
    {
        return Err(Error::Invalid(
            "Staged rename changed after restart preview".into(),
        ));
    }
    backup(plan, action, cancel, Some(pin))?;
    let source = checked_path(root, &temporary, false)?;
    let destination = checked_path(root, &new, true)?;
    rename::absent(&destination)?;
    crate::no_replace::rename(&source, &destination)?;
    flush_directory(
        destination
            .parent()
            .ok_or_else(|| Error::Invalid("Rename parent missing".into()))?,
    )?;
    let settled = AtomicBool::new(false);
    if file(root, &new, &settled)?.as_ref()
        != Some(&(
            pin.clone(),
            action.target_digest.clone().unwrap_or_default(),
        ))
        || file(root, &temporary, &settled)?.is_some()
        || file(root, &old, &settled)?.is_some()
    {
        return Err(Error::Invalid("Resumed rename verification failed".into()));
    }
    Ok(())
}
pub fn execute(plan: &Plan, cancel: &AtomicBool) -> Result<SyncResult> {
    let root = plan
        .journal
        .parent()
        .ok_or_else(|| Error::Invalid("Restart root missing".into()))?;
    let _lock = durable::lock(root, false)?;
    let mut checked = None;
    let setup = crate::task::validated_mutation(cancel, || {
        let (current, hash) = inspect(&plan.journal, cancel)?;
        if hash != plan.hash
            || current.modes != plan.modes
            || !same_cleanup(&current.cleanup, &plan.cleanup)
        {
            return Err(Error::Invalid(
                "Restart preview expired; review it again".into(),
            ));
        }
        checked = Some(current);
        Ok(())
    })?;
    let mut evidence = checked.ok_or_else(|| Error::Invalid("Restart evidence missing".into()))?;
    let attempt = Path::new(&plan.view.backup_root);
    let id = &plan.view.id;
    durable::namespace(id)?;
    if attempt != root.join("attempts").join(id) {
        return Err(Error::Invalid("Restart attempt path differs".into()));
    }
    let attempts = checked_path(root, Path::new("attempts"), true)?;
    fs::create_dir_all(&attempts)?;
    checked_path(root, Path::new("attempts"), false)?;
    fs::create_dir(attempt)?;
    flush_ancestors(attempt, root)?;
    let prior = durable::bytes(&plan.journal, 64 * 1024 * 1024)?;
    if durable::digest(&prior) != plan.hash {
        return Err(Error::Invalid(
            "Operation journal changed while locked".into(),
        ));
    }
    let mut previous = File::create_new(attempt.join("prior-journal.json"))?;
    previous.write_all(&prior)?;
    previous.sync_all()?;
    drop(previous);
    flush_directory(attempt)?;
    let retention = checked_path(attempt, Path::new("retained-temporaries"), true)?;
    fs::create_dir(&retention)?;
    flush_directory(attempt)?;
    let retained = evidence
        .cleanup
        .iter()
        .enumerate()
        .map(|(i, c)| Retained {
            relative: &c.record.relative,
            digest: &c.digest,
            object: &c.current,
            backup: format!("retained-temporaries/{i}.bin"),
            bytes: c.bytes.to_string(),
        })
        .collect::<Vec<_>>();
    durable::write(&attempt.join("retained.json"), &retained, 64 * 1024 * 1024)?;
    evidence.report.attempts.push(id.clone());
    for (i, mode) in evidence.modes.iter().enumerate() {
        evidence.report.actions[i].error = None;
        if *mode == Mode::Done {
            evidence.report.actions[i].state = "completed".into();
        } else {
            evidence.report.actions[i].state = "pending".into();
            if *mode == Mode::Pending {
                evidence.report.plan.actions[i].backup_namespace = Some(id.clone());
            }
        }
    }
    // Leave space for completed-state growth and one bounded failure message.
    struct Bound(usize);
    impl Write for Bound {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            if self.0 + b.len() > 63 * 1024 * 1024 {
                return Err(std::io::Error::other(
                    "Resumed journal exceeds its safe size budget",
                ));
            }
            self.0 += b.len();
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer_pretty(Bound(0), &evidence.report)?;
    atomic_json(&plan.journal, &evidence.report)?;
    drop(setup);
    crate::task::completed_many(evidence.modes.iter().filter(|m| **m == Mode::Done).count());
    for (i, entry) in evidence.cleanup.iter().enumerate() {
        crate::task::action(&crate::tree::display_path(&native_path::decode(
            &entry.record.relative,
        )?));
        retain(
            &evidence.snapshot,
            &evidence.report.plan,
            entry,
            &retention.join(format!("{i}.bin")),
            cancel,
        )?;
    }
    for (i, mode) in evidence.modes.iter().enumerate() {
        if *mode == Mode::Done {
            continue;
        }
        let action = &evidence.report.plan.actions[i];
        crate::task::action(&action.path);
        if let Err(e) = check_cancel(cancel) {
            evidence.report.actions[i].state = "cancelled".into();
            evidence.report.actions[i].error = Some(failure_message(&e));
            atomic_json(&plan.journal, &evidence.report)?;
            break;
        }
        evidence.report.actions[i].state = "running".into();
        atomic_json(&plan.journal, &evidence.report)?;
        let applied = if *mode == Mode::Staged {
            staged(&evidence.snapshot, &evidence.report.plan, action, cancel)
        } else {
            match action.operation {
                FileOperation::Copy => {
                    copy_one(&evidence.snapshot, &evidence.report.plan, action, cancel)
                }
                FileOperation::Delete => {
                    delete::delete_one(&evidence.snapshot, &evidence.report.plan, action, cancel)
                }
                FileOperation::Rename => {
                    rename::rename_one(&evidence.snapshot, &evidence.report.plan, action, cancel)
                }
                FileOperation::CreateDirectory | FileOperation::RemoveDirectory => {
                    directory::execute(&evidence.snapshot, &evidence.report.plan, action, cancel)
                }
            }
        };
        match applied {
            Ok(()) => {
                evidence.report.actions[i].state = "completed".into();
                crate::task::completed();
            }
            Err(e) => {
                evidence.report.actions[i].state = if matches!(e, Error::Cancelled) {
                    "cancelled"
                } else {
                    "failed"
                }
                .into();
                evidence.report.actions[i].error = Some(failure_message(&e));
            }
        }
        atomic_json(&plan.journal, &evidence.report)?;
        if evidence.report.actions[i].state != "completed" {
            break;
        }
    }
    Ok(evidence.report)
}
impl Plan {
    pub fn effects(
        &self,
        result: &SyncResult,
    ) -> Result<(Vec<PathBuf>, Vec<crate::editing::Relocation>)> {
        let root = self
            .journal
            .parent()
            .ok_or_else(|| Error::Invalid("Restart root missing".into()))?;
        let (intent, hash): (Intent, String) = durable::read(
            &checked_path(root, Path::new("intent.json"), false)?,
            durable::MAX_INTENT,
        )?;
        if Some(hash) != result.intent_digest {
            return Err(Error::Invalid("Restart intent changed".into()));
        }
        if !durable::intent_matches(&result.plan, &intent.plan) {
            return Err(Error::Invalid(
                "Restart outcome differs from immutable intent".into(),
            ));
        }
        let root = native_path::decode(
            intent
                .plan
                .target_native
                .as_ref()
                .ok_or_else(|| Error::Invalid("Missing restart target".into()))?,
        )?;
        let mut affected = Vec::new();
        let mut moves = Vec::new();
        for (action, state) in result.plan.actions.iter().zip(&result.actions) {
            let relative = native_path::decode(
                action
                    .relative_native
                    .as_ref()
                    .ok_or_else(|| Error::Invalid("Missing restart entry".into()))?,
            )?;
            affected.push(root.join(&relative));
            if action.operation == FileOperation::Rename {
                let (old, new, temp) = rename::validated_paths(&result.plan, action)?;
                affected.push(root.join(&new));
                affected.push(root.join(&temp));
                if state.state == "completed" {
                    for from in [old, temp] {
                        moves.push(crate::editing::Relocation {
                            root: root.clone(),
                            from,
                            to: new.clone(),
                        });
                    }
                }
            }
        }
        Ok((affected, moves))
    }
}

#[cfg(test)]
mod crash_tests {
    use super::*;
    use std::{
        process::{Command, Stdio},
        sync::{Arc, atomic::AtomicBool},
        time::{Duration, Instant},
    };
    const ENV: &str = "CLEAR_COMPARE_RESUME_CRASH_FIXTURE";
    #[test]
    fn crash_fixture() {
        let Some(base) = std::env::var_os(ENV) else {
            return;
        };
        let base = PathBuf::from(base);
        let snapshot = crate::compare::compare(
            &clear_contracts::CompareRequest {
                left: base.join("l").display().to_string(),
                right: base.join("r").display().to_string(),
                exclusions: "-keep.txt".into(),
                content: true,
            },
            &AtomicBool::new(false),
        )
        .unwrap();
        let plan = plan_mirror_tree(
            &snapshot,
            &base.join("b"),
            &AtomicBool::new(false),
            Side::Left,
        )
        .unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let control = crate::task::Control::new(
            clear_contracts::OperationKind::File,
            plan.actions.len(),
            cancel.clone(),
        );
        let observer = control.clone();
        let journal = Path::new(&plan.backup_root).join("journal.json");
        std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(20);
            loop {
                let status = observer.status();
                if status.phase == clear_contracts::TaskPhase::Paused && status.bytes_written != "0"
                {
                    fs::write(base.join("ready"), journal.to_str().unwrap()).unwrap();
                    return;
                }
                assert!(Instant::now() < deadline, "fixture did not pause");
                std::thread::sleep(Duration::from_millis(5));
            }
        });
        crate::task::pause_after_first_stream_write();
        control
            .run(|| super::super::execute(&snapshot, &plan, &cancel))
            .unwrap();
        panic!("fixture unexpectedly completed before being killed");
    }
    #[test]
    fn cancelled_retry_keeps_its_artifacts_and_a_later_attempt_retains_original_rollback() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for directory in ["l", "r", "b", "undo"] {
            fs::create_dir(root.join(directory)).unwrap();
        }
        fs::write(root.join("l/a"), vec![b'n'; 256 * 1024]).unwrap();
        fs::write(root.join("r/a"), b"original").unwrap();
        let snapshot = crate::compare::compare(
            &clear_contracts::CompareRequest {
                left: root.join("l").display().to_string(),
                right: root.join("r").display().to_string(),
                exclusions: String::new(),
                content: true,
            },
            &AtomicBool::new(false),
        )
        .unwrap();
        let original = super::super::plan(
            &snapshot,
            &[snapshot.report.rows[0].id.clone()],
            &root.join("b"),
            &AtomicBool::new(false),
        )
        .unwrap();
        let stopped = super::super::execute(&snapshot, &original, &AtomicBool::new(true)).unwrap();
        let journal = Path::new(&stopped.journal);
        let first = super::preview(journal, &AtomicBool::new(false)).unwrap();
        let original_bytes = fs::read(journal).unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let control =
            crate::task::Control::new(clear_contracts::OperationKind::File, 1, cancel.clone());
        let observer = control.clone();
        let cancelled = std::thread::scope(|scope| {
            let waiting = scope.spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(20);
                loop {
                    if observer.status().phase == clear_contracts::TaskPhase::Paused {
                        observer.cancel();
                        return;
                    }
                    if Instant::now() >= deadline {
                        observer.cancel();
                        panic!("retry did not pause");
                    }
                    std::thread::sleep(Duration::from_millis(5));
                }
            });
            crate::task::pause_after_first_stream_write();
            let out = control.run(|| super::execute(&first, &cancel)).unwrap();
            waiting.join().unwrap();
            out
        });
        assert_eq!(cancelled.actions[0].state, "cancelled");
        assert_eq!(fs::read(root.join("r/a")).unwrap(), b"original");
        let first_backup = Path::new(&first.view.backup_root).join("prior-journal.json");
        assert_eq!(fs::read(&first_backup).unwrap(), original_bytes);
        let cancelled_bytes = fs::read(journal).unwrap();
        let second = super::preview(journal, &AtomicBool::new(false)).unwrap();
        assert_ne!(first.view.backup_root, second.view.backup_root);
        let done = super::execute(&second, &AtomicBool::new(false)).unwrap();
        assert_eq!(done.actions[0].state, "completed");
        assert_eq!(done.attempts, [first.view.id, second.view.id]);
        assert_eq!(fs::read(&first_backup).unwrap(), original_bytes);
        assert_eq!(
            fs::read(Path::new(&second.view.backup_root).join("prior-journal.json")).unwrap(),
            cancelled_bytes
        );
        assert_eq!(
            fs::read(Path::new(&second.view.backup_root).join("files/a")).unwrap(),
            b"original"
        );
        let undo =
            crate::recovery::preview(journal, &root.join("undo"), &AtomicBool::new(false)).unwrap();
        assert!(
            crate::recovery::execute(&undo, &AtomicBool::new(false))
                .unwrap()
                .actions
                .iter()
                .all(|a| a.state == "completed")
        );
        assert_eq!(fs::read(root.join("r/a")).unwrap(), b"original");
    }

    #[test]
    fn actual_process_crash_retains_partial_work_and_resumes_a_tree_mirror() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        for p in ["l/new/empty", "r/old/empty", "b", "undo"] {
            fs::create_dir_all(root.join(p)).unwrap();
        }
        fs::write(root.join("l/new/file"), vec![b'n'; 256 * 1024]).unwrap();
        fs::write(root.join("r/old/file"), b"original").unwrap();
        fs::write(root.join("r/keep.txt"), b"excluded").unwrap();
        struct Child(std::process::Child);
        impl Drop for Child {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let mut child = Child(
            Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "sync::resume::crash_tests::crash_fixture",
                    "--nocapture",
                ])
                .env(ENV, root)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        let deadline = Instant::now() + Duration::from_secs(25);
        while !root.join("ready").exists() {
            assert!(
                child.0.try_wait().unwrap().is_none(),
                "crash fixture exited prematurely"
            );
            assert!(Instant::now() < deadline, "crash fixture timed out");
            std::thread::sleep(Duration::from_millis(10));
        }
        child.0.kill().unwrap();
        child.0.wait().unwrap();
        let journal = PathBuf::from(fs::read_to_string(root.join("ready")).unwrap());
        // A new excluded child must not disappear inside a formerly removable directory.
        fs::write(root.join("r/old/keep.txt"), b"new excluded child").unwrap();
        assert!(super::preview(&journal, &AtomicBool::new(false)).is_err());
        assert_eq!(
            fs::read(root.join("r/old/keep.txt")).unwrap(),
            b"new excluded child"
        );
        fs::remove_file(root.join("r/old/keep.txt")).unwrap();
        let preview = super::preview(&journal, &AtomicBool::new(false)).unwrap();
        assert_eq!(preview.view.cleanup_files, 1);
        assert!(preview.view.satisfied > 0);
        assert!(preview.view.pending > 0);
        let result = super::execute(&preview, &AtomicBool::new(false)).unwrap();
        assert!(
            result.actions.iter().all(|a| a.state == "completed"),
            "{result:?}"
        );
        assert_eq!(
            fs::read(root.join("r/new/file")).unwrap(),
            vec![b'n'; 256 * 1024]
        );
        assert!(root.join("r/new/empty").is_dir());
        assert!(!root.join("r/old").exists());
        assert_eq!(fs::read(root.join("r/keep.txt")).unwrap(), b"excluded");
        let retained = Path::new(&preview.view.backup_root).join("retained-temporaries/0.bin");
        assert_eq!(fs::metadata(retained).unwrap().len(), 65536);
        let rollback =
            crate::recovery::preview(&journal, &root.join("undo"), &AtomicBool::new(false))
                .unwrap();
        let undone = crate::recovery::execute(&rollback, &AtomicBool::new(false)).unwrap();
        assert!(
            undone.actions.iter().all(|a| a.state == "completed"),
            "{undone:?}"
        );
        assert_eq!(fs::read(root.join("r/old/file")).unwrap(), b"original");
        assert!(root.join("r/old/empty").is_dir());
        assert!(!root.join("r/new").exists());
    }
}
