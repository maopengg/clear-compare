//! Three-way directory merge into a separate, initially empty output tree.
use crate::{
    Error, Result,
    compare::{canonical_root, check_cancel, checked_path, hash_file, reject_overlap, scan},
    editing, merge, native_path,
    rules::Rules,
    sync::atomic_json,
    tree::display_path,
    unique_id,
};
use clear_contracts::{
    ActionResult, Entry, FolderMergePreview, FolderMergeRequest, FolderMergeResult, FolderMergeRow,
    FolderResolution as Resolution, Kind,
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
use tempfile::NamedTempFile;
struct Record {
    relative: PathBuf,
    entries: [Option<Entry>; 3],
    row: FolderMergeRow,
    generated: Option<Vec<u8>>,
}
pub struct Plan {
    roots: [PathBuf; 3],
    output: PathBuf,
    backup_base: PathBuf,
    id: String,
    records: Vec<Record>,
    rules: Rules,
    two_way: bool,
}
fn equivalent(a: Option<&Entry>, b: Option<&Entry>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) if a.error.is_none() && b.error.is_none() && a.kind == b.kind => {
            match a.kind {
                Kind::Directory => true,
                Kind::File => a.digest.is_some() && a.digest == b.digest,
                _ => false,
            }
        }
        _ => false,
    }
}
fn supported(e: &Option<Entry>) -> bool {
    e.as_ref()
        .is_none_or(|e| e.error.is_none() && matches!(e.kind, Kind::File | Kind::Directory))
}
fn source(resolution: &Resolution) -> Option<usize> {
    match resolution {
        Resolution::Base => Some(0),
        Resolution::Left => Some(1),
        Resolution::Right => Some(2),
        _ => None,
    }
}
fn text_bytes(root: &Path, relative: &Path, entry: &Entry) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(checked_path(root, relative, false)?)?
        .take((editing::MAX_DOCUMENT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > editing::MAX_DOCUMENT_BYTES
        || Some(format!("{:x}", Sha256::digest(&bytes))) != entry.digest
    {
        return Err(Error::Invalid(
            "Text source changed or exceeds merge limit".into(),
        ));
    }
    Ok(bytes)
}
fn auto_text(
    roots: &[PathBuf; 3],
    relative: &Path,
    entries: &[Option<Entry>; 3],
) -> Result<Vec<u8>> {
    let b = text_bytes(
        &roots[0],
        relative,
        entries[0]
            .as_ref()
            .ok_or_else(|| Error::Invalid("No text baseline".into()))?,
    )?;
    let l = text_bytes(
        &roots[1],
        relative,
        entries[1]
            .as_ref()
            .ok_or_else(|| Error::Invalid("No left text".into()))?,
    )?;
    let r = text_bytes(
        &roots[2],
        relative,
        entries[2]
            .as_ref()
            .ok_or_else(|| Error::Invalid("No right text".into()))?,
    )?;
    let merged = merge::three_way(
        &editing::decode(&b)?.0,
        &editing::decode(&l)?.0,
        &editing::decode(&r)?.0,
    )?;
    if merged.conflicts > 0 {
        return Err(Error::Invalid(format!(
            "{} overlapping text conflict groups",
            merged.conflicts
        )));
    }
    let document = editing::open_file(&checked_path(&roots[1], relative, false)?)?;
    if Some(&document.view.digest) != entries[1].as_ref().and_then(|e| e.digest.as_ref()) {
        return Err(Error::Invalid("Left text changed".into()));
    }
    editing::encode(&merged.text, &document.view)
}
fn empty_output(output: &Path) -> Result<()> {
    if fs::read_dir(output)?.next().is_some() {
        return Err(Error::Invalid("Folder merge output must be empty".into()));
    }
    Ok(())
}
pub fn prepare(
    request: &FolderMergeRequest,
    backup_base: &Path,
    cancel: &AtomicBool,
) -> Result<Plan> {
    let roots = [
        if request.two_way {
            PathBuf::new()
        } else {
            canonical_root(&request.base)?
        },
        canonical_root(&request.left)?,
        canonical_root(&request.right)?,
    ];
    let output = canonical_root(&request.output)?;
    for i in usize::from(request.two_way)..3 {
        for j in i + 1..3 {
            reject_overlap(&roots[i], &roots[j])?;
        }
        reject_overlap(&roots[i], &output)?;
    }
    empty_output(&output)?;
    let backup_base = fs::canonicalize(backup_base)?;
    if backup_base.to_str().is_none() {
        return Err(Error::Invalid(
            "Merge journal directory must have a Unicode path".into(),
        ));
    }
    for root in roots
        .iter()
        .skip(usize::from(request.two_way))
        .chain(std::iter::once(&output))
    {
        reject_overlap(&backup_base, root)?;
    }
    let rules = Rules::parse(&request.exclusions)?;
    let maps = [
        if request.two_way {
            BTreeMap::new()
        } else {
            scan(&roots[0], &rules, true, cancel)?
        },
        scan(&roots[1], &rules, true, cancel)?,
        scan(&roots[2], &rules, true, cancel)?,
    ];
    let all: BTreeSet<PathBuf> = maps.iter().flat_map(|m| m.keys().cloned()).collect();
    if all.len() > 200000 {
        return Err(Error::Invalid(
            "Folder merge union exceeds 200000 entries".into(),
        ));
    }
    let pairs = [(0, 1), (0, 2), (1, 2)];
    let mut changed: [BTreeSet<PathBuf>; 3] = std::array::from_fn(|_| BTreeSet::new());
    for path in &all {
        check_cancel(cancel)?;
        for (index, (a, b)) in pairs.iter().enumerate() {
            if !equivalent(maps[*a].get(path), maps[*b].get(path)) {
                for parent in path.ancestors().skip(1) {
                    if parent.as_os_str().is_empty() {
                        break;
                    }
                    changed[index].insert(parent.to_path_buf());
                }
            }
        }
    }
    let mut records = Vec::new();
    let mut generated_bytes = 0usize;
    for (index, path) in all.into_iter().enumerate() {
        check_cancel(cancel)?;
        let entries: [Option<Entry>; 3] = std::array::from_fn(|i| maps[i].get(&path).cloned());
        let equal = |a: usize, b: usize, pair: usize| {
            equivalent(entries[a].as_ref(), entries[b].as_ref()) && !changed[pair].contains(&path)
        };
        let (bl, br, lr) = (equal(0, 1, 0), equal(0, 2, 1), equal(1, 2, 2));
        let mut generated = None;
        let mut reason = None;
        let (classification, resolution) = if !entries.iter().all(supported) {
            reason=Some("Unreadable entries, links and special files require an explicit supported selection or omission".into());
            ("conflict", Resolution::Conflict)
        } else if bl && br {
            (
                "unchanged",
                if entries[1].is_some() {
                    Resolution::Left
                } else {
                    Resolution::Delete
                },
            )
        } else if lr {
            (
                "same_change",
                if entries[1].is_some() {
                    Resolution::Left
                } else {
                    Resolution::Delete
                },
            )
        } else if bl {
            (
                "right_changed",
                if entries[2].is_some() {
                    Resolution::Right
                } else {
                    Resolution::Delete
                },
            )
        } else if br {
            (
                "left_changed",
                if entries[1].is_some() {
                    Resolution::Left
                } else {
                    Resolution::Delete
                },
            )
        } else if entries[1]
            .as_ref()
            .is_some_and(|e| e.kind == Kind::Directory)
            && entries[2]
                .as_ref()
                .is_some_and(|e| e.kind == Kind::Directory)
        {
            ("combined_directory", Resolution::Combine)
        } else if entries
            .iter()
            .all(|e| e.as_ref().is_some_and(|e| e.kind == Kind::File))
        {
            match auto_text(&roots, &path, &entries) {
                Ok(bytes) if generated_bytes + bytes.len() <= 64 * 1024 * 1024 => {
                    generated_bytes += bytes.len();
                    generated = Some(bytes);
                    ("auto_merged_text", Resolution::Auto)
                }
                Ok(_) => {
                    reason = Some(
                        "Automatic merge buffers exceed 64 MiB; choose a version explicitly".into(),
                    );
                    ("conflict", Resolution::Conflict)
                }
                Err(e) => {
                    reason = Some(e.to_string());
                    ("conflict", Resolution::Conflict)
                }
            }
        } else {
            reason = Some(
                if entries[0].is_some() && (entries[1].is_none() || entries[2].is_none()) {
                    "Deletion versus modification"
                } else {
                    "Competing additions or entry type changes"
                }
                .into(),
            );
            ("conflict", Resolution::Conflict)
        };
        records.push(Record {
            relative: path.clone(),
            entries,
            row: FolderMergeRow {
                id: index.to_string(),
                path: display_path(&path),
                classification: classification.into(),
                resolution,
                reason,
            },
            generated,
        });
    }
    // Only unique matching digests imply a probable rename; ambiguous duplicate content is not guessed.
    let mut base_digests = BTreeMap::<String, Vec<PathBuf>>::new();
    for (path, e) in &maps[0] {
        if e.kind == Kind::File
            && let Some(d) = &e.digest
        {
            base_digests
                .entry(d.clone())
                .or_default()
                .push(path.clone());
        }
    }
    for side in [1, 2] {
        let other = if side == 1 { 2 } else { 1 };
        let mut added = BTreeMap::<String, Vec<PathBuf>>::new();
        for (path, e) in &maps[side] {
            if !maps[0].contains_key(path)
                && e.kind == Kind::File
                && let Some(d) = &e.digest
            {
                added.entry(d.clone()).or_default().push(path.clone());
            }
        }
        for (digest, new) in added {
            let Some(old) = base_digests
                .get(&digest)
                .filter(|p| p.len() == 1 && new.len() == 1)
            else {
                continue;
            };
            let old = &old[0];
            if maps[side].contains_key(old) || equivalent(maps[0].get(old), maps[other].get(old)) {
                continue;
            }
            for record in records
                .iter_mut()
                .filter(|r| &r.relative == old || r.relative == new[0])
            {
                record.row.classification = "rename_modify_conflict".into();
                record.row.resolution = Resolution::Conflict;
                record.row.reason = Some(format!(
                    "Probable rename {} → {} conflicts with changes on the other side",
                    display_path(old),
                    display_path(&new[0])
                ));
            }
        }
    }
    Ok(Plan {
        roots,
        output,
        backup_base,
        id: unique_id(),
        records,
        rules,
        two_way: request.two_way,
    })
}
impl Plan {
    pub fn preview(&self, offset: usize) -> FolderMergePreview {
        FolderMergePreview {
            id: self.id.clone(),
            two_way: self.two_way,
            base: if self.two_way {
                String::new()
            } else {
                self.roots[0].to_string_lossy().into()
            },
            left: self.roots[1].to_string_lossy().into(),
            right: self.roots[2].to_string_lossy().into(),
            output: self.output.to_string_lossy().into(),
            backup_root: self.backup_base.join(&self.id).to_string_lossy().into(),
            conflicts: self
                .records
                .iter()
                .filter(|r| r.row.resolution == Resolution::Conflict)
                .count(),
            total: self.records.len(),
            rows: self
                .records
                .iter()
                .skip(offset)
                .take(500)
                .map(|r| r.row.clone())
                .collect(),
        }
    }
    pub fn resolve_path(&mut self, path: &Path, resolution: Resolution) -> Result<()> {
        let id = self
            .records
            .iter()
            .find(|r| r.relative == path)
            .map(|r| r.row.id.clone())
            .ok_or_else(|| Error::Invalid("Choice path is not a scanned merge entry".into()))?;
        self.resolve(&id, resolution)
    }
    pub fn resolve(&mut self, id: &str, resolution: Resolution) -> Result<()> {
        let index = self
            .records
            .iter()
            .position(|r| r.row.id == id)
            .ok_or_else(|| Error::Invalid("Unknown merge entry".into()))?;
        if resolution == Resolution::Combine
            && !self.records[index].entries[1..]
                .iter()
                .all(|e| e.as_ref().is_some_and(|e| e.kind == Kind::Directory))
        {
            return Err(Error::Invalid(
                "Combining child choices requires directories on both sides".into(),
            ));
        }
        if resolution == Resolution::Conflict {
            return Err(Error::Invalid(
                "Choose a supported version or omission".into(),
            ));
        }
        if resolution == Resolution::Auto && self.records[index].generated.is_none() {
            return Err(Error::Invalid(
                "No conflict-free automatic text result".into(),
            ));
        }
        let relative = self.records[index].relative.clone();
        let cascade = self.records[index]
            .entries
            .iter()
            .any(|e| e.as_ref().is_some_and(|e| e.kind == Kind::Directory))
            && !matches!(resolution, Resolution::Auto | Resolution::Combine);
        let affected = self
            .records
            .iter()
            .enumerate()
            .filter(|(i, r)| *i == index || (cascade && r.relative.starts_with(&relative)))
            .map(|(i, _)| i)
            .collect::<Vec<_>>();
        for i in &affected {
            if let Some(s) = source(&resolution)
                && !supported(&self.records[*i].entries[s])
            {
                return Err(Error::Invalid("Selected subtree contains unsupported entries; resolve children individually or omit the subtree".into()));
            }
        }
        for i in affected {
            self.records[i].row.resolution =
                if source(&resolution).is_some_and(|s| self.records[i].entries[s].is_none()) {
                    Resolution::Delete
                } else {
                    resolution.clone()
                };
        }
        self.id = unique_id();
        Ok(())
    }
}
fn root_valid(path: &Path) -> Result<()> {
    if fs::canonicalize(path)? != path {
        return Err(Error::Invalid("Merge root changed".into()));
    }
    Ok(())
}
fn chosen_kind(record: &Record) -> Option<Kind> {
    if record.row.resolution == Resolution::Combine {
        Some(Kind::Directory)
    } else if record.row.resolution == Resolution::Auto {
        Some(Kind::File)
    } else {
        source(&record.row.resolution)
            .and_then(|s| record.entries[s].as_ref().map(|e| e.kind.clone()))
    }
}
fn copy_record(plan: &Plan, record: &Record, cancel: &AtomicBool) -> Result<()> {
    check_cancel(cancel)?;
    root_valid(&plan.output)?;
    let target = checked_path(&plan.output, &record.relative, true)?;
    if chosen_kind(record) == Some(Kind::Directory) {
        match fs::create_dir(&target) {
            Ok(()) => return Ok(()),
            Err(e)
                if e.kind() == std::io::ErrorKind::AlreadyExists
                    && fs::symlink_metadata(&target)?.is_dir() =>
            {
                return Ok(());
            }
            Err(e) => return Err(e.into()),
        }
    }
    let parent = target
        .parent()
        .ok_or_else(|| Error::Invalid("Invalid output parent".into()))?;
    fs::create_dir_all(parent)?;
    checked_path(&plan.output, &record.relative, true)?;
    let mut temp = NamedTempFile::new_in(parent)?;
    let expected = if record.row.resolution == Resolution::Auto {
        for i in 0..3 {
            root_valid(&plan.roots[i])?;
            let p = checked_path(&plan.roots[i], &record.relative, false)?;
            if Some(hash_file(&p, cancel)?)
                != record.entries[i].as_ref().and_then(|e| e.digest.clone())
            {
                return Err(Error::Invalid(
                    "Automatic text source changed after preview".into(),
                ));
            }
        }
        let bytes = record
            .generated
            .as_ref()
            .ok_or_else(|| Error::Invalid("Missing automatic text result".into()))?;
        temp.write_all(bytes)?;
        format!("{:x}", Sha256::digest(bytes))
    } else {
        let index = source(&record.row.resolution)
            .ok_or_else(|| Error::Invalid("Unresolved merge entry".into()))?;
        root_valid(&plan.roots[index])?;
        let path = checked_path(&plan.roots[index], &record.relative, false)?;
        let expected = record.entries[index]
            .as_ref()
            .and_then(|e| e.digest.clone())
            .ok_or_else(|| Error::Invalid("Missing source digest".into()))?;
        if hash_file(&path, cancel)? != expected {
            return Err(Error::Invalid("Merge source changed after preview".into()));
        }
        let mut reader = File::open(&path)?;
        let mut chunk = [0u8; 65536];
        loop {
            check_cancel(cancel)?;
            let n = reader.read(&mut chunk)?;
            if n == 0 {
                break;
            }
            temp.write_all(&chunk[..n])?;
        }
        drop(reader);
        temp.as_file()
            .set_permissions(fs::metadata(path)?.permissions())?;
        expected
    };
    temp.as_file().sync_all()?;
    if hash_file(temp.path(), cancel)? != expected {
        return Err(Error::Invalid(
            "Merged output failed staging verification".into(),
        ));
    }
    check_cancel(cancel)?;
    root_valid(&plan.output)?;
    checked_path(&plan.output, &record.relative, true)?;
    temp.persist_noclobber(&target).map_err(|e| e.error)?;
    if hash_file(&target, &AtomicBool::new(false))? != expected {
        return Err(Error::Invalid(
            "Merged output failed final verification".into(),
        ));
    }
    Ok(())
}
pub fn execute(plan: &Plan, cancel: &AtomicBool) -> Result<FolderMergeResult> {
    let preview = plan.preview(0);
    if preview.conflicts > 0 {
        return Err(Error::Invalid(
            "Resolve every folder-merge conflict before creating output".into(),
        ));
    }
    for root in plan
        .roots
        .iter()
        .skip(usize::from(plan.two_way))
        .chain(std::iter::once(&plan.output))
    {
        root_valid(root)?;
    }
    empty_output(&plan.output)?;
    for side in usize::from(plan.two_way)..3 {
        let current = scan(&plan.roots[side], &plan.rules, true, cancel)?;
        let expected = plan
            .records
            .iter()
            .filter(|r| r.entries[side].is_some())
            .count();
        if current.len() != expected {
            return Err(Error::Invalid(
                "Merge source tree changed after preview".into(),
            ));
        }
        for record in &plan.records {
            let (old, new) = (record.entries[side].as_ref(), current.get(&record.relative));
            let stable = match (old, new) {
                (None, None) => true,
                (Some(a), Some(b)) if a.kind == b.kind => {
                    if a.kind == Kind::File && a.digest.is_some() {
                        a.digest == b.digest && b.error.is_none()
                    } else if a.kind == Kind::Directory {
                        a.error == b.error
                    } else {
                        a.bytes == b.bytes && a.modified_ms == b.modified_ms && a.error == b.error
                    }
                }
                _ => false,
            };
            if !stable {
                return Err(Error::Invalid(format!(
                    "Merge source changed after preview: {}",
                    record.row.path
                )));
            }
        }
    }
    root_valid(&plan.backup_base)?;
    let selected = plan
        .records
        .iter()
        .filter(|r| chosen_kind(r).is_some())
        .map(|r| (&r.relative, chosen_kind(r).unwrap()))
        .collect::<BTreeMap<_, _>>();
    for path in selected.keys() {
        for parent in path.ancestors().skip(1) {
            if parent.as_os_str().is_empty() {
                break;
            }
            if !selected
                .get(&parent.to_path_buf())
                .is_some_and(|k| *k == Kind::Directory)
            {
                return Err(Error::Invalid(
                    "Chosen entry conflicts with an omitted or non-directory parent".into(),
                ));
            }
        }
    }
    let backup = plan.backup_base.join(&plan.id);
    fs::create_dir(&backup)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&backup, fs::Permissions::from_mode(0o700))?;
    }
    let manifest = serde_json::json!({"version":1,"two_way":plan.two_way,"sources":plan.roots.iter().enumerate().map(|(i,p)|if plan.two_way &&i==0{None}else{Some(native_path::encode(p))}).collect::<Vec<_>>(),"output":native_path::encode(&plan.output),"entries":plan.records.iter().map(|r|serde_json::json!({"relative":native_path::encode(&r.relative),"resolution":r.row.resolution,"sources":r.entries,"generated_digest":r.generated.as_ref().map(|b|format!("{:x}",Sha256::digest(b))),"generated_file":r.generated.as_ref().map(|_|format!("generated/{}.bin",r.row.id))})).collect::<Vec<_>>()});
    atomic_json(&backup.join("manifest.json"), &manifest)?;
    for record in &plan.records {
        if record.row.resolution == Resolution::Auto {
            let generated = backup.join("generated");
            fs::create_dir_all(&generated)?;
            let mut file = File::create_new(generated.join(format!("{}.bin", record.row.id)))?;
            file.write_all(
                record
                    .generated
                    .as_ref()
                    .ok_or_else(|| Error::Invalid("Missing automatic result".into()))?,
            )?;
            file.sync_all()?;
        }
    }
    let journal = backup.join("journal.json");
    let mut result = FolderMergeResult {
        plan: preview,
        actions: plan
            .records
            .iter()
            .map(|r| ActionResult {
                path: r.row.path.clone(),
                state: if chosen_kind(r).is_some() {
                    "pending"
                } else {
                    "omitted"
                }
                .into(),
                error: None,
            })
            .collect(),
        journal: journal.to_string_lossy().into(),
    };
    atomic_json(&journal, &result)?;
    for (i, record) in plan.records.iter().enumerate() {
        if chosen_kind(record).is_none() {
            continue;
        }
        if let Err(e) = check_cancel(cancel) {
            result.actions[i].state = "cancelled".into();
            result.actions[i].error = Some(e.to_string());
            atomic_json(&journal, &result)?;
            break;
        }
        result.actions[i].state = "running".into();
        atomic_json(&journal, &result)?;
        match copy_record(plan, record, cancel) {
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
    if result
        .actions
        .iter()
        .all(|a| matches!(a.state.as_str(), "completed" | "omitted"))
    {
        let validation = (|| -> Result<()> {
            let actual = scan(
                &plan.output,
                &Rules::parse("")?,
                true,
                &AtomicBool::new(false),
            )?;
            if actual.len() != selected.len() {
                return Err(Error::Invalid(
                    "Output contains missing or unexpected entries".into(),
                ));
            }
            for record in &plan.records {
                let Some(kind) = chosen_kind(record) else {
                    continue;
                };
                let actual = actual
                    .get(&record.relative)
                    .ok_or_else(|| Error::Invalid("Merged output entry disappeared".into()))?;
                if actual.kind != kind || actual.error.is_some() {
                    return Err(Error::Invalid(
                        "Merged output entry changed type or became unreadable".into(),
                    ));
                }
                if kind == Kind::File {
                    let expected = if record.row.resolution == Resolution::Auto {
                        record
                            .generated
                            .as_ref()
                            .map(|b| format!("{:x}", Sha256::digest(b)))
                    } else {
                        source(&record.row.resolution)
                            .and_then(|i| record.entries[i].as_ref().and_then(|e| e.digest.clone()))
                    };
                    if actual.digest != expected {
                        return Err(Error::Invalid(
                            "Merged output changed during execution".into(),
                        ));
                    }
                }
            }
            Ok(())
        })();
        if let Err(e) = validation {
            result.actions.push(ActionResult {
                path: "Output validation".into(),
                state: "failed".into(),
                error: Some(e.to_string()),
            });
            atomic_json(&journal, &result)?;
        }
    }
    Ok(result)
}
