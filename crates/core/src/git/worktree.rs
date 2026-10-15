//! Physical tracked/optional-untracked working-tree reads. No Git filters or writes.
use super::{GitEntry, MAX_ENTRIES, MAX_LIST, path_from_bytes, strict_command};
#[cfg(any(windows, test))]
use crate::native_path::windows_file_name as windows_component;
use crate::{
    Error, Result,
    compare::{check_cancel, checked_path},
    rules::Rules,
};
use clear_contracts::{Entry, Kind};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, Metadata},
    io::Read,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
    time::{SystemTime, UNIX_EPOCH},
};
fn working_path(bytes: &[u8]) -> Result<PathBuf> {
    let path = path_from_bytes(bytes)?;
    if path
        .iter()
        .any(|c| c.to_str().is_some_and(|s| s.eq_ignore_ascii_case(".git")))
    {
        return Err(Error::Invalid(
            "Git administrative paths are not working-tree entries".into(),
        ));
    }
    #[cfg(windows)]
    if path
        .iter()
        .any(|c| !c.to_str().is_some_and(windows_component))
    {
        return Err(Error::Invalid(
            "Git working-tree path uses an unsupported Windows filename".into(),
        ));
    }
    Ok(path)
}

#[derive(Clone, PartialEq, Eq)]
struct Stamp {
    bytes: u64,
    modified: Option<SystemTime>,
    #[cfg(unix)]
    permissions: u32,
}
fn stamp(meta: &Metadata) -> Stamp {
    Stamp {
        bytes: meta.len(),
        modified: meta.modified().ok(),
        #[cfg(unix)]
        permissions: {
            use std::os::unix::fs::PermissionsExt;
            meta.permissions().mode()
        },
    }
}
#[derive(Clone)]
pub(super) struct FileState {
    relative: PathBuf,
    digest: String,
    stamp: Stamp,
}
#[derive(Clone, Copy)]
enum ObjectFormat {
    Sha1,
    Sha256,
}
enum BlobHash {
    Sha1(sha1::Sha1),
    Sha256(Sha256),
}
impl BlobHash {
    fn new(format: ObjectFormat, bytes: u64) -> Self {
        let header = format!("blob {bytes}\0");
        match format {
            ObjectFormat::Sha1 => {
                let mut h = sha1::Sha1::new();
                h.update(header.as_bytes());
                Self::Sha1(h)
            }
            ObjectFormat::Sha256 => {
                let mut h = Sha256::new();
                h.update(header.as_bytes());
                Self::Sha256(h)
            }
        }
    }
    fn update(&mut self, bytes: &[u8]) {
        match self {
            Self::Sha1(h) => h.update(bytes),
            Self::Sha256(h) => h.update(bytes),
        }
    }
    fn finish(self) -> String {
        match self {
            Self::Sha1(h) => format!("{:x}", h.finalize()),
            Self::Sha256(h) => format!("{:x}", h.finalize()),
        }
    }
}
fn config(root: &Path, key: &str, cancel: &AtomicBool) -> Result<bool> {
    let raw = strict_command(
        root,
        &["config", "--type=bool", "--default", "true", "--get", key],
        32,
        cancel,
    )?;
    match raw.as_slice() {
        b"true\n" | b"true\r\n" => Ok(true),
        b"false\n" | b"false\r\n" => Ok(false),
        _ => Err(Error::Invalid(format!(
            "Invalid Git Boolean configuration: {key}"
        ))),
    }
}
fn candidate(root: &Path, relative: &Path) -> Result<Option<PathBuf>> {
    let parent = relative
        .parent()
        .ok_or_else(|| Error::Invalid("Invalid working-tree relative path".into()))?;
    let parent = match checked_path(root, parent, false) {
        Ok(p) => p,
        Err(Error::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(None);
        }
        Err(e) => return Err(e),
    };
    if !fs::symlink_metadata(&parent)?.is_dir() {
        return Err(Error::Invalid(
            "A working-tree parent is not a directory".into(),
        ));
    }
    let path = parent.join(
        relative
            .file_name()
            .ok_or_else(|| Error::Invalid("Missing working-tree filename".into()))?,
    );
    match fs::symlink_metadata(&path) {
        Ok(_) => Ok(Some(path)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
fn hash_file(
    path: &Path,
    format: ObjectFormat,
    collect: bool,
    cancel: &AtomicBool,
) -> Result<(String, String, Stamp, Vec<u8>)> {
    check_cancel(cancel)?;
    let observed = fs::symlink_metadata(path)?;
    if !observed.is_file() || observed.file_type().is_symlink() {
        return Err(Error::Invalid(
            "Working-tree preview requires a regular file; links are not followed".into(),
        ));
    }
    let mut file = File::open(path)?;
    let before = stamp(&file.metadata()?);
    if before != stamp(&observed) {
        return Err(Error::Invalid(
            "Working-tree file changed before reading".into(),
        ));
    }
    if collect && before.bytes > crate::editing::MAX_DOCUMENT_BYTES as u64 {
        return Err(Error::Invalid(
            "Git working-tree text preview exceeds 4 MiB".into(),
        ));
    }
    let mut blob = BlobHash::new(format, before.bytes);
    let mut digest = Sha256::new();
    let mut bytes = Vec::new();
    let mut count = 0u64;
    let mut buffer = [0u8; 65536];
    let mut reader = (&mut file).take(before.bytes.saturating_add(1));
    loop {
        check_cancel(cancel)?;
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        count = count
            .checked_add(n as u64)
            .ok_or_else(|| Error::Invalid("Working-tree byte count overflow".into()))?;
        if count > before.bytes {
            return Err(Error::Invalid(
                "Working-tree file grew during reading".into(),
            ));
        }
        blob.update(&buffer[..n]);
        digest.update(&buffer[..n]);
        if collect {
            bytes.extend_from_slice(&buffer[..n]);
        }
    }
    if count != before.bytes || stamp(&file.metadata()?) != before {
        return Err(Error::Invalid(
            "Working-tree file changed during reading".into(),
        ));
    }
    check_cancel(cancel)?;
    Ok((
        blob.finish(),
        format!("{:x}", digest.finalize()),
        before,
        bytes,
    ))
}
fn file_mode(meta: &Metadata, index_mode: &str, trust_exec: bool) -> String {
    if !trust_exec {
        return if index_mode == "100755" {
            "100755"
        } else {
            "100644"
        }
        .into();
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if meta.permissions().mode() & 0o111 != 0 {
            "100755"
        } else {
            "100644"
        }
        .into()
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        if index_mode == "100755" {
            "100755"
        } else {
            "100644"
        }
        .into()
    }
}
fn error_entry(error: Error) -> GitEntry {
    GitEntry {
        working: None,
        oid: String::new(),
        mode: String::new(),
        entry: Entry {
            kind: Kind::Other,
            bytes: "0".into(),
            modified_ms: None,
            digest: None,
            error: Some(format!("Cannot read working-tree entry: {error}")),
        },
    }
}
fn entry(
    root: &Path,
    relative: &Path,
    index_mode: &str,
    format: ObjectFormat,
    trust_exec: bool,
    trust_links: bool,
    cancel: &AtomicBool,
) -> Result<Option<GitEntry>> {
    check_cancel(cancel)?;
    let Some(path) = candidate(root, relative)? else {
        return Ok(None);
    };
    let meta = fs::symlink_metadata(&path)?;
    let kind = if index_mode == "160000" {
        Kind::Other
    } else if meta.file_type().is_symlink() || !trust_links && index_mode == "120000" {
        Kind::Link
    } else if meta.is_dir() {
        Kind::Directory
    } else if meta.is_file() {
        Kind::File
    } else {
        Kind::Other
    };
    let modified = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|t| t.as_millis().to_string());
    let mut result = GitEntry {
        working: None,
        oid: String::new(),
        mode: match kind {
            Kind::Directory => "040000",
            Kind::Link => "120000",
            _ => "",
        }
        .into(),
        entry: Entry {
            kind: kind.clone(),
            bytes: if kind == Kind::Directory {
                "0".into()
            } else {
                meta.len().to_string()
            },
            modified_ms: modified,
            digest: None,
            error: None,
        },
    };
    if kind == Kind::Other {
        result.entry.error = Some(
            if index_mode == "160000" {
                "Git submodule references are not expanded"
            } else {
                "Working-tree entry is not a regular file, directory or symbolic link"
            }
            .into(),
        );
    }
    if kind == Kind::Directory && index_mode.is_empty() {
        result.entry.error=Some("A listed file is now a directory or an embedded repository; children are not implied by this entry".into());
    }
    if kind == Kind::File {
        let (oid, digest, stamp, _) = hash_file(&path, format, false, cancel)?;
        if stamp != self::stamp(&meta) {
            return Err(Error::Invalid(
                "Working-tree metadata changed during capture".into(),
            ));
        }
        result.mode = file_mode(&meta, index_mode, trust_exec);
        result.oid = oid;
        result.entry.bytes = stamp.bytes.to_string();
        result.entry.digest = Some(digest.clone());
        result.working = Some(FileState {
            relative: relative.to_owned(),
            digest,
            stamp,
        });
    }
    Ok(Some(result))
}
pub(super) fn load(
    root: &Path,
    rules: &Rules,
    include_untracked: bool,
    cancel: &AtomicBool,
) -> Result<(String, BTreeMap<PathBuf, GitEntry>)> {
    if fs::canonicalize(root)? != root || fs::symlink_metadata(root)?.file_type().is_symlink() {
        return Err(Error::Invalid("Working-tree root changed".into()));
    }
    let format = match strict_command(
        root,
        &["rev-parse", "--show-object-format=storage"],
        64,
        cancel,
    )?
    .as_slice()
    {
        b"sha1\n" | b"sha1\r\n" => ObjectFormat::Sha1,
        b"sha256\n" | b"sha256\r\n" => ObjectFormat::Sha256,
        _ => return Err(Error::Invalid("Unsupported Git object format".into())),
    };
    let trust_exec = config(root, "core.filemode", cancel)?;
    let trust_links = config(root, "core.symlinks", cancel)?;
    let tracked = strict_command(
        root,
        &[
            "ls-files",
            "--cached",
            "--full-name",
            "-z",
            "--format=%(objectmode) %(stage)%x09%(path)",
            "--",
        ],
        MAX_LIST,
        cancel,
    )?;
    if !tracked.is_empty() && tracked.last() != Some(&0) {
        return Err(Error::Invalid(
            "Truncated tracked working-tree listing".into(),
        ));
    }
    let mut paths = BTreeMap::<PathBuf, (u8, String)>::new();
    for (count, record) in tracked
        .split(|b| *b == 0)
        .filter(|r| !r.is_empty())
        .enumerate()
    {
        check_cancel(cancel)?;
        if count >= MAX_ENTRIES * 3 {
            return Err(Error::Invalid(
                "Working-tree index scope exceeds 600000 stage records".into(),
            ));
        }
        let tab = record
            .iter()
            .position(|b| *b == b'\t')
            .ok_or_else(|| Error::Invalid("Malformed tracked path record".into()))?;
        let header = std::str::from_utf8(&record[..tab])
            .map_err(|_| Error::Invalid("Invalid tracked path metadata".into()))?
            .split_ascii_whitespace()
            .collect::<Vec<_>>();
        if header.len() != 2 || !matches!(header[0], "100644" | "100755" | "120000" | "160000") {
            return Err(Error::Invalid(
                "Unsupported/unexpanded working-tree index mode".into(),
            ));
        }
        let stage = header[1]
            .parse::<u8>()
            .map_err(|_| Error::Invalid("Invalid working-tree index stage".into()))?;
        if stage > 3 {
            return Err(Error::Invalid("Invalid working-tree index stage".into()));
        }
        let path = working_path(&record[tab + 1..])?;
        if rules.excludes(&path, false) {
            continue;
        }
        let priority = match stage {
            0 => 0,
            2 => 1,
            1 => 2,
            _ => 3,
        };
        match paths.get_mut(&path) {
            Some((p, mode)) if priority < *p => {
                *p = priority;
                *mode = header[0].into();
            }
            None => {
                paths.insert(path, (priority, header[0].into()));
            }
            _ => {}
        }
        if paths.len() > MAX_ENTRIES {
            return Err(Error::Invalid(
                "Working-tree scope exceeds 200000 paths".into(),
            ));
        }
    }
    if include_untracked {
        let other = strict_command(
            root,
            &[
                "ls-files",
                "--others",
                "--exclude-standard",
                "--full-name",
                "-z",
                "--",
            ],
            MAX_LIST,
            cancel,
        )?;
        if !other.is_empty() && other.last() != Some(&0) {
            return Err(Error::Invalid("Truncated untracked path listing".into()));
        }
        for record in other.split(|b| *b == 0).filter(|r| !r.is_empty()) {
            check_cancel(cancel)?;
            let path = working_path(record.strip_suffix(b"/").unwrap_or(record))?;
            if rules.excludes(&path, false) {
                continue;
            }
            paths.entry(path).or_insert((0, String::new()));
            if paths.len() > MAX_ENTRIES {
                return Err(Error::Invalid(
                    "Working-tree scope exceeds 200000 paths".into(),
                ));
            }
        }
    }
    let mut entries = BTreeMap::new();
    for (path, (_, mode)) in paths {
        match entry(root, &path, &mode, format, trust_exec, trust_links, cancel) {
            Ok(Some(e)) => {
                if !rules.excludes(&path, e.entry.kind == Kind::Directory) {
                    entries.insert(path, e);
                }
            }
            Ok(None) => {}
            Err(Error::Cancelled) => return Err(Error::Cancelled),
            Err(e) => {
                entries.insert(path, error_entry(e));
            }
        }
    }
    let files = entries.keys().cloned().collect::<Vec<_>>();
    for path in files {
        for parent in path
            .ancestors()
            .skip(1)
            .filter(|p| !p.as_os_str().is_empty())
        {
            check_cancel(cancel)?;
            if !entries.contains_key(parent) {
                let actual = match entry(
                    root,
                    parent,
                    "040000",
                    format,
                    trust_exec,
                    trust_links,
                    cancel,
                ) {
                    Ok(Some(e)) => e,
                    Ok(None) => error_entry(Error::Invalid(
                        "Working-tree parent disappeared during capture".into(),
                    )),
                    Err(Error::Cancelled) => return Err(Error::Cancelled),
                    Err(e) => error_entry(e),
                };
                entries.insert(parent.to_owned(), actual);
            }
            if entries.len() > MAX_ENTRIES {
                return Err(Error::Invalid(
                    "Working-tree entries with directories exceed 200000".into(),
                ));
            }
        }
    }
    if fs::canonicalize(root)? != root {
        return Err(Error::Invalid(
            "Working-tree root changed during capture".into(),
        ));
    }
    Ok((
        if include_untracked {
            "Working tree (tracked + nonignored untracked)"
        } else {
            "Working tree (tracked paths)"
        }
        .into(),
        entries,
    ))
}
pub(super) fn preview(
    root: &Path,
    state: &FileState,
    entry: &GitEntry,
    cancel: &AtomicBool,
) -> Result<Vec<u8>> {
    if fs::canonicalize(root)? != root {
        return Err(Error::Invalid("Working-tree root changed".into()));
    }
    let path = checked_path(root, &state.relative, false)?;
    if stamp(&fs::symlink_metadata(&path)?) != state.stamp {
        return Err(Error::Invalid(
            "Working-tree source changed. Compare Git sources again.".into(),
        ));
    }
    let format = match entry.oid.len() {
        40 => ObjectFormat::Sha1,
        64 => ObjectFormat::Sha256,
        _ => {
            return Err(Error::Invalid(
                "Invalid working-tree object identity".into(),
            ));
        }
    };
    let (oid, digest, stamp, bytes) = hash_file(&path, format, true, cancel)?;
    if digest != state.digest || oid != entry.oid || stamp != state.stamp {
        return Err(Error::Invalid(
            "Working-tree content changed. Compare Git sources again.".into(),
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn working_names_cannot_address_administrative_storage_or_win32_devices() {
        for path in [b".git/config".as_slice(), b"dir/.GIT/index"] {
            assert!(working_path(path).is_err());
        }
        assert!(working_path(b".gitignore").is_ok());
        for name in [
            "NUL",
            "nul.txt",
            "COM1.log",
            "LPT9",
            "COM\u{b9}",
            "LPT\u{b2}.txt",
            "file.",
            "file ",
            "CON .log",
            "a:b",
            "a?b",
            "bad\nname",
        ] {
            assert!(!windows_component(name), "{name}");
        }
        for name in [
            "report.csv",
            ".gitignore",
            "COM10",
            "NULL",
            "name with spaces.txt",
        ] {
            assert!(windows_component(name), "{name}");
        }
    }
}
