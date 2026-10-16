//! Immutable restart intent, per-attempt artifact roots and owned work-file records.
use super::*;
use crate::native_path;
use clear_contracts::{
    Comparison, ComparisonPolicy, DirectoryIdentity, Entry, NativePath, Row, Summary,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::io::{BufWriter, Read};
pub const MAX_INTENT: u64 = 128 * 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Object {
    pub kind: Kind,
    pub identity: DirectoryIdentity,
    pub permissions: FilePermissions,
}
impl Object {
    pub fn observe(root: &Path, relative: &Path) -> Result<Option<Self>> {
        let path = checked_path(root, relative, true)?;
        let metadata = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(e.into()),
        };
        let kind = if metadata.is_file() {
            Kind::File
        } else if metadata.is_dir() {
            Kind::Directory
        } else {
            return Err(Error::Invalid(
                "Restart evidence refuses links/special entries".into(),
            ));
        };
        let mut identity = directory::identity(&path, &metadata)?;
        // NTFS tunneling can change a renamed file's creation time while its file ID stays stable.
        if cfg!(windows) && metadata.is_file() {
            identity.birth = None;
        }
        directory::validate_identity(&identity)?;
        Ok(Some(Self {
            kind,
            identity,
            permissions: permissions(&metadata),
        }))
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Pin {
    pub source: Option<Object>,
    pub target: Option<Object>,
}
pub struct Constraints {
    pub left: Object,
    pub right: Object,
    pub pins: BTreeMap<String, Pin>,
}
pub fn verify_restart(snapshot: &Snapshot, plan: &SyncPlan, action: &FileAction) -> Result<()> {
    let Some(constraints) = &snapshot.restart else {
        return Ok(());
    };
    verify_roots(snapshot)?;
    let pin = constraints
        .pins
        .get(&action.row_id)
        .ok_or_else(|| Error::Invalid("Restart identity pin missing".into()))?;
    let relative = snapshot
        .paths
        .get(&action.row_id)
        .ok_or_else(|| Error::Invalid("Restart path missing".into()))?;
    let target = target_root(snapshot, plan);
    let source = if target == snapshot.left {
        &snapshot.right
    } else {
        &snapshot.left
    };
    if Object::observe(target, relative)? != pin.target {
        return Err(Error::Invalid(
            "Restart target identity/permissions changed before mutation".into(),
        ));
    }
    if matches!(
        action.operation,
        FileOperation::Copy | FileOperation::CreateDirectory
    ) && Object::observe(source, relative)? != pin.source
    {
        return Err(Error::Invalid(
            "Restart source identity/permissions changed before mutation".into(),
        ));
    }
    Ok(())
}
pub fn verify_roots(snapshot: &Snapshot) -> Result<()> {
    if let Some(c) = &snapshot.restart {
        for (root, pin) in [(&snapshot.left, &c.left), (&snapshot.right, &c.right)] {
            if fs::canonicalize(root)? != *root
                || Object::observe(root, Path::new(""))?.as_ref() != Some(pin)
            {
                return Err(Error::Invalid(
                    "Restart root identity/permissions changed".into(),
                ));
            }
        }
    }
    Ok(())
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Captured {
    pub id: String,
    pub relative: NativePath,
    pub left: Option<Entry>,
    pub right: Option<Entry>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    pub version: u32,
    pub plan: SyncPlan,
    pub left: NativePath,
    pub right: NativePath,
    pub left_root: Option<Object>,
    pub right_root: Option<Object>,
    pub policy: ComparisonPolicy,
    pub rows: Vec<Captured>,
    pub pins: Vec<Pin>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Temporary {
    pub version: u32,
    pub row_id: String,
    pub namespace: Option<String>,
    pub relative: NativePath,
    pub object: Object,
}
pub fn temporaries(root: &Path, plan: &SyncPlan, cancel: &AtomicBool) -> Result<Vec<Temporary>> {
    let folder = checked_path(root, Path::new("temporary-records"), true)?;
    if !folder.exists() {
        return Ok(Vec::new());
    }
    let by_id = plan
        .actions
        .iter()
        .map(|a| (a.row_id.as_str(), a))
        .collect::<BTreeMap<_, _>>();
    let mut output = Vec::new();
    let mut total = 0u64;
    for entry in fs::read_dir(folder)? {
        check_cancel(cancel)?;
        if output.len() >= crate::compare::MAX_ENTRIES * 4 {
            return Err(Error::Invalid("Too many owned work-file records".into()));
        }
        let entry = entry?;
        total = total.saturating_add(entry.metadata()?.len());
        if total > 256 * 1024 * 1024 {
            return Err(Error::Invalid(
                "Owned work-file records exceed 256 MiB".into(),
            ));
        }
        if !entry.file_type()?.is_file() {
            return Err(Error::Invalid("Invalid work-file record type".into()));
        }
        let (record, _): (Temporary, String) = read(&entry.path(), 65536)?;
        let action = by_id
            .get(record.row_id.as_str())
            .copied()
            .ok_or_else(|| Error::Invalid("Work-file record references another action".into()))?;
        let relative = native_path::decode(&record.relative)?;
        let original = native_path::decode(
            action
                .relative_native
                .as_ref()
                .ok_or_else(|| Error::Invalid("Missing work-file action path".into()))?,
        )?;
        if record.version != 1
            || action.operation != FileOperation::Copy
            || record.object.kind != Kind::File
            || relative.parent() != original.parent()
            || !relative
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(&format!(".clear-compare-{}-", plan.id)))
            || relative
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
        {
            return Err(Error::Invalid("Invalid owned work-file record".into()));
        }
        if let Some(id) = &record.namespace {
            namespace(id)?;
        }
        directory::validate_identity(&record.object.identity)?;
        output.push(record);
    }
    Ok(output)
}
pub fn namespace(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 96
        || !value.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
    {
        return Err(Error::Invalid("Invalid backup attempt namespace".into()));
    }
    Ok(())
}
pub fn action_root(plan: &SyncPlan, action: &FileAction) -> Result<PathBuf> {
    let root = Path::new(&plan.backup_root);
    if let Some(id) = &action.backup_namespace {
        namespace(id)?;
        checked_path(root, &PathBuf::from("attempts").join(id), false)
    } else {
        Ok(root.to_owned())
    }
}
pub fn lock(root: &Path, create: bool) -> Result<File> {
    if !fs::symlink_metadata(root)?.is_dir() || fs::canonicalize(root)? != root {
        return Err(Error::Invalid("Operation backup root changed".into()));
    }
    let path = checked_path(root, Path::new("operation.lock"), create)?;
    let file = File::options()
        .read(true)
        .write(true)
        .create(create)
        .truncate(false)
        .open(path)?;
    file.try_lock().map_err(|e| {
        Error::Invalid(format!(
            "Operation backup set is in use or cannot be locked: {e}"
        ))
    })?;
    Ok(file)
}
pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn bytes(path: &Path, limit: u64) -> Result<Vec<u8>> {
    if !fs::symlink_metadata(path)?.is_file() {
        return Err(Error::Invalid(
            "Restart metadata is not a regular file".into(),
        ));
    }
    let mut data = Vec::new();
    File::open(path)?.take(limit + 1).read_to_end(&mut data)?;
    if data.len() as u64 > limit {
        return Err(Error::Invalid(
            "Restart metadata exceeds its size limit".into(),
        ));
    }
    Ok(data)
}
pub fn read<T: DeserializeOwned>(path: &Path, limit: u64) -> Result<(T, String)> {
    let data = bytes(path, limit)?;
    Ok((serde_json::from_slice(&data)?, digest(&data)))
}
pub fn write<T: Serialize>(path: &Path, value: &T, limit: u64) -> Result<String> {
    struct Limited<'a> {
        file: &'a mut dyn Write,
        left: u64,
    }
    impl Write for Limited<'_> {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            if bytes.len() as u64 > self.left {
                return Err(std::io::Error::other(
                    "Restart metadata exceeds its size limit",
                ));
            }
            self.file.write_all(bytes)?;
            self.left -= bytes.len() as u64;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            self.file.flush()
        }
    }
    let parent = path
        .parent()
        .ok_or_else(|| Error::Invalid("Restart metadata parent missing".into()))?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    {
        let mut buffered = BufWriter::with_capacity(64 * 1024, temporary.as_file_mut());
        serde_json::to_writer_pretty(
            Limited {
                file: &mut buffered,
                left: limit,
            },
            value,
        )?;
        buffered.flush()?;
    }
    temporary.as_file().sync_all()?;
    temporary.persist_noclobber(path).map_err(|e| e.error)?;
    flush_directory(parent)?;
    let mut data = Vec::new();
    File::open(path)?.take(limit + 1).read_to_end(&mut data)?;
    if data.len() as u64 > limit {
        return Err(Error::Invalid(
            "Restart metadata exceeds its size limit".into(),
        ));
    }
    Ok(digest(&data))
}
pub fn intent_matches(effective: &SyncPlan, original: &SyncPlan) -> bool {
    fn normalize(p: &SyncPlan) -> SyncPlan {
        let mut p = p.clone();
        p.target.clear();
        p.warnings.clear();
        for a in &mut p.actions {
            a.backup_namespace = None;
            a.path.clear();
            if let Some(rename) = &mut a.rename_to {
                rename.destination.clear();
            }
        }
        p
    }
    normalize(effective) == normalize(original)
}
pub fn capture(snapshot: &Snapshot, plan: &SyncPlan) -> Result<Intent> {
    let target = target_root(snapshot, plan);
    let source = if target == snapshot.left {
        &snapshot.right
    } else {
        &snapshot.left
    };
    let pins = plan
        .actions
        .iter()
        .map(|action| {
            let relative = snapshot
                .paths
                .get(&action.row_id)
                .ok_or_else(|| Error::Invalid("Restart entry missing".into()))?;
            if action.relative_native.as_ref() != Some(&native_path::encode(relative)) {
                return Err(Error::Invalid("Restart action native path differs".into()));
            }
            Ok(Pin {
                source: if matches!(
                    action.operation,
                    FileOperation::Copy | FileOperation::CreateDirectory
                ) {
                    Object::observe(source, relative).ok().flatten()
                } else {
                    None
                },
                target: Object::observe(target, relative).ok().flatten(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    // Selected copies need only their own captured rows for replay. Mirrors retain
    // the complete inventory because their deletion policy depends on its scope.
    let selected = plan
        .actions
        .iter()
        .map(|a| a.row_id.as_str())
        .collect::<BTreeSet<_>>();
    let rows = snapshot
        .report
        .rows
        .iter()
        .filter(|row| {
            plan.policy != FilePlanPolicy::SelectedCopy || selected.contains(row.id.as_str())
        })
        .map(|row| {
            Ok(Captured {
                id: row.id.clone(),
                relative: native_path::encode(
                    snapshot
                        .paths
                        .get(&row.id)
                        .ok_or_else(|| Error::Invalid("Restart row missing".into()))?,
                ),
                left: row.left.clone(),
                right: row.right.clone(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Intent {
        version: 1,
        plan: plan.clone(),
        left: native_path::encode(&snapshot.left),
        right: native_path::encode(&snapshot.right),
        left_root: Object::observe(&snapshot.left, Path::new(""))
            .ok()
            .flatten(),
        right_root: Object::observe(&snapshot.right, Path::new(""))
            .ok()
            .flatten(),
        policy: snapshot
            .report
            .policy
            .clone()
            .ok_or_else(|| Error::Invalid("Restart policy missing".into()))?,
        rows,
        pins,
    })
}
impl Intent {
    pub fn snapshot(&self) -> Result<Snapshot> {
        if self.version != 1
            || self.rows.len() > crate::compare::MAX_ENTRIES * 2
            || self.pins.len() != self.plan.actions.len()
        {
            return Err(Error::Invalid("Invalid restart context".into()));
        }
        let left = native_path::decode(&self.left)?;
        let right = native_path::decode(&self.right)?;
        reject_overlap(&left, &right)?;
        for (root, pin) in [(&left, &self.left_root), (&right, &self.right_root)] {
            if !root.is_absolute()
                || fs::canonicalize(root)? != *root
                || pin.is_none()
                || Object::observe(root, Path::new(""))?.as_ref() != pin.as_ref()
            {
                return Err(Error::Invalid(
                    "Original root identity/permissions changed or are unavailable".into(),
                ));
            }
        }
        let mut paths = BTreeMap::new();
        let mut seen = BTreeSet::new();
        let mut rows = Vec::new();
        for row in &self.rows {
            let relative = native_path::decode(&row.relative)?;
            if relative.as_os_str().is_empty()
                || relative
                    .components()
                    .any(|c| !matches!(c, std::path::Component::Normal(_)))
                || !seen.insert(relative.clone())
                || paths.insert(row.id.clone(), relative.clone()).is_some()
            {
                return Err(Error::Invalid("Duplicate or unsafe restart entry".into()));
            }
            rows.push(Row {
                id: row.id.clone(),
                path: crate::tree::display_path(&relative),
                directory_summary: None,
                status: Status::MetadataMatch,
                left: row.left.clone(),
                right: row.right.clone(),
            });
        }
        for action in &self.plan.actions {
            let path = paths.get(&action.row_id).ok_or_else(|| {
                Error::Invalid("Restart intent references an unknown entry".into())
            })?;
            if action.relative_native.as_ref() != Some(&native_path::encode(path)) {
                return Err(Error::Invalid("Restart intent native path differs".into()));
            }
        }
        let directory_index = rows
            .iter()
            .filter(|r| crate::tree::is_directory(r))
            .filter_map(|r| paths.get(&r.id).map(|p| (p.clone(), r.id.clone())))
            .collect();
        Ok(Snapshot {
            restart: Some(std::sync::Arc::new(Constraints {
                left: self
                    .left_root
                    .clone()
                    .ok_or_else(|| Error::Invalid("Missing root identity".into()))?,
                right: self
                    .right_root
                    .clone()
                    .ok_or_else(|| Error::Invalid("Missing root identity".into()))?,
                pins: self
                    .plan
                    .actions
                    .iter()
                    .zip(&self.pins)
                    .map(|(a, p)| (a.row_id.clone(), p.clone()))
                    .collect(),
            })),
            left: left.clone(),
            right: right.clone(),
            paths,
            directory_index,
            rules: crate::rules::Rules::parse(&self.policy.exclusions)?,
            report: Comparison {
                policy: Some(self.policy.clone()),
                session: self.plan.session.clone(),
                left: left.to_string_lossy().into(),
                right: right.to_string_lossy().into(),
                summary: Summary::default(),
                rows,
                warnings: Vec::new(),
            },
        })
    }
}
pub fn register_temporary(
    snapshot: &Snapshot,
    plan: &SyncPlan,
    action: &FileAction,
    path: &Path,
) -> Result<()> {
    let run = Path::new(&plan.backup_root);
    if !run.join("intent.json").is_file() {
        return Ok(());
    }
    let root = target_root(snapshot, plan);
    let relative = path
        .strip_prefix(root)
        .map_err(|_| Error::Invalid("Temporary path escaped target root".into()))?;
    let Some(object) = Object::observe(root, relative).ok().flatten() else {
        return Ok(());
    };
    let folder = checked_path(run, Path::new("temporary-records"), true)?;
    fs::create_dir_all(&folder)?;
    let record = Temporary {
        version: 1,
        row_id: action.row_id.clone(),
        namespace: action.backup_namespace.clone(),
        relative: native_path::encode(relative),
        object,
    };
    write(
        &folder.join(format!("{}.json", unique_id())),
        &record,
        65536,
    )?;
    flush_directory(run)
}
