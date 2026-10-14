//! Three-way line merge. Produces a buffer; never writes input files.
use crate::{Error, Result, editing::MAX_DOCUMENT_BYTES, text::split_lines};
use clear_contracts::MergeResult;
use similar::{Algorithm, DiffTag, capture_diff_slices_deadline};
use std::{
    ops::Range,
    time::{Duration, Instant},
};

struct Edit<'a> {
    range: Range<usize>,
    replacement: Vec<&'a str>,
    side: usize,
}
fn edits<'a>(base: &[&'a str], side: &[&'a str], index: usize) -> Vec<Edit<'a>> {
    capture_diff_slices_deadline(
        Algorithm::Myers,
        base,
        side,
        Some(Instant::now() + Duration::from_secs(2)),
    )
    .into_iter()
    .filter(|op| op.tag() != DiffTag::Equal)
    .map(|op| Edit {
        range: op.old_range(),
        replacement: side[op.new_range()].to_vec(),
        side: index,
    })
    .collect()
}
fn render(base: &[&str], group: &[Edit<'_>], span: Range<usize>, side: usize) -> String {
    let mut result = String::new();
    let mut cursor = span.start;
    for edit in group.iter().filter(|e| e.side == side) {
        for line in &base[cursor..edit.range.start] {
            result.push_str(line);
        }
        for line in &edit.replacement {
            result.push_str(line);
        }
        cursor = edit.range.end;
    }
    for line in &base[cursor..span.end] {
        result.push_str(line);
    }
    result
}
fn section(output: &mut String, label: &str, text: &str) {
    output.push_str(label);
    output.push('\n');
    output.push_str(text);
    if !text.is_empty() && !text.ends_with(['\n', '\r']) {
        output.push('\n');
    }
}

pub fn three_way(base: &str, left: &str, right: &str) -> Result<MergeResult> {
    if [base, left, right]
        .iter()
        .any(|s| s.len() > MAX_DOCUMENT_BYTES || split_lines(s).len() > 30_000)
    {
        return Err(Error::Invalid(
            "Merge inputs are limited to 4 MiB and 30,000 lines".into(),
        ));
    }
    // Exact shortcuts avoid algorithm timeouts creating unnecessary conflicts.
    if left == right {
        return Ok(MergeResult {
            text: left.into(),
            conflicts: 0,
        });
    }
    if left == base {
        return Ok(MergeResult {
            text: right.into(),
            conflicts: 0,
        });
    }
    if right == base {
        return Ok(MergeResult {
            text: left.into(),
            conflicts: 0,
        });
    }
    let b = split_lines(base);
    let l = split_lines(left);
    let r = split_lines(right);
    let mut changes = edits(&b, &l, 0);
    changes.extend(edits(&b, &r, 1));
    changes.sort_by_key(|e| (e.range.start, e.range.end, e.side));
    let mut output = String::new();
    let mut cursor = 0;
    let mut index = 0;
    let mut conflicts = 0;
    while index < changes.len() {
        let begin = index;
        let start = changes[index].range.start;
        let mut end = changes[index].range.end;
        index += 1;
        while index < changes.len() {
            let next = &changes[index].range;
            let overlaps =
                next.start < end || (next.start == end && (next.is_empty() || start == end));
            if !overlaps {
                break;
            }
            end = end.max(next.end);
            index += 1;
        }
        for line in &b[cursor..start] {
            output.push_str(line);
        }
        let group = &changes[begin..index];
        let original = b[start..end].concat();
        let left = render(&b, group, start..end, 0);
        let right = render(&b, group, start..end, 1);
        if left == right {
            output.push_str(&left);
        } else if left == original {
            output.push_str(&right);
        } else if right == original {
            output.push_str(&left);
        } else {
            conflicts += 1;
            if !output.is_empty() && !output.ends_with(['\n', '\r']) {
                output.push('\n');
            }
            section(&mut output, "<<<<<<< LEFT", &left);
            section(&mut output, "||||||| BASE", &original);
            section(&mut output, "=======", &right);
            output.push_str(">>>>>>> RIGHT\n");
        }
        cursor = end;
    }
    for line in &b[cursor..] {
        output.push_str(line);
    }
    Ok(MergeResult {
        text: output,
        conflicts,
    })
}
