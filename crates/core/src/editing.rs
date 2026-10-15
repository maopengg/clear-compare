//! Bounded text editing with encoding retention, optimistic concurrency and backups.
use crate::{
    Error, Result,
    compare::{Snapshot, checked_path, hash_file, reject_overlap},
    unique_id,
};
use clear_contracts::{
    Encoding, Kind, LineEnding, RestorePreview, SaveOptions, SaveResult, Side, TextDocument,
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
use tempfile::NamedTempFile;

pub const MAX_DOCUMENT_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone)]
pub struct OpenDocument {
    pub view: TextDocument,
    root: PathBuf,
    relative: PathBuf,
    restore_alias: Option<PathBuf>,
}

#[derive(Clone)]
pub struct Relocation {
    pub root: PathBuf,
    pub from: PathBuf,
    pub to: PathBuf,
}
impl OpenDocument {
    pub fn has_path(&self, path: &Path) -> bool {
        self.root.join(&self.relative) == path
    }
    pub fn relocated(&self, change: &Relocation) -> Result<Self> {
        if !self.has_path(&change.root.join(&change.from)) {
            return Err(Error::Invalid(
                "Editor relocation does not match its native path".into(),
            ));
        }
        let path = checked_path(&change.root, &change.to, false)?;
        let cancel = AtomicBool::new(false);
        if !crate::native_path::exact_leaf(&change.root, &change.to, &cancel)?
            || hash_file(&path, &cancel)? != self.view.digest
        {
            return Err(Error::Invalid("Renamed editor source changed".into()));
        }
        let mut updated = self.clone();
        if updated.restore_alias.is_none() {
            updated.restore_alias = Some(self.root.join(&self.relative));
        }
        updated.root = change.root.clone();
        updated.relative = change.to.clone();
        updated.view.path = crate::tree::display_path(&change.to);
        updated.view.absolute_path = path.to_string_lossy().into();
        Ok(updated)
    }
    pub fn location_valid(&self) -> bool {
        let cancel = AtomicBool::new(false);
        crate::native_path::exact_leaf(&self.root, &self.relative, &cancel).unwrap_or(false)
            && checked_path(&self.root, &self.relative, false)
                .and_then(|p| hash_file(&p, &cancel))
                .is_ok_and(|d| d == self.view.digest)
    }
}

pub fn decode(bytes: &[u8]) -> Result<(String, Encoding, bool)> {
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        let little = bytes[0] == 0xff;
        if !bytes.len().is_multiple_of(2) {
            return Err(Error::Invalid("Malformed UTF-16 file".into()));
        }
        let units = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| {
                if little {
                    u16::from_le_bytes([p[0], p[1]])
                } else {
                    u16::from_be_bytes([p[0], p[1]])
                }
            })
            .collect::<Vec<_>>();
        let text = String::from_utf16(&units)
            .map_err(|_| Error::Invalid("Malformed UTF-16 file".into()))?;
        if text.contains('\0') {
            return Err(Error::Invalid("Binary text is not editable".into()));
        }
        return Ok((
            text,
            if little {
                Encoding::Utf16Le
            } else {
                Encoding::Utf16Be
            },
            true,
        ));
    }
    let bom = bytes.starts_with(&[0xef, 0xbb, 0xbf]);
    let body = if bom { &bytes[3..] } else { bytes };
    if body.contains(&0) {
        return Err(Error::Invalid("Binary or unsupported text encoding".into()));
    }
    Ok((
        String::from_utf8(body.to_vec())
            .map_err(|_| Error::Invalid("Use UTF-8 or BOM-marked UTF-16 for editing".into()))?,
        Encoding::Utf8,
        bom,
    ))
}

pub fn endings(text: &str) -> LineEnding {
    let crlf = text.matches("\r\n").count();
    let lf = text.matches('\n').count() - crlf;
    let cr = text.matches('\r').count() - crlf;
    if usize::from(crlf > 0) + usize::from(lf > 0) + usize::from(cr > 0) > 1 {
        LineEnding::Mixed
    } else if crlf > 0 {
        LineEnding::CrLf
    } else if cr > 0 {
        LineEnding::Cr
    } else {
        LineEnding::Lf
    }
}

pub(crate) fn encode(text: &str, original: &TextDocument) -> Result<Vec<u8>> {
    if text.len() > clear_contracts::edit_history::MAX_TEXT_BYTES {
        return Err(Error::Invalid("Decoded editor text exceeds 8 MiB".into()));
    }
    if text.contains('\0') {
        return Err(Error::Invalid("Text contains a NUL character".into()));
    }
    let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
    let text =
        match original.line_ending {
            LineEnding::CrLf => normalized.replace('\n', "\r\n"),
            LineEnding::Cr => normalized.replace('\n', "\r"),
            LineEnding::Lf => normalized,
            LineEnding::Mixed => return Err(Error::Invalid(
                "Mixed line endings require an explicit normalization policy; saving is disabled"
                    .into(),
            )),
        };
    let mut bytes = Vec::new();
    match original.encoding {
        Encoding::Utf8 => {
            if original.bom {
                bytes.extend_from_slice(&[0xef, 0xbb, 0xbf]);
            }
            bytes.extend_from_slice(text.as_bytes());
        }
        Encoding::Utf16Le | Encoding::Utf16Be => {
            let little = original.encoding == Encoding::Utf16Le;
            bytes.extend_from_slice(if little { &[0xff, 0xfe] } else { &[0xfe, 0xff] });
            for unit in text.encode_utf16() {
                bytes.extend_from_slice(&if little {
                    unit.to_le_bytes()
                } else {
                    unit.to_be_bytes()
                });
            }
        }
    }
    if bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(Error::Invalid("Edited document exceeds 4 MiB".into()));
    }
    Ok(bytes)
}

pub fn open(snapshot: &Snapshot, row_id: &str, side: Side) -> Result<OpenDocument> {
    let relative = snapshot
        .paths
        .get(row_id)
        .ok_or_else(|| Error::Invalid("Unknown entry".into()))?
        .clone();
    let row = snapshot
        .report
        .rows
        .iter()
        .find(|r| r.id == row_id)
        .ok_or_else(|| Error::Invalid("Unknown entry".into()))?;
    let (root, entry) = if side == Side::Left {
        (&snapshot.left, &row.left)
    } else {
        (&snapshot.right, &row.right)
    };
    if !entry
        .as_ref()
        .is_some_and(|e| e.kind == Kind::File && e.error.is_none())
    {
        return Err(Error::Invalid(
            "Select an existing regular file on this side".into(),
        ));
    }
    let path = checked_path(root, &relative, false)?;
    let mut bytes = Vec::new();
    File::open(&path)?
        .take((MAX_DOCUMENT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(Error::Invalid("Text editing is limited to 4 MiB".into()));
    }
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let (text, encoding, bom) = decode(&bytes)?;
    let view = TextDocument {
        id: unique_id(),
        absolute_path: path.to_string_lossy().into_owned(),
        path: row.path.clone(),
        side,
        line_ending: endings(&text),
        text,
        encoding,
        bom,
        digest,
    };
    Ok(OpenDocument {
        restore_alias: None,
        view,
        root: root.clone(),
        relative,
    })
}

/// Explicit CLI file entry point; the desktop uses opaque comparison entry IDs.
pub fn open_file(path: &Path) -> Result<OpenDocument> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let root = fs::canonicalize(
        absolute
            .parent()
            .ok_or_else(|| Error::Invalid("Invalid file path".into()))?,
    )?;
    let relative = PathBuf::from(
        absolute
            .file_name()
            .ok_or_else(|| Error::Invalid("Invalid file name".into()))?,
    );
    let target = checked_path(&root, &relative, false)?;
    let cancel = AtomicBool::new(false);
    let baseline = hash_file(&target, &cancel)?;
    let mut bytes = Vec::new();
    File::open(&target)?
        .take((MAX_DOCUMENT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(Error::Invalid("Text editing is limited to 4 MiB".into()));
    }
    let digest = format!("{:x}", Sha256::digest(&bytes));
    if digest != baseline {
        return Err(Error::Invalid("File changed while opening".into()));
    }
    let (text, encoding, bom) = decode(&bytes)?;
    let view = TextDocument {
        id: unique_id(),
        absolute_path: target.to_string_lossy().into(),
        path: relative.to_string_lossy().into(),
        side: Side::Left,
        line_ending: endings(&text),
        text,
        encoding,
        bom,
        digest,
    };
    Ok(OpenDocument {
        restore_alias: None,
        view,
        root,
        relative,
    })
}

pub fn save(
    original: &OpenDocument,
    text: &str,
    backup_base: &Path,
) -> Result<(OpenDocument, SaveResult)> {
    save_with_options(original, text, backup_base, &SaveOptions::default())
}

pub fn save_merge_aware(
    original: &OpenDocument,
    text: &str,
    backup_base: &Path,
    options: &SaveOptions,
    tokens: &[String],
) -> Result<(OpenDocument, SaveResult)> {
    if clear_contracts::edit_history::has_any_conflict(text, tokens)
        || (!tokens.is_empty()
            && clear_contracts::edit_history::generic_markers(text)
                > clear_contracts::edit_history::generic_markers(&original.view.text))
    {
        return Err(Error::Invalid(
            "Resolve all generated merge conflict markers before saving".into(),
        ));
    }
    save_with_options(original, text, backup_base, options)
}

pub fn save_with_options(
    original: &OpenDocument,
    text: &str,
    backup_base: &Path,
    options: &SaveOptions,
) -> Result<(OpenDocument, SaveResult)> {
    let mut format = original.view.clone();
    if let Some(encoding) = &options.encoding {
        format.encoding = encoding.clone();
    }
    if let Some(ending) = &options.line_ending {
        if *ending == LineEnding::Mixed {
            return Err(Error::Invalid(
                "Choose LF, CRLF or CR to normalize line endings".into(),
            ));
        }
        format.line_ending = ending.clone();
    }
    if let Some(bom) = options.bom {
        format.bom = bom;
    }
    if format.encoding != Encoding::Utf8 {
        if options.bom == Some(false) {
            return Err(Error::Invalid("UTF-16 requires a BOM".into()));
        }
        format.bom = true;
    }
    let bytes = encode(text, &format)?;
    save_bytes(original, &bytes, backup_base)
}

fn save_bytes(
    original: &OpenDocument,
    bytes: &[u8],
    backup_base: &Path,
) -> Result<(OpenDocument, SaveResult)> {
    if bytes.len() > MAX_DOCUMENT_BYTES {
        return Err(Error::Invalid("Document exceeds 4 MiB".into()));
    }
    decode(bytes)?;
    if !backup_base.is_dir() {
        return Err(Error::Invalid("Backup base must exist".into()));
    }
    let base = fs::canonicalize(backup_base)?;
    reject_overlap(&base, &original.root)?;
    let path = checked_path(&original.root, &original.relative, false)?;
    let cancel = AtomicBool::new(false);
    if !crate::native_path::exact_leaf(&original.root, &original.relative, &cancel)? {
        return Err(Error::Invalid(
            "Editor filename changed; reopen or relocate it before saving".into(),
        ));
    }
    if hash_file(&path, &cancel)? != original.view.digest {
        return Err(Error::Invalid(
            "File changed outside the editor. Reopen it before saving.".into(),
        ));
    }
    let permissions = fs::metadata(&path)?.permissions();
    if permissions.readonly() {
        return Err(Error::Invalid("File is read-only".into()));
    }
    let parent = path
        .parent()
        .ok_or_else(|| Error::Invalid("Invalid file parent".into()))?;
    let mut temporary = NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary
        .as_file()
        .set_permissions(permissions)
        .map_err(|e| std::io::Error::new(e.kind(), format!("Set edited-file permissions: {e}")))?;
    temporary.as_file().sync_all()?;
    let backup_dir = base.join(unique_id());
    fs::create_dir(&backup_dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&backup_dir, fs::Permissions::from_mode(0o700))?;
    }
    let backup = backup_dir.join("original.bin");
    let mut writer = File::create_new(&backup)?;
    let mut reader = File::open(&path)?;
    std::io::copy(&mut reader, &mut writer)?;
    drop(reader);
    writer.sync_all()?;
    drop(writer);
    if hash_file(&backup, &cancel)? != original.view.digest {
        return Err(Error::Invalid("Editor backup verification failed".into()));
    }
    checked_path(&original.root, &original.relative, false)?;
    if hash_file(&path, &cancel)? != original.view.digest {
        return Err(Error::Invalid("File changed while saving".into()));
    }
    let digest = format!("{:x}", Sha256::digest(bytes));
    let manifest = backup_dir.join("manifest.json");
    let write_manifest = |state: &str| -> Result<()> {
        let value = serde_json::json!({"version":1,"state":state,"target":path,
            "original_digest":original.view.digest,"saved_digest":digest,
            "backup":"original.bin","encoding":original.view.encoding,
            "bom":original.view.bom,"line_ending":original.view.line_ending});
        let mut metadata = NamedTempFile::new_in(&backup_dir)?;
        metadata.write_all(&serde_json::to_vec_pretty(&value)?)?;
        metadata.as_file().sync_all()?;
        metadata.persist(&manifest).map_err(|e| {
            std::io::Error::new(
                e.error.kind(),
                format!("Persist editor manifest: {}", e.error),
            )
        })?;
        Ok(())
    };
    write_manifest("prepared")?;
    if !crate::native_path::exact_leaf(&original.root, &original.relative, &cancel)? {
        return Err(Error::Invalid("Editor filename changed during save".into()));
    }
    temporary.persist(&path).map_err(|e| {
        std::io::Error::new(
            e.error.kind(),
            format!("Replace edited document: {}", e.error),
        )
    })?;
    if hash_file(&path, &cancel)? != digest {
        return Err(Error::Invalid("Saved document failed verification".into()));
    }
    write_manifest("completed")?;
    let (text, encoding, bom) = decode(bytes)?;
    let view = TextDocument {
        id: unique_id(),
        absolute_path: path.to_string_lossy().into_owned(),
        path: original.view.path.clone(),
        side: original.view.side.clone(),
        line_ending: endings(&text),
        text,
        encoding,
        bom,
        digest,
    };
    let result = SaveResult {
        document: view.clone(),
        backup: backup.to_string_lossy().into_owned(),
    };
    Ok((
        OpenDocument {
            restore_alias: None,
            view,
            root: original.root.clone(),
            relative: original.relative.clone(),
        },
        result,
    ))
}

/// A reviewed restore owns verified bytes and a pinned editor baseline.
pub struct RestorePlan {
    pub view: RestorePreview,
    document: OpenDocument,
    bytes: Vec<u8>,
}

pub fn preview_restore(original: &OpenDocument, backup: &Path) -> Result<RestorePlan> {
    let directory = backup
        .parent()
        .ok_or_else(|| Error::Invalid("Invalid backup path".into()))?;
    if backup.file_name() != Some(std::ffi::OsStr::new("original.bin")) {
        return Err(Error::Invalid(
            "Select an editor original.bin backup".into(),
        ));
    }
    let manifest = directory.join("manifest.json");
    let mut data = Vec::new();
    File::open(&manifest)?.take(65537).read_to_end(&mut data)?;
    if data.len() > 65536 {
        return Err(Error::Invalid("Backup manifest is too large".into()));
    }
    let metadata: serde_json::Value = serde_json::from_slice(&data)?;
    if metadata["version"] != 1
        || metadata["state"] != "completed"
        || (metadata["target"] != original.view.absolute_path
            && !metadata["target"].as_str().is_some_and(|p| {
                original
                    .restore_alias
                    .as_ref()
                    .is_some_and(|a| a == Path::new(p))
            }))
        || metadata["saved_digest"] != original.view.digest
    {
        return Err(Error::Invalid(
            "Backup does not match this editor's completed save".into(),
        ));
    }
    let expected = metadata["original_digest"]
        .as_str()
        .ok_or_else(|| Error::Invalid("Missing backup digest".into()))?;
    let cancel = AtomicBool::new(false);
    if hash_file(backup, &cancel)? != expected {
        return Err(Error::Invalid("Backup contents changed".into()));
    }
    let mut bytes = Vec::new();
    File::open(backup)?
        .take((MAX_DOCUMENT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_DOCUMENT_BYTES || format!("{:x}", Sha256::digest(&bytes)) != expected {
        return Err(Error::Invalid("Invalid editor backup".into()));
    }
    decode(&bytes)?;
    let target = checked_path(&original.root, &original.relative, false)?;
    if hash_file(&target, &cancel)? != original.view.digest {
        return Err(Error::Invalid(
            "Current file changed. Reopen before restoring.".into(),
        ));
    }
    Ok(RestorePlan {
        view: RestorePreview {
            id: unique_id(),
            target: original.view.absolute_path.clone(),
            backup: backup.to_string_lossy().into(),
            original_digest: expected.into(),
            current_digest: original.view.digest.clone(),
            bytes: bytes.len().to_string(),
        },
        document: original.clone(),
        bytes,
    })
}

pub fn apply_restore(plan: &RestorePlan, backup_base: &Path) -> Result<(OpenDocument, SaveResult)> {
    // Restore also backs up the current file, allowing another reviewed reversal.
    save_bytes(&plan.document, &plan.bytes, backup_base)
}
