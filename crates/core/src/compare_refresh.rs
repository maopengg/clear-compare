//! Re-read a single captured file pair without rescanning either root.
use crate::{
    Error, Result,
    compare::{Snapshot, check_cancel, checked_path, hash_file},
};
use clear_contracts::{Entry, Kind, Status, Summary};
use std::{fs, path::Path, sync::atomic::AtomicBool, time::UNIX_EPOCH};
fn read(root: &Path, relative: &Path, cancel: &AtomicBool) -> Result<Option<Entry>> {
    check_cancel(cancel)?;
    let path = checked_path(root, relative, true)?;
    let meta = match fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if !meta.is_file() {
        return Err(Error::Invalid(
            "File type changed. Compare directories again.".into(),
        ));
    }
    Ok(Some(Entry {
        kind: Kind::File,
        bytes: meta.len().to_string(),
        modified_ms: meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|t| t.as_millis().to_string()),
        digest: Some(hash_file(&path, cancel)?),
        error: None,
    }))
}
pub fn file(snapshot: &mut Snapshot, id: &str, cancel: &AtomicBool) -> Result<()> {
    let index = snapshot
        .report
        .rows
        .iter()
        .position(|r| r.id == id)
        .ok_or_else(|| Error::Invalid("Unknown entry".into()))?;
    let row = &snapshot.report.rows[index];
    if !row
        .left
        .as_ref()
        .or(row.right.as_ref())
        .is_some_and(|e| e.kind == Kind::File)
    {
        return Err(Error::Invalid("Expected a captured file".into()));
    }
    let relative = snapshot
        .paths
        .get(id)
        .ok_or_else(|| Error::Invalid("Unknown entry".into()))?;
    if snapshot.rules.excludes(relative, false) {
        return Err(Error::Invalid("Excluded entry".into()));
    }
    // Gather both sides before modifying the snapshot: failed/cancelled reads retain it.
    let left = read(&snapshot.left, relative, cancel)?;
    let right = read(&snapshot.right, relative, cancel)?;
    if left.is_none() && right.is_none() {
        return Err(Error::Invalid(
            "Both files are missing. Compare directories again.".into(),
        ));
    }
    check_cancel(cancel)?;
    let row = &mut snapshot.report.rows[index];
    row.status = crate::compare::status(left.as_ref(), right.as_ref());
    row.left = left;
    row.right = right;
    // Directory statuses already contain the previous aggregation; restore their base
    // state before aggregating again, so a resolved child can clear Changed/Error.
    for row in snapshot
        .report
        .rows
        .iter_mut()
        .filter(|r| crate::tree::is_directory(r))
    {
        row.status = crate::compare::status(row.left.as_ref(), row.right.as_ref());
    }
    crate::tree::aggregate(&mut snapshot.report.rows, &snapshot.paths);
    let mut summary = Summary::default();
    for row in &snapshot.report.rows {
        if crate::tree::is_directory(row)
            && !matches!(row.status, Status::Error | Status::TypeConflict)
        {
            continue;
        }
        match row.status {
            Status::Same => summary.same += 1,
            Status::Changed => summary.changed += 1,
            Status::LeftOnly => summary.left_only += 1,
            Status::RightOnly => summary.right_only += 1,
            Status::MetadataMatch => summary.unverified += 1,
            _ => summary.errors += 1,
        }
    }
    snapshot.report.summary = summary;
    Ok(())
}
