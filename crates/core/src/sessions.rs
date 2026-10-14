//! Versioned local session library with atomic writes and revision checks.
use crate::{Error, Result, rules::Rules, unique_id};
use clear_contracts::{LibraryEntry, SavedSession};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
};
const MAX_BYTES: u64 = 65536;
const MAX_SESSIONS: usize = 200;
#[derive(Serialize, Deserialize)]
struct Stored {
    version: u32,
    id: String,
    name: String,
    updated_ms: String,
    session: SavedSession,
}
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 128 && id.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
}
fn root(base: &Path, create: bool) -> Result<PathBuf> {
    let path = base.join("sessions");
    if create && !path.exists() {
        fs::create_dir(&path)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
        }
    }
    if path.exists() {
        let m = fs::symlink_metadata(&path)?;
        if !m.is_dir() || m.file_type().is_symlink() {
            return Err(Error::Invalid(
                "Session library must be a real directory".into(),
            ));
        }
    }
    Ok(path)
}
fn lock(base: &Path) -> Result<File> {
    let path = root(base, true)?.join(".lock");
    if let Ok(metadata) = fs::symlink_metadata(&path)
        && (!metadata.is_file() || metadata.file_type().is_symlink())
    {
        return Err(Error::Invalid("Invalid session lock file".into()));
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)?;
    file.try_lock()
        .map_err(|e| Error::Invalid(format!("Session storage is busy: {e}")))?;
    Ok(file)
}

fn read_bytes(path: &Path) -> Result<Vec<u8>> {
    let m = fs::symlink_metadata(path)?;
    if !m.is_file() || m.file_type().is_symlink() {
        return Err(Error::Invalid("Session data must be a regular file".into()));
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(Error::Invalid("Session data exceeds 64 KiB".into()));
    }
    Ok(bytes)
}
fn validate(session: &SavedSession) -> Result<()> {
    for path in [&session.request.left, &session.request.right] {
        if path.trim().is_empty() || path.len() > 32768 || path.contains('\0') {
            return Err(Error::Invalid("Choose two valid session paths".into()));
        }
    }
    if session.view.search.len() > 4096 {
        return Err(Error::Invalid("Search exceeds 4096 bytes".into()));
    }
    Rules::parse(&session.request.exclusions)?;
    Ok(())
}
fn entry(path: &Path) -> Result<LibraryEntry> {
    let bytes = read_bytes(path)?;
    let stored: Stored = serde_json::from_slice(&bytes)?;
    if stored.version != 1
        || !valid_id(&stored.id)
        || path.file_stem().and_then(|s| s.to_str()) != Some(&stored.id)
    {
        return Err(Error::Invalid("Invalid session envelope".into()));
    }
    validate(&stored.session)?;
    if stored.name.trim().is_empty()
        || stored.name.len() > 256
        || stored.name.chars().any(char::is_control)
    {
        return Err(Error::Invalid("Invalid stored session name".into()));
    }
    Ok(LibraryEntry {
        id: stored.id,
        name: stored.name,
        updated_ms: stored.updated_ms,
        session: stored.session,
        revision: format!("{:x}", Sha256::digest(bytes)),
    })
}
pub fn load(base: &Path, id: &str) -> Result<LibraryEntry> {
    if !valid_id(id) {
        return Err(Error::Invalid("Invalid session ID".into()));
    }
    entry(&root(base, false)?.join(format!("{id}.json")))
}

pub fn list(base: &Path) -> Result<Vec<LibraryEntry>> {
    let root = root(base, false)?;
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut entries = Vec::new();
    for item in fs::read_dir(root)? {
        let item = item?;
        if item.path().extension() != Some(std::ffi::OsStr::new("json")) {
            continue;
        }
        let Some(id) = item
            .path()
            .file_stem()
            .and_then(|s| s.to_str())
            .map(str::to_owned)
        else {
            continue;
        };
        if !valid_id(&id) {
            continue;
        }
        entries.push(entry(&item.path())?);
        if entries.len() > MAX_SESSIONS {
            return Err(Error::Invalid("Session library exceeds 200 entries".into()));
        }
    }
    entries.sort_by(|a, b| {
        b.updated_ms
            .parse::<u128>()
            .unwrap_or_default()
            .cmp(&a.updated_ms.parse::<u128>().unwrap_or_default())
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(entries)
}
pub fn save(
    base: &Path,
    id: Option<&str>,
    revision: Option<&str>,
    name: &str,
    session: &SavedSession,
) -> Result<LibraryEntry> {
    validate(session)?;
    let _lock = lock(base)?;
    let name = name.trim();
    if name.is_empty() || name.len() > 256 || name.chars().any(char::is_control) {
        return Err(Error::Invalid(
            "Session name must be 1–256 bytes without control characters".into(),
        ));
    }
    let root = root(base, true)?;
    let id = if let Some(id) = id {
        if !valid_id(id) {
            return Err(Error::Invalid("Invalid session ID".into()));
        }
        id.to_owned()
    } else {
        if list(base)?.len() >= MAX_SESSIONS {
            return Err(Error::Invalid(
                "Archive a session before adding more".into(),
            ));
        }
        unique_id()
    };
    let path = root.join(format!("{id}.json"));
    if path.exists() {
        let current = entry(&path)?;
        if revision != Some(current.revision.as_str()) {
            return Err(Error::Invalid(
                "Session changed externally. Reload the library.".into(),
            ));
        }
    } else if revision.is_some() {
        return Err(Error::Invalid("Session no longer exists".into()));
    }
    let value = Stored {
        version: 1,
        id: id.clone(),
        name: name.into(),
        updated_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| Error::Invalid("System clock is before epoch".into()))?
            .as_millis()
            .to_string(),
        session: session.clone(),
    };
    let bytes = serde_json::to_vec_pretty(&value)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(Error::Invalid("Session data exceeds 64 KiB".into()));
    }
    crate::sync::atomic_json(&path, &value)?;
    entry(&path)
}
pub fn archive(base: &Path, id: &str, revision: &str) -> Result<()> {
    let _lock = lock(base)?;
    if !valid_id(id) {
        return Err(Error::Invalid("Invalid session ID".into()));
    }
    let root = root(base, false)?;
    let path = root.join(format!("{id}.json"));
    let current = entry(&path)?;
    if current.revision != revision {
        return Err(Error::Invalid(
            "Session changed externally. Reload the library.".into(),
        ));
    }
    let trash = root.join("trash");
    if !trash.exists() {
        fs::create_dir(&trash)?;
    }
    let m = fs::symlink_metadata(&trash)?;
    if !m.is_dir() || m.file_type().is_symlink() {
        return Err(Error::Invalid("Invalid session archive directory".into()));
    }
    fs::rename(path, trash.join(format!("{id}-{}.json", unique_id())))?;
    Ok(())
}
pub fn load_last(base: &Path) -> Result<Option<SavedSession>> {
    let path = base.join("session.json");
    match fs::symlink_metadata(&path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
        Ok(_) => {}
    }
    let session = serde_json::from_slice(&read_bytes(&path)?)?;
    validate(&session)?;
    Ok(Some(session))
}
pub fn save_last(base: &Path, session: &SavedSession) -> Result<()> {
    let _lock = lock(base)?;
    validate(session)?;
    let bytes = serde_json::to_vec(session)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(Error::Invalid("Session data exceeds 64 KiB".into()));
    }
    let path = base.join("session.json");
    if path.exists() {
        read_bytes(&path)?;
    }
    crate::sync::atomic_json(&path, session)
}
