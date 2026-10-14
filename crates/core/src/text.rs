use crate::{
    Error, Result,
    compare::{Snapshot, checked_path},
};
use clear_contracts::{DiffLine, Kind, TextDiff};
use similar::{ChangeTag, TextDiff as Diff};
use std::{fs::File, io::Read, path::Path, time::Duration};

const MAX_TEXT: u64 = 2 * 1024 * 1024;

pub fn read_utf8(path: &Path) -> Result<String> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_TEXT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_TEXT {
        return Err(Error::Invalid(
            "Text preview is limited to 2 MiB. Use the binary viewer or narrow the file.".into(),
        ));
    }
    if bytes.contains(&0) {
        return Err(Error::Invalid(
            "Binary or unsupported encoding; UTF-8 text is required for this preview".into(),
        ));
    }
    let text = String::from_utf8(bytes)
        .map_err(|_| Error::Invalid("Unsupported encoding; this preview requires UTF-8".into()))?;
    if text.lines().count() > 30_000 {
        return Err(Error::Invalid(
            "Text preview is limited to 30,000 lines".into(),
        ));
    }
    Ok(text.trim_start_matches('\u{feff}').to_string())
}

pub fn preview(snapshot: &Snapshot, row_id: &str) -> Result<TextDiff> {
    let path = snapshot
        .paths
        .get(row_id)
        .ok_or_else(|| Error::Invalid("Unknown entry".into()))?;
    let row = snapshot
        .report
        .rows
        .iter()
        .find(|r| r.id == row_id)
        .ok_or_else(|| Error::Invalid("Unknown entry".into()))?;
    let load = |root: &Path, entry: Option<&clear_contracts::Entry>| -> Result<String> {
        match entry {
            None => Ok(String::new()),
            Some(e) if e.kind == Kind::File => read_utf8(&checked_path(root, path, false)?),
            _ => Err(Error::Invalid("Select a regular text file".into())),
        }
    };
    let left = load(&snapshot.left, row.left.as_ref())?;
    let right = load(&snapshot.right, row.right.as_ref())?;
    let diff = Diff::configure()
        .timeout(Duration::from_secs(2))
        .diff_lines(&left, &right);
    let lines = diff
        .iter_all_changes()
        .map(|c| DiffLine {
            tag: match c.tag() {
                ChangeTag::Delete => "delete",
                ChangeTag::Insert => "insert",
                ChangeTag::Equal => "equal",
            }
            .into(),
            old_line: c.old_index().map(|n| n + 1),
            new_line: c.new_index().map(|n| n + 1),
            text: c.value().to_owned(),
        })
        .collect();
    Ok(TextDiff {
        path: row.path.clone(),
        lines,
    })
}
