//! Bounded, encoding-aware line comparison. Normalization never modifies files.
use crate::{
    Error, Result,
    compare::{Snapshot, checked_path},
    editing,
};
use clear_contracts::{DiffLine, Kind, TextDiff, TextOptions};
use similar::{Algorithm, DiffTag, capture_diff_slices_deadline};
use std::{
    fs::File,
    io::Read,
    path::Path,
    time::{Duration, Instant},
};

pub fn read_text(path: &Path) -> Result<String> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take((editing::MAX_DOCUMENT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > editing::MAX_DOCUMENT_BYTES {
        return Err(Error::Invalid("Text comparison is limited to 4 MiB".into()));
    }
    let text = editing::decode(&bytes)?.0;
    if split_lines(&text).len() > 30_000 {
        return Err(Error::Invalid(
            "Text comparison is limited to 30,000 lines".into(),
        ));
    }
    Ok(text)
}

/// Retains CR, CRLF, LF and the absence of a final newline.
pub fn split_lines(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    let mut lines = Vec::new();
    let (mut start, mut index) = (0, 0);
    while index < bytes.len() {
        if bytes[index] == b'\r' || bytes[index] == b'\n' {
            if bytes[index] == b'\r' && bytes.get(index + 1) == Some(&b'\n') {
                index += 1;
            }
            index += 1;
            lines.push(&text[start..index]);
            start = index;
        } else {
            index += 1;
        }
    }
    if start < bytes.len() {
        lines.push(&text[start..]);
    }
    lines
}

fn normalized(line: &str, options: &TextOptions) -> String {
    let body = line.trim_end_matches(['\r', '\n']);
    let ending = &line[body.len()..];
    let mut value = if options.ignore_whitespace {
        body.chars()
            .filter(|c| !c.is_whitespace())
            .collect::<String>()
    } else {
        body.to_owned()
    };
    if options.ignore_case {
        value = value.to_lowercase();
    }
    if options.ignore_line_endings {
        if !ending.is_empty() {
            value.push('\n');
        }
    } else {
        value.push_str(ending);
    }
    value
}

pub fn compare_strings(
    path: &str,
    left: &str,
    right: &str,
    options: &TextOptions,
) -> Result<TextDiff> {
    if left.len() > editing::MAX_DOCUMENT_BYTES || right.len() > editing::MAX_DOCUMENT_BYTES {
        return Err(Error::Invalid("Text comparison is limited to 4 MiB".into()));
    }
    let old = split_lines(left);
    let new = split_lines(right);
    if old.len() > 30_000 || new.len() > 30_000 {
        return Err(Error::Invalid("Too many text lines".into()));
    }
    let a = old
        .iter()
        .map(|line| normalized(line, options))
        .collect::<Vec<_>>();
    let b = new
        .iter()
        .map(|line| normalized(line, options))
        .collect::<Vec<_>>();
    let ops = capture_diff_slices_deadline(
        Algorithm::Myers,
        &a,
        &b,
        Some(Instant::now() + Duration::from_secs(2)),
    );
    let mut lines = Vec::new();
    for op in ops {
        if op.tag() == DiffTag::Equal {
            for (i, j) in op.old_range().zip(op.new_range()) {
                lines.push(DiffLine {
                    tag: "equal".into(),
                    old_line: Some(i + 1),
                    new_line: Some(j + 1),
                    text: old[i].into(),
                    right_text: Some(new[j].into()),
                });
            }
        } else {
            for i in op.old_range() {
                lines.push(DiffLine {
                    tag: "delete".into(),
                    old_line: Some(i + 1),
                    new_line: None,
                    text: old[i].into(),
                    right_text: None,
                });
            }
            for j in op.new_range() {
                lines.push(DiffLine {
                    tag: "insert".into(),
                    old_line: None,
                    new_line: Some(j + 1),
                    text: new[j].into(),
                    right_text: None,
                });
            }
        }
    }
    Ok(TextDiff {
        path: path.into(),
        lines,
    })
}

pub fn preview(snapshot: &Snapshot, row_id: &str) -> Result<TextDiff> {
    preview_with_options(snapshot, row_id, &TextOptions::default())
}

pub fn load_pair(snapshot: &Snapshot, row_id: &str) -> Result<(String, String, String)> {
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
            Some(e) if e.kind == Kind::File => read_text(&checked_path(root, path, false)?),
            _ => Err(Error::Invalid("Select a regular text file".into())),
        }
    };
    Ok((
        row.path.clone(),
        load(&snapshot.left, row.left.as_ref())?,
        load(&snapshot.right, row.right.as_ref())?,
    ))
}
pub fn preview_with_options(
    snapshot: &Snapshot,
    row_id: &str,
    options: &TextOptions,
) -> Result<TextDiff> {
    let (path, left, right) = load_pair(snapshot, row_id)?;
    compare_strings(&path, &left, &right, options)
}
