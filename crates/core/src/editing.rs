//! Bounded text editing with encoding retention, optimistic concurrency and backups.
use crate::{
    Error, Result,
    compare::{Snapshot, checked_path, hash_file, reject_overlap},
    unique_id,
};
use clear_contracts::{Encoding, Kind, LineEnding, SaveResult, Side, TextDocument};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
use tempfile::NamedTempFile;

pub const MAX_DOCUMENT_BYTES: usize = 4 * 1024 * 1024;

pub struct OpenDocument {
    pub view: TextDocument,
    root: PathBuf,
    relative: PathBuf,
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

fn encode(text: &str, original: &TextDocument) -> Result<Vec<u8>> {
    if text.len() > MAX_DOCUMENT_BYTES {
        return Err(Error::Invalid("Edited document exceeds 4 MiB".into()));
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
        view,
        root: root.clone(),
        relative,
    })
}

pub fn save(
    original: &OpenDocument,
    text: &str,
    backup_base: &Path,
) -> Result<(OpenDocument, SaveResult)> {
    let bytes = encode(text, &original.view)?;
    if !backup_base.is_dir() {
        return Err(Error::Invalid("Backup base must exist".into()));
    }
    let base = fs::canonicalize(backup_base)?;
    reject_overlap(&base, &original.root)?;
    let path = checked_path(&original.root, &original.relative, false)?;
    let cancel = AtomicBool::new(false);
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
    temporary.write_all(&bytes)?;
    temporary.as_file().set_permissions(permissions)?;
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
    writer.sync_all()?;
    drop(writer);
    if hash_file(&backup, &cancel)? != original.view.digest {
        return Err(Error::Invalid("Editor backup verification failed".into()));
    }
    checked_path(&original.root, &original.relative, false)?;
    if hash_file(&path, &cancel)? != original.view.digest {
        return Err(Error::Invalid("File changed while saving".into()));
    }
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let manifest = backup_dir.join("manifest.json");
    let write_manifest = |state: &str| -> Result<()> {
        let value = serde_json::json!({"version":1,"state":state,"target":path,
            "original_digest":original.view.digest,"saved_digest":digest,
            "backup":"original.bin","encoding":original.view.encoding,
            "bom":original.view.bom,"line_ending":original.view.line_ending});
        let mut metadata = NamedTempFile::new_in(&backup_dir)?;
        metadata.write_all(&serde_json::to_vec_pretty(&value)?)?;
        metadata.as_file().sync_all()?;
        metadata.persist(&manifest).map_err(|e| e.error)?;
        Ok(())
    };
    write_manifest("prepared")?;
    temporary.persist(&path).map_err(|e| e.error)?;
    if hash_file(&path, &cancel)? != digest {
        return Err(Error::Invalid("Saved document failed verification".into()));
    }
    write_manifest("completed")?;
    let (text, encoding, bom) = decode(&bytes)?;
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
            view,
            root: original.root.clone(),
            relative: original.relative.clone(),
        },
        result,
    ))
}
