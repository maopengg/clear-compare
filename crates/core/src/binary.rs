//! Fixed-size binary pages; file size does not determine memory use.
use crate::{
    Error, Result,
    compare::{Snapshot, checked_path},
};
use clear_contracts::{BinaryLine, BinaryPage, Entry, Kind};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};
pub const PAGE_BYTES: u64 = 4096;
fn read(
    root: &Path,
    relative: &Path,
    entry: Option<&Entry>,
    offset: u64,
) -> Result<(Vec<u8>, u64)> {
    match entry {
        None => Ok((Vec::new(), 0)),
        Some(e) if e.kind == Kind::File && e.error.is_none() => {
            let mut file = File::open(checked_path(root, relative, false)?)?;
            let size = file.metadata()?.len();
            file.seek(SeekFrom::Start(offset))?;
            let mut bytes = Vec::new();
            file.take(PAGE_BYTES).read_to_end(&mut bytes)?;
            Ok((bytes, size))
        }
        _ => Err(Error::Invalid(
            "Binary viewer requires regular files".into(),
        )),
    }
}
fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}
fn ascii(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| {
            if (32..=126).contains(b) {
                char::from(*b)
            } else {
                '.'
            }
        })
        .collect()
}
pub fn page(snapshot: &Snapshot, row_id: &str, offset: &str) -> Result<BinaryPage> {
    let offset = offset
        .parse::<u64>()
        .map_err(|_| Error::Invalid("Invalid binary offset".into()))?;
    if offset % PAGE_BYTES != 0 {
        return Err(Error::Invalid(
            "Binary offset must be a 4096-byte boundary".into(),
        ));
    }
    let relative = snapshot
        .paths
        .get(row_id)
        .ok_or_else(|| Error::Invalid("Unknown entry".into()))?;
    let row = snapshot
        .report
        .rows
        .iter()
        .find(|r| r.id == row_id)
        .ok_or_else(|| Error::Invalid("Unknown entry".into()))?;
    let (left, left_size) = read(&snapshot.left, relative, row.left.as_ref(), offset)?;
    let (right, right_size) = read(&snapshot.right, relative, row.right.as_ref(), offset)?;
    let mut lines = Vec::new();
    for start in (0..left.len().max(right.len())).step_by(16) {
        let a = &left[start.min(left.len())..(start + 16).min(left.len())];
        let b = &right[start.min(right.len())..(start + 16).min(right.len())];
        lines.push(BinaryLine {
            offset: offset
                .checked_add(start as u64)
                .ok_or_else(|| Error::Invalid("Offset overflow".into()))?
                .to_string(),
            left_hex: hex(a),
            right_hex: hex(b),
            left_ascii: ascii(a),
            right_ascii: ascii(b),
            changed: a != b,
        });
    }
    Ok(BinaryPage {
        path: row.path.clone(),
        offset: offset.to_string(),
        left_size: left_size.to_string(),
        right_size: right_size.to_string(),
        lines,
    })
}
