use crate::{Error, Result, rules::Rules, unique_id};
use clear_contracts::{CompareRequest, Comparison, Entry, Kind, Row, Status, Summary};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    time::UNIX_EPOCH,
};
use walkdir::WalkDir;

pub const MAX_ENTRIES: usize = 200_000;

pub struct Snapshot {
    pub(crate) restart: Option<std::sync::Arc<crate::sync::durable::Constraints>>,
    pub report: Comparison,
    pub left: PathBuf,
    pub right: PathBuf,
    pub paths: BTreeMap<String, PathBuf>,
    pub directory_index: BTreeMap<PathBuf, String>,
    pub rules: Rules,
}

pub fn check_cancel(cancel: &AtomicBool) -> Result<()> {
    crate::task::checkpoint(cancel)
}

pub fn hash_file(path: &Path, cancel: &AtomicBool) -> Result<String> {
    if !fs::symlink_metadata(path)?.file_type().is_file() {
        return Err(Error::Invalid(
            "Expected a regular file; symbolic links are not followed".into(),
        ));
    }
    let mut reader = File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        check_cancel(cancel)?;
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        crate::task::read(n);
        digest.update(&buffer[..n]);
    }
    Ok(format!("{:x}", digest.finalize()))
}

pub fn canonical_root(input: &str) -> Result<PathBuf> {
    if input.trim().is_empty() {
        return Err(Error::Invalid("Select a directory".into()));
    }
    let path = fs::canonicalize(input)?;
    if !path.is_dir() {
        return Err(Error::Invalid("Expected a directory".into()));
    }
    Ok(path)
}

pub fn reject_overlap(left: &Path, right: &Path) -> Result<()> {
    if left.starts_with(right) || right.starts_with(left) {
        return Err(Error::Invalid(
            "Directories must be separate and must not contain one another".into(),
        ));
    }
    Ok(())
}

pub(crate) fn scan(
    root: &Path,
    rules: &Rules,
    content: bool,
    cancel: &AtomicBool,
) -> Result<BTreeMap<PathBuf, Entry>> {
    let mut entries = BTreeMap::new();
    let walker = WalkDir::new(root)
        .follow_links(false)
        .min_depth(1)
        .into_iter()
        .filter_entry(|entry| {
            entry
                .path()
                .strip_prefix(root)
                .is_ok_and(|path| !rules.excludes(path, entry.file_type().is_dir()))
        });
    for item in walker {
        check_cancel(cancel)?;
        let item = match item {
            Ok(item) => item,
            Err(error) => {
                let path = error
                    .path()
                    .and_then(|p| p.strip_prefix(root).ok())
                    .ok_or_else(|| Error::Invalid(error.to_string()))?;
                entries.insert(
                    path.to_owned(),
                    Entry {
                        kind: Kind::Other,
                        bytes: "0".into(),
                        modified_ms: None,
                        digest: None,
                        error: Some(error.to_string()),
                    },
                );
                continue;
            }
        };
        if entries.len() >= MAX_ENTRIES {
            return Err(Error::Invalid(format!(
                "Directory exceeds {MAX_ENTRIES} entries. Narrow the selection or add exclusions."
            )));
        }
        let relative = item
            .path()
            .strip_prefix(root)
            .map_err(|e| Error::Invalid(e.to_string()))?
            .to_owned();
        let kind = if item.file_type().is_symlink() {
            Kind::Link
        } else if item.file_type().is_dir() {
            Kind::Directory
        } else if item.file_type().is_file() {
            Kind::File
        } else {
            Kind::Other
        };
        let metadata = fs::symlink_metadata(item.path());
        let mut entry = match metadata {
            Ok(m) => Entry {
                kind,
                bytes: m.len().to_string(),
                modified_ms: m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                    .map(|t| t.as_millis().to_string()),
                digest: None,
                error: None,
            },
            Err(e) => Entry {
                kind,
                bytes: "0".into(),
                modified_ms: None,
                digest: None,
                error: Some(e.to_string()),
            },
        };
        if content && entry.kind == Kind::File && entry.error.is_none() {
            match hash_file(item.path(), cancel) {
                Ok(hash) => entry.digest = Some(hash),
                Err(Error::Cancelled) => return Err(Error::Cancelled),
                Err(e) => entry.error = Some(e.to_string()),
            }
        }
        entries.insert(relative, entry);
    }
    Ok(entries)
}

pub(crate) fn status(left: Option<&Entry>, right: Option<&Entry>) -> Status {
    if left.is_some_and(|e| e.error.is_some()) || right.is_some_and(|e| e.error.is_some()) {
        return Status::Error;
    }
    match (left, right) {
        (Some(_), None) => Status::LeftOnly,
        (None, Some(_)) => Status::RightOnly,
        (Some(a), Some(b)) if a.kind != b.kind => Status::TypeConflict,
        (Some(a), Some(_)) if a.kind == Kind::Directory => Status::Same,
        (Some(a), Some(_)) if a.kind != Kind::File => Status::MetadataMatch,
        (Some(a), Some(b)) if a.digest.is_some() && b.digest.is_some() => {
            if a.digest == b.digest {
                Status::Same
            } else {
                Status::Changed
            }
        }
        (Some(a), Some(b)) => {
            if a.bytes == b.bytes {
                Status::MetadataMatch
            } else {
                Status::Changed
            }
        }
        _ => Status::Error,
    }
}

pub fn compare(request: &CompareRequest, cancel: &AtomicBool) -> Result<Snapshot> {
    compare_inner(request, false, cancel)
}

/// Verify only pairs whose equal sizes leave their content equality unresolved.
/// One-sided files and unequal sizes already have a definite difference status.
pub fn compare_matching(request: &CompareRequest, cancel: &AtomicBool) -> Result<Snapshot> {
    compare_inner(request, true, cancel)
}

fn compare_inner(
    request: &CompareRequest,
    matching_only: bool,
    cancel: &AtomicBool,
) -> Result<Snapshot> {
    let left = canonical_root(&request.left)?;
    let right = canonical_root(&request.right)?;
    reject_overlap(&left, &right)?;
    let rules = Rules::parse(&request.exclusions)?;
    let l = scan(&left, &rules, request.content && !matching_only, cancel)?;
    let r = scan(&right, &rules, request.content && !matching_only, cancel)?;
    let all: BTreeSet<_> = l.keys().chain(r.keys()).cloned().collect();
    let mut rows = Vec::new();
    let mut paths = BTreeMap::new();
    let mut summary = Summary::default();
    let mut warnings = Vec::new();
    for (index, path) in all.into_iter().enumerate() {
        check_cancel(cancel)?;
        let mut a = l.get(&path).cloned();
        let mut b = r.get(&path).cloned();
        if matching_only
            && a.as_ref().zip(b.as_ref()).is_some_and(|(a, b)| {
                a.kind == Kind::File
                    && b.kind == Kind::File
                    && a.error.is_none()
                    && b.error.is_none()
                    && a.bytes == b.bytes
            })
        {
            for (root, entry) in [(&left, &mut a), (&right, &mut b)] {
                if let Some(entry) = entry {
                    match hash_file(&root.join(&path), cancel) {
                        Ok(digest) => entry.digest = Some(digest),
                        Err(Error::Cancelled) => return Err(Error::Cancelled),
                        Err(e) => entry.error = Some(e.to_string()),
                    }
                }
            }
        }
        let state = status(a.as_ref(), b.as_ref());
        let id = index.to_string();
        paths.insert(id.clone(), path.clone());
        // File summary excludes ordinary directories; subtree status is aggregated below.
        if !a
            .as_ref()
            .or(b.as_ref())
            .is_some_and(|e| e.kind == Kind::Directory)
            || state == Status::Error
            || state == Status::TypeConflict
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
        rows.push(Row {
            id,
            path: crate::tree::display_path(&path),
            directory_summary: None,
            status: state,
            left: a,
            right: b,
        });
    }
    if rows.iter().any(|r| {
        r.left
            .as_ref()
            .or(r.right.as_ref())
            .is_some_and(|e| e.kind == Kind::Link)
    }) {
        warnings
            .push("Symbolic links are listed but not followed, compared or synchronized.".into());
    }
    crate::tree::aggregate(&mut rows, &paths);
    let directory_index = rows
        .iter()
        .filter(|r| crate::tree::is_directory(r))
        .filter_map(|r| paths.get(&r.id).map(|p| (p.clone(), r.id.clone())))
        .collect();
    Ok(Snapshot {
        restart: None,
        directory_index,
        report: Comparison {
            policy: Some(clear_contracts::ComparisonPolicy {
                exclusions: request.exclusions.clone(),
                verification: if request.content {
                    clear_contracts::VerificationBasis::Sha256
                } else {
                    clear_contracts::VerificationBasis::SizeAndTime
                },
            }),
            session: unique_id(),
            left: left.to_string_lossy().into_owned(),
            right: right.to_string_lossy().into_owned(),
            summary,
            rows,
            warnings,
        },
        left,
        right,
        paths,
        rules,
    })
}

/// Reject symbolic-link ancestors every time a path is used for reading or writing.
pub fn checked_path(root: &Path, relative: &Path, allow_missing: bool) -> Result<PathBuf> {
    let mut path = root.to_owned();
    for part in relative.components() {
        let std::path::Component::Normal(name) = part else {
            return Err(Error::Invalid("Unsafe relative path".into()));
        };
        path.push(name);
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(Error::Invalid("Symbolic-link path is not allowed".into()));
            }
            Ok(_) => {}
            Err(e) if allow_missing && e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(path)
}
