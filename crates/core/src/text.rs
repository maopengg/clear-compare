//! Bounded, encoding-aware line comparison. Normalization never modifies files.
use crate::{
    Error, Result,
    compare::{Snapshot, checked_path},
    editing,
};
use clear_contracts::{DiffLine, Kind, TextDiff, TextOptions, TextRange};
use similar::{Algorithm, DiffTag, capture_diff_slices_deadline};
use std::{
    fs::File,
    io::Read,
    ops::Range,
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

fn units(line: &str, options: &TextOptions) -> Option<(Vec<char>, Vec<Range<usize>>)> {
    let body = line.trim_end_matches(['\r', '\n']);
    let mut filtered = String::new();
    let mut positions = Vec::new();
    for (start, ch) in body.char_indices() {
        if options.ignore_whitespace && ch.is_whitespace() {
            continue;
        }
        filtered.push(ch);
        let count = if options.ignore_case {
            ch.to_lowercase().count()
        } else {
            1
        };
        positions.extend(std::iter::repeat_n(start..start + ch.len_utf8(), count));
    }
    let lower = if options.ignore_case {
        filtered.to_lowercase()
    } else {
        filtered
    };
    let mut chars = lower.chars().collect::<Vec<_>>();
    if chars.len() != positions.len() {
        return None;
    }
    if options.ignore_line_endings {
        if body.len() < line.len() {
            chars.push('\n');
            positions.push(body.len()..line.len());
        }
    } else {
        for (n, ch) in line[body.len()..].char_indices() {
            chars.push(ch);
            positions.push(body.len() + n..body.len() + n + ch.len_utf8());
        }
    }
    Some((chars, positions))
}
fn full(line: &str) -> Vec<TextRange> {
    if line.is_empty() {
        Vec::new()
    } else {
        vec![TextRange {
            start: 0,
            end: line.len(),
        }]
    }
}
fn ranges(positions: &[Range<usize>], range: Range<usize>) -> Vec<TextRange> {
    let mut result: Vec<TextRange> = Vec::new();
    for p in &positions[range] {
        if let Some(last) = result.last_mut()
            && p.start <= last.end
        {
            last.end = last.end.max(p.end);
            continue;
        }
        result.push(TextRange {
            start: p.start,
            end: p.end,
        });
    }
    result
}
fn intraline(
    left: &str,
    right: &str,
    options: &TextOptions,
    deadline: Instant,
    budget: &mut usize,
) -> (Vec<TextRange>, Vec<TextRange>, bool) {
    if left.len() + right.len() > 16384 || Instant::now() >= deadline {
        return (full(left), full(right), true);
    }
    let (Some((a, ap)), Some((b, bp))) = (units(left, options), units(right, options)) else {
        return (full(left), full(right), true);
    };
    let cost = a.len() + b.len();
    if cost > 4096 || cost > *budget {
        return (full(left), full(right), true);
    }
    *budget -= cost;
    let ops = capture_diff_slices_deadline(Algorithm::Myers, &a, &b, Some(deadline));
    let mut l = Vec::new();
    let mut r = Vec::new();
    for op in ops {
        if op.tag() != DiffTag::Equal {
            l.extend(ranges(&ap, op.old_range()));
            r.extend(ranges(&bp, op.new_range()));
        }
    }
    (l, r, Instant::now() >= deadline)
}
pub fn compare_strings(
    path: &str,
    left: &str,
    right: &str,
    options: &TextOptions,
) -> Result<TextDiff> {
    if left.len() > clear_contracts::edit_history::MAX_TEXT_BYTES
        || right.len() > clear_contracts::edit_history::MAX_TEXT_BYTES
    {
        return Err(Error::Invalid(
            "Decoded text comparison is limited to 8 MiB".into(),
        ));
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
    let mut intraline_limited = false;
    let deadline = Instant::now() + Duration::from_millis(150);
    let mut budget = 100000;
    for op in ops {
        if op.tag() == DiffTag::Equal {
            for (i, j) in op.old_range().zip(op.new_range()) {
                lines.push(DiffLine {
                    intraline_limited: false,
                    changes: Vec::new(),
                    tag: "equal".into(),
                    old_line: Some(i + 1),
                    new_line: Some(j + 1),
                    text: old[i].into(),
                    right_text: Some(new[j].into()),
                });
            }
        } else {
            let old_range = op.old_range();
            let new_range = op.new_range();
            let pairs = old_range
                .clone()
                .zip(new_range.clone())
                .map(|(i, j)| {
                    let (l, r, limited) = intraline(old[i], new[j], options, deadline, &mut budget);
                    intraline_limited |= limited;
                    (l, r, limited)
                })
                .collect::<Vec<_>>();
            for (n, i) in old_range.enumerate() {
                lines.push(DiffLine {
                    intraline_limited: pairs.get(n).is_some_and(|p| p.2),
                    changes: pairs
                        .get(n)
                        .map(|p| p.0.clone())
                        .unwrap_or_else(|| full(old[i])),
                    tag: "delete".into(),
                    old_line: Some(i + 1),
                    new_line: None,
                    text: old[i].into(),
                    right_text: None,
                });
            }
            for (n, j) in new_range.enumerate() {
                lines.push(DiffLine {
                    intraline_limited: pairs.get(n).is_some_and(|p| p.2),
                    changes: pairs
                        .get(n)
                        .map(|p| p.1.clone())
                        .unwrap_or_else(|| full(new[j])),
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
        id: crate::unique_id(),
        intraline_limited,
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
