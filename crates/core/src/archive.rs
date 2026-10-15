//! Read-only virtual trees for classic ZIP files and local directories.
use crate::{
    Error, Result,
    compare::{check_cancel, checked_path, hash_file, scan},
    rules::Rules,
    tree, unique_id,
};
use clear_contracts::{
    ArchiveRequest, Comparison, Entry, Kind, Page, Row, Status, Summary, TextDiff, TextOptions,
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
use zip::ZipArchive;
const MAX_ARCHIVE: u64 = 256 * 1024 * 1024;
const MAX_ENTRY: u64 = 128 * 1024 * 1024;
const MAX_EXPANDED: u64 = 1024 * 1024 * 1024;
const MAX_ENTRIES: usize = 50000;
fn zip_error(e: impl std::fmt::Display) -> Error {
    Error::Invalid(format!("ZIP: {e}"))
}
fn preflight(file: &mut File, cancel: &AtomicBool) -> Result<usize> {
    let len = file.metadata()?.len();
    if len > MAX_ARCHIVE {
        return Err(Error::Invalid("ZIP input exceeds 256 MiB".into()));
    }
    let tail_len = len.min(65557) as usize;
    file.seek(SeekFrom::End(-(tail_len as i64)))?;
    let mut tail = vec![0; tail_len];
    file.read_exact(&mut tail)?;
    let position = (0..tail_len.saturating_sub(21))
        .rev()
        .find(|i| {
            tail[*i..].starts_with(b"PK\x05\x06")
                && *i + 22 + usize::from(u16::from_le_bytes([tail[*i + 20], tail[*i + 21]]))
                    == tail_len
        })
        .ok_or_else(|| {
            Error::Invalid("ZIP end record is missing or trailing data is unsupported".into())
        })?;
    if position >= 20 && tail[position - 20..].starts_with(b"PK\x06\x07") {
        return Err(Error::Invalid(
            "ZIP64 is not supported by this adapter".into(),
        ));
    }
    let u16_at = |n| u16::from_le_bytes([tail[position + n], tail[position + n + 1]]);
    let u32_at = |n| u32::from_le_bytes(tail[position + n..position + n + 4].try_into().unwrap());
    let count = usize::from(u16_at(10));
    if u16_at(4) != 0
        || u16_at(6) != 0
        || usize::from(u16_at(8)) != count
        || count > MAX_ENTRIES
        || u32_at(12) > 16 * 1024 * 1024
        || u32_at(16) == u32::MAX
    {
        return Err(Error::Invalid(
            "ZIP is multi-disk, ZIP64 or exceeds directory limits".into(),
        ));
    }
    let end = len - tail_len as u64 + position as u64;
    let start = end
        .checked_sub(u64::from(u32_at(12)))
        .ok_or_else(|| Error::Invalid("Invalid ZIP directory bounds".into()))?;
    if start < u64::from(u32_at(16)) {
        return Err(Error::Invalid(
            "ZIP directory offset is inconsistent".into(),
        ));
    }
    file.seek(SeekFrom::Start(start))?;
    for _ in 0..count {
        check_cancel(cancel)?;
        let mut header = [0u8; 46];
        if file.stream_position()?.saturating_add(46) > end {
            return Err(Error::Invalid(
                "ZIP directory exceeds declared bounds".into(),
            ));
        }
        file.read_exact(&mut header)?;
        if !header.starts_with(b"PK\x01\x02") {
            return Err(Error::Invalid("Invalid ZIP directory entry".into()));
        }
        let lengths = [28usize, 30, 32]
            .iter()
            .map(|n| u64::from(u16::from_le_bytes([header[*n], header[*n + 1]])))
            .sum::<u64>();
        if file.stream_position()?.saturating_add(lengths) > end {
            return Err(Error::Invalid(
                "ZIP directory entry exceeds declared bounds".into(),
            ));
        }
        file.seek(SeekFrom::Current(lengths as i64))?;
    }
    if file.stream_position()? != end {
        return Err(Error::Invalid(
            "ZIP directory has unsupported trailing records".into(),
        ));
    }
    file.rewind()?;
    Ok(count)
}
fn member_path(name: &str) -> Result<PathBuf> {
    let name = name.strip_suffix('/').unwrap_or(name);
    if name.is_empty()
        || name.len() > 4096
        || name.starts_with('/')
        || name.contains(['\\', ':', '\0'])
        || name
            .split('/')
            .any(|p| p.is_empty() || matches!(p, "." | ".."))
    {
        return Err(Error::Invalid(
            "ZIP contains an unsafe or ambiguous member path".into(),
        ));
    }
    Ok(PathBuf::from(name))
}
fn directory() -> Entry {
    Entry {
        kind: Kind::Directory,
        bytes: "0".into(),
        modified_ms: None,
        digest: None,
        error: None,
    }
}
enum Source {
    Empty,
    Directory(PathBuf),
    Zip {
        path: PathBuf,
        digest: String,
        indices: BTreeMap<PathBuf, usize>,
    },
}
pub struct Snapshot {
    pub report: Comparison,
    left: Source,
    right: Source,
    paths: BTreeMap<String, PathBuf>,
}
fn zip_tree(
    path: &Path,
    rules: &Rules,
    content: bool,
    cancel: &AtomicBool,
) -> Result<(Source, BTreeMap<PathBuf, Entry>)> {
    let mut file = File::open(path)?;
    let count = preflight(&mut file, cancel)?;
    let digest = hash_file(path, cancel)?;
    let mut archive = ZipArchive::new(file).map_err(zip_error)?;
    if archive.len() != count {
        return Err(Error::Invalid(
            "ZIP has duplicate member names or inconsistent entry count".into(),
        ));
    }
    let mut entries = BTreeMap::<PathBuf, Entry>::new();
    let mut indices = BTreeMap::new();
    let mut actual_names = BTreeSet::new();
    let mut expanded = 0u64;
    for index in 0..archive.len() {
        check_cancel(cancel)?;
        let meta = archive.by_index_data(index).map_err(zip_error)?;
        let name = meta.name().map_err(zip_error)?;
        let relative = member_path(&name)?;
        if !actual_names.insert(relative.clone()) {
            return Err(Error::Invalid("ZIP has duplicate normalized names".into()));
        }
        let kind = if meta.is_symlink() {
            Kind::Link
        } else if meta.is_dir() {
            Kind::Directory
        } else if meta
            .unix_mode()
            .is_some_and(|m| m & 0o170000 != 0 && m & 0o170000 != 0o100000)
        {
            Kind::Other
        } else {
            Kind::File
        };
        if rules.excludes(&relative, kind == Kind::Directory) {
            continue;
        }
        let mut entry = Entry {
            kind: kind.clone(),
            bytes: meta.size().to_string(),
            modified_ms: None,
            digest: None,
            error: None,
        };
        let size = meta.size();
        let compressed = meta.compressed_size();
        let encrypted = meta.encrypted();
        if encrypted {
            entry.error = Some("Encrypted ZIP members are unsupported".into());
        } else if kind == Kind::Other {
            entry.error = Some("Special ZIP members are unsupported".into());
        } else if content && kind == Kind::File {
            expanded = expanded
                .checked_add(size)
                .ok_or_else(|| Error::Invalid("ZIP expanded size overflow".into()))?;
            if expanded > MAX_EXPANDED {
                return Err(Error::Invalid("ZIP expanded content exceeds 1 GiB".into()));
            }
            if size > MAX_ENTRY || (size > 1024 * 1024 && size / compressed.max(1) > 1000) {
                entry.error = Some("ZIP member exceeds 128 MiB or expansion-ratio limit".into());
            } else {
                let read = (|| -> Result<String> {
                    let mut member = archive.by_index(index).map_err(zip_error)?;
                    let mut hash = Sha256::new();
                    let mut total = 0u64;
                    let mut chunk = [0; 65536];
                    loop {
                        check_cancel(cancel)?;
                        let n = member.read(&mut chunk)?;
                        if n == 0 {
                            break;
                        }
                        total += n as u64;
                        if total > size || total > MAX_ENTRY {
                            return Err(Error::Invalid("ZIP member exceeds declared size".into()));
                        }
                        hash.update(&chunk[..n]);
                    }
                    if total != size {
                        return Err(Error::Invalid("ZIP member size mismatch".into()));
                    }
                    Ok(format!("{:x}", hash.finalize()))
                })();
                match read {
                    Ok(hash) => entry.digest = Some(hash),
                    Err(Error::Cancelled) => return Err(Error::Cancelled),
                    Err(e) => entry.error = Some(e.to_string()),
                }
            }
        }
        for parent in relative.ancestors().skip(1) {
            if parent.as_os_str().is_empty() {
                break;
            }
            match entries.get(parent) {
                Some(e) if e.kind != Kind::Directory => {
                    return Err(Error::Invalid(
                        "ZIP member has a non-directory ancestor".into(),
                    ));
                }
                Some(_) => {}
                None => {
                    entries.insert(parent.to_path_buf(), directory());
                }
            }
        }
        if entries
            .get(&relative)
            .is_some_and(|e| e.kind == Kind::Directory && kind != Kind::Directory)
        {
            return Err(Error::Invalid(
                "ZIP member conflicts with an implicit directory".into(),
            ));
        }
        indices.insert(relative.clone(), index);
        entries.insert(relative, entry);
        if entries.len() > MAX_ENTRIES {
            return Err(Error::Invalid(
                "ZIP virtual tree exceeds 50000 entries".into(),
            ));
        }
    }
    if hash_file(path, cancel)? != digest {
        return Err(Error::Invalid("ZIP changed during comparison".into()));
    }
    Ok((
        Source::Zip {
            path: path.to_path_buf(),
            digest,
            indices,
        },
        entries,
    ))
}
fn load(
    path: &str,
    rules: &Rules,
    content: bool,
    cancel: &AtomicBool,
) -> Result<(Source, BTreeMap<PathBuf, Entry>)> {
    let original = Path::new(path);
    let meta = fs::symlink_metadata(original)?;
    if meta.file_type().is_symlink() {
        return Err(Error::Invalid(
            "Archive sources must not be symbolic links".into(),
        ));
    }
    let path = fs::canonicalize(original)?;
    if meta.is_dir() {
        let entries = scan(&path, rules, content, cancel)?;
        Ok((Source::Directory(path), entries))
    } else if meta.is_file() {
        zip_tree(&path, rules, content, cancel)
    } else {
        Err(Error::Invalid("Choose a directory or ZIP file".into()))
    }
}
fn status(left: Option<&Entry>, right: Option<&Entry>) -> Status {
    if left.is_some_and(|e| e.error.is_some()) || right.is_some_and(|e| e.error.is_some()) {
        return Status::Error;
    }
    match (left, right) {
        (Some(_), None) => Status::LeftOnly,
        (None, Some(_)) => Status::RightOnly,
        (Some(a), Some(b)) if a.kind != b.kind => Status::TypeConflict,
        (Some(a), Some(_)) if a.kind == Kind::Directory => Status::Same,
        (Some(a), Some(b)) if a.kind == Kind::File && a.digest.is_some() && b.digest.is_some() => {
            if a.digest == b.digest {
                Status::Same
            } else {
                Status::Changed
            }
        }
        (Some(a), Some(b)) if a.bytes != b.bytes => Status::Changed,
        _ => Status::MetadataMatch,
    }
}
pub fn compare(request: &ArchiveRequest, cancel: &AtomicBool) -> Result<Snapshot> {
    let rules = Rules::parse(&request.exclusions)?;
    let (left, l) = load(&request.left, &rules, request.content, cancel)?;
    let (right, r) = if request.right.trim().is_empty() {
        (Source::Empty, BTreeMap::new())
    } else {
        load(&request.right, &rules, request.content, cancel)?
    };
    let all = l.keys().chain(r.keys()).cloned().collect::<BTreeSet<_>>();
    let mut rows = Vec::new();
    let mut paths = BTreeMap::new();
    let mut summary = Summary::default();
    for (index, path) in all.into_iter().enumerate() {
        check_cancel(cancel)?;
        let a = l.get(&path).cloned();
        let b = r.get(&path).cloned();
        let state = status(a.as_ref(), b.as_ref());
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
        let id = index.to_string();
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
    Ok(Snapshot {report:Comparison{session:unique_id(),left:request.left.clone(),right:request.right.clone(),summary,rows,warnings:vec!["Archive comparison is read-only. ZIP symlinks are listed without following them. Metadata matches are not verified content equality.".into()]},left,right,paths})
}
impl Snapshot {
    pub fn page(&self, offset: usize, search: &str, differences_only: bool) -> Result<Page> {
        if search.len() > 4096 {
            return Err(Error::Invalid("Search exceeds 4096 bytes".into()));
        }
        let query = search.to_lowercase();
        let matched = self
            .report
            .rows
            .iter()
            .filter(|r| {
                (!differences_only || r.status != Status::Same)
                    && (query.is_empty() || r.path.to_lowercase().contains(&query))
            })
            .collect::<Vec<_>>();
        Ok(Page {
            total: matched.len(),
            rows: matched
                .into_iter()
                .skip(offset)
                .take(500)
                .cloned()
                .collect(),
        })
    }
    fn read(
        &self,
        source: &Source,
        relative: &Path,
        entry: Option<&Entry>,
        cancel: &AtomicBool,
    ) -> Result<String> {
        let Some(entry) = entry else {
            return Ok(String::new());
        };
        if entry.kind != Kind::File || entry.error.is_some() {
            return Err(Error::Invalid(
                "Select a readable regular text member".into(),
            ));
        }
        let mut bytes = Vec::new();
        match source {
            Source::Empty => return Ok(String::new()),
            Source::Directory(root) => {
                File::open(checked_path(root, relative, false)?)?
                    .take((crate::editing::MAX_DOCUMENT_BYTES + 1) as u64)
                    .read_to_end(&mut bytes)?;
            }
            Source::Zip {
                path,
                digest,
                indices,
            } => {
                if fs::metadata(path)?.len() > MAX_ARCHIVE || hash_file(path, cancel)? != *digest {
                    return Err(Error::Invalid("ZIP changed. Compare again.".into()));
                }
                let index = *indices
                    .get(relative)
                    .ok_or_else(|| Error::Invalid("Missing ZIP member index".into()))?;
                let mut archive = ZipArchive::new(File::open(path)?).map_err(zip_error)?;
                {
                    let member = archive.by_index(index).map_err(zip_error)?;
                    if member_path(&member.name().map_err(zip_error)?)? != relative {
                        return Err(Error::Invalid("ZIP member identity changed".into()));
                    }
                    member
                        .take((crate::editing::MAX_DOCUMENT_BYTES + 1) as u64)
                        .read_to_end(&mut bytes)?;
                }
                if hash_file(path, cancel)? != *digest {
                    return Err(Error::Invalid("ZIP changed during preview".into()));
                }
            }
        }
        if bytes.len() > crate::editing::MAX_DOCUMENT_BYTES {
            return Err(Error::Invalid("Archive text preview exceeds 4 MiB".into()));
        }
        if entry
            .digest
            .as_ref()
            .is_some_and(|d| *d != format!("{:x}", Sha256::digest(&bytes)))
        {
            return Err(Error::Invalid("Member changed since comparison".into()));
        }
        Ok(crate::editing::decode(&bytes)?.0)
    }
    pub fn text_diff(
        &self,
        row_id: &str,
        options: &TextOptions,
        cancel: &AtomicBool,
    ) -> Result<TextDiff> {
        let relative = self
            .paths
            .get(row_id)
            .ok_or_else(|| Error::Invalid("Unknown archive entry".into()))?;
        let row = self
            .report
            .rows
            .iter()
            .find(|r| r.id == row_id)
            .ok_or_else(|| Error::Invalid("Unknown archive entry".into()))?;
        crate::text::compare_strings(
            &row.path,
            &self.read(&self.left, relative, row.left.as_ref(), cancel)?,
            &self.read(&self.right, relative, row.right.as_ref(), cancel)?,
            options,
        )
    }
}
