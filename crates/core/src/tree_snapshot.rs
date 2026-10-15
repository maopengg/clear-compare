//! Bounded, content-free directory inventories. Imported names never become write targets.
use crate::{
    Error, Result,
    compare::{check_cancel, scan},
    native_path,
    rules::Rules,
    tree, unique_id,
};
use clear_contracts::{
    ArchiveRequest, Comparison, Entry, Kind, NativePath, Page, Row, Status, Summary,
    TreeSnapshotInfo, TreeSnapshotRequest,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
const MAX_FILE: u64 = 64 * 1024 * 1024;
const MAX_ENTRIES: usize = crate::compare::MAX_ENTRIES;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    format: String,
    version: u32,
    captured_ms: String,
    source_name: String,
    exclusions: String,
    entries: Vec<Member>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Member {
    path: StoredPath,
    kind: Kind,
    bytes: String,
    modified_ms: Option<String>,
    error: Option<String>,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "encoding", content = "value", rename_all = "snake_case")]
enum StoredPath {
    Portable(Vec<String>),
    Native(NativePath),
}
fn valid_component(text: &str) -> bool {
    !text.is_empty() && !matches!(text, "." | "..") && !text.contains(['/', '\\', ':', '\0'])
}
fn validate_path(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty()
        || path.as_os_str().len() > 16384
        || path.components().count() > 256
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
        || path.as_os_str().as_encoded_bytes().contains(&0)
    {
        return Err(Error::Invalid("Invalid relative snapshot path".into()));
    }
    Ok(())
}
fn store_path(path: &Path) -> Result<StoredPath> {
    validate_path(path)?;
    let components = path
        .components()
        .map(|c| {
            c.as_os_str()
                .to_str()
                .filter(|s| valid_component(s))
                .map(str::to_owned)
        })
        .collect::<Option<Vec<_>>>();
    Ok(match components {
        Some(c) => StoredPath::Portable(c),
        None => StoredPath::Native(native_path::encode(path)),
    })
}
fn load_path(path: &StoredPath) -> Result<PathBuf> {
    let path = match path {
        StoredPath::Portable(parts) => {
            if parts.is_empty() || parts.len() > 256 || parts.iter().any(|s| !valid_component(s)) {
                return Err(Error::Invalid("Unsafe portable snapshot path".into()));
            }
            parts.iter().collect()
        }
        StoredPath::Native(native) => native_path::decode(native)?,
    };
    validate_path(&path)?;
    Ok(path)
}
fn decimal(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 20
        && text.bytes().all(|c| c.is_ascii_digit())
        && text.parse::<u64>().is_ok()
}
fn metadata(entry: &Entry, path: StoredPath) -> Member {
    Member {
        path,
        kind: entry.kind.clone(),
        bytes: entry.bytes.clone(),
        modified_ms: entry.modified_ms.clone(),
        error: entry.error.as_ref().map(|e| e.chars().take(4096).collect()),
    }
}
fn info(stored: &Stored) -> TreeSnapshotInfo {
    TreeSnapshotInfo {
        version: stored.version,
        captured_ms: stored.captured_ms.clone(),
        source_name: stored.source_name.clone(),
        entries: stored.entries.len(),
        errors: stored.entries.iter().filter(|e| e.error.is_some()).count(),
    }
}
struct BoundedWriter<'a> {
    file: &'a mut File,
    count: u64,
    cancel: &'a AtomicBool,
}
impl Write for BoundedWriter<'_> {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        if self.cancel.load(Ordering::Relaxed) {
            return Err(std::io::Error::other("Operation cancelled"));
        }
        if self.count.saturating_add(buffer.len() as u64) > MAX_FILE {
            return Err(std::io::Error::other("Snapshot exceeds 64 MiB"));
        }
        let n = self.file.write(buffer)?;
        self.count += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}
pub fn capture(request: &TreeSnapshotRequest, cancel: &AtomicBool) -> Result<TreeSnapshotInfo> {
    check_cancel(cancel)?;
    let original = Path::new(&request.root);
    if fs::symlink_metadata(original)?.file_type().is_symlink() {
        return Err(Error::Invalid(
            "Snapshot source must be a real directory".into(),
        ));
    }
    let root = crate::compare::canonical_root(&request.root)?;
    let destination = Path::new(&request.destination);
    let name = destination
        .file_name()
        .ok_or_else(|| Error::Invalid("Choose a snapshot filename".into()))?;
    let parent = fs::canonicalize(
        destination
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )?;
    if !parent.is_dir() || parent.starts_with(&root) {
        return Err(Error::Invalid(
            "Save snapshots outside the captured directory".into(),
        ));
    }
    let destination = parent.join(name);
    if destination.try_exists()? || fs::symlink_metadata(&destination).is_ok() {
        return Err(Error::Invalid(
            "Snapshot destination already exists. Choose a new file.".into(),
        ));
    }
    let rules = Rules::parse(&request.exclusions)?;
    let entries = scan(&root, &rules, false, cancel)?;
    if entries.len() > MAX_ENTRIES {
        return Err(Error::Invalid("Snapshot exceeds 200000 entries".into()));
    }
    let entries = entries
        .iter()
        .map(|(p, e)| Ok(metadata(e, store_path(p)?)))
        .collect::<Result<Vec<_>>>()?;
    let stored = Stored {
        format: "clear-compare-tree".into(),
        version: 1,
        captured_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .to_string(),
        source_name: root
            .file_name()
            .map(|s| s.to_string_lossy().chars().take(256).collect())
            .unwrap_or_else(|| "Root".into()),
        exclusions: request.exclusions.clone(),
        entries,
    };
    let result = info(&stored);
    let mut temp = tempfile::NamedTempFile::new_in(&parent)?;
    {
        let bounded = BoundedWriter {
            file: temp.as_file_mut(),
            count: 0,
            cancel,
        };
        let mut writer = std::io::BufWriter::new(bounded);
        serde_json::to_writer(&mut writer, &stored)?;
        writer.flush()?;
    }
    temp.as_file().sync_all()?;
    check_cancel(cancel)?;
    temp.persist_noclobber(&destination)
        .map_err(|e| Error::Io(e.error))?;
    Ok(result)
}
fn read(path: &Path, rules: &Rules, cancel: &AtomicBool) -> Result<BTreeMap<PathBuf, Entry>> {
    check_cancel(cancel)?;
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > MAX_FILE {
        return Err(Error::Invalid(
            "Snapshot must be a regular file of at most 64 MiB".into(),
        ));
    }
    let mut data = Vec::new();
    File::open(path)?
        .take(MAX_FILE + 1)
        .read_to_end(&mut data)?;
    if data.len() as u64 > MAX_FILE {
        return Err(Error::Invalid("Snapshot exceeds 64 MiB".into()));
    }
    let stored: Stored = serde_json::from_slice(&data)?;
    if stored.format != "clear-compare-tree"
        || stored.version != 1
        || stored.entries.len() > MAX_ENTRIES
        || !decimal(&stored.captured_ms)
        || stored.source_name.len() > 4096
    {
        return Err(Error::Invalid(
            "Unsupported or invalid snapshot envelope".into(),
        ));
    }
    Rules::parse(&stored.exclusions)?;
    let mut entries = BTreeMap::new();
    for member in stored.entries {
        check_cancel(cancel)?;
        let relative = load_path(&member.path)?;
        if !decimal(&member.bytes)
            || member.modified_ms.as_ref().is_some_and(|s| !decimal(s))
            || member.error.as_ref().is_some_and(|s| s.len() > 16384)
        {
            return Err(Error::Invalid("Invalid snapshot metadata".into()));
        }
        let entry = Entry {
            kind: member.kind,
            bytes: member.bytes,
            modified_ms: member.modified_ms,
            digest: None,
            error: member.error,
        };
        if entries.insert(relative, entry).is_some() {
            return Err(Error::Invalid("Snapshot contains duplicate paths".into()));
        }
    }
    // Validate the complete inventory before applying exclusions, including omitted ancestors.
    for path in entries.keys() {
        for parent in path
            .ancestors()
            .skip(1)
            .filter(|p| !p.as_os_str().is_empty())
        {
            if !entries
                .get(parent)
                .is_some_and(|e| e.kind == Kind::Directory)
            {
                return Err(Error::Invalid(
                    "Snapshot has a missing or invalid parent directory".into(),
                ));
            }
        }
    }
    entries.retain(|path, e| !rules.excludes(path, e.kind == Kind::Directory));
    Ok(entries)
}
fn load(input: &str, rules: &Rules, cancel: &AtomicBool) -> Result<BTreeMap<PathBuf, Entry>> {
    let path = Path::new(input);
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() {
        return Err(Error::Invalid(
            "Choose a real directory or snapshot file".into(),
        ));
    }
    if meta.is_dir() {
        scan(&fs::canonicalize(path)?, rules, false, cancel)
    } else {
        read(path, rules, cancel)
    }
}
pub struct Snapshot {
    roots: Vec<PathBuf>,
    pub report: Comparison,
}
pub fn compare(request: &ArchiveRequest, cancel: &AtomicBool) -> Result<Snapshot> {
    check_cancel(cancel)?;
    if request.content {
        return Err(Error::Invalid(
            "Tree snapshots contain metadata only; content verification is unavailable".into(),
        ));
    }
    let rules = Rules::parse(&request.exclusions)?;
    let roots = [&request.left, &request.right]
        .iter()
        .filter(|s| !s.trim().is_empty())
        .filter_map(|s| {
            fs::metadata(s)
                .ok()
                .filter(|m| m.is_dir())
                .map(|_| fs::canonicalize(s))
        })
        .collect::<std::io::Result<Vec<_>>>()?;
    let l = load(&request.left, &rules, cancel)?;
    let r = if request.right.trim().is_empty() {
        BTreeMap::new()
    } else {
        load(&request.right, &rules, cancel)?
    };
    let all = l.keys().chain(r.keys()).cloned().collect::<BTreeSet<_>>();
    let mut rows = Vec::new();
    let mut paths = BTreeMap::new();
    let mut summary = Summary::default();
    for (i, path) in all.into_iter().enumerate() {
        check_cancel(cancel)?;
        let a = l.get(&path).cloned();
        let b = r.get(&path).cloned();
        let mut state = crate::archive::status(a.as_ref(), b.as_ref());
        if state == Status::MetadataMatch
            && a.as_ref()
                .zip(b.as_ref())
                .is_some_and(|(a, b)| a.kind == Kind::File && a.modified_ms != b.modified_ms)
        {
            state = Status::Changed;
        }
        if !a
            .as_ref()
            .or(b.as_ref())
            .is_some_and(|e| e.kind == Kind::Directory)
            || matches!(state, Status::Error | Status::TypeConflict)
        {
            match state {
                Status::Same => summary.same += 1,
                Status::Changed => summary.changed += 1,
                Status::LeftOnly => summary.left_only += 1,
                Status::RightOnly => summary.right_only += 1,
                Status::MetadataMatch => summary.unverified += 1,
                _ => summary.errors += 1,
            }
        }
        let id = i.to_string();
        paths.insert(id.clone(), path.clone());
        rows.push(Row {
            id,
            path: tree::display_path(&path),
            directory_summary: None,
            status: state,
            left: a,
            right: b,
        });
    }
    tree::aggregate(&mut rows, &paths);
    Ok(Snapshot{roots,report:Comparison{policy:Some(clear_contracts::ComparisonPolicy{exclusions:request.exclusions.clone(),verification:clear_contracts::VerificationBasis::SizeAndTime}),session:unique_id(),left:request.left.clone(),right:request.right.clone(),summary,rows,warnings:vec!["Historical metadata inventory only. Size/time matches never verify file contents. No text preview, editing, synchronization or restore is available.".into()]}})
}
impl Snapshot {
    pub fn report_roots(&self) -> Vec<PathBuf> {
        self.roots.clone()
    }

    pub fn page(&self, offset: usize, search: &str, differences_only: bool) -> Result<Page> {
        crate::archive::page_report(&self.report, offset, search, differences_only)
    }
}
