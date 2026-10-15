//! Immutable row alignment, navigation and safe rendering of original text.
use crate::{DiffLine, TextRange};
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub left: Option<usize>,
    pub right: Option<usize>,
    pub changed: bool,
}
pub fn align(lines: &[DiffLine]) -> Vec<Row> {
    let mut rows = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].tag == "equal" {
            rows.push(Row {
                left: Some(i),
                right: Some(i),
                changed: false,
            });
            i += 1;
            continue;
        }
        let start = i;
        while i < lines.len() && lines[i].tag != "equal" {
            i += 1;
        }
        let left = (start..i)
            .filter(|n| lines[*n].old_line.is_some())
            .collect::<Vec<_>>();
        let right = (start..i)
            .filter(|n| lines[*n].new_line.is_some())
            .collect::<Vec<_>>();
        for n in 0..left.len().max(right.len()) {
            rows.push(Row {
                left: left.get(n).copied(),
                right: right.get(n).copied(),
                changed: true,
            });
        }
    }
    rows
}
pub fn hunks(rows: &[Row]) -> Vec<usize> {
    rows.iter()
        .enumerate()
        .filter_map(|(i, r)| (r.changed && (i == 0 || !rows[i - 1].changed)).then_some(i))
        .collect()
}
pub fn body(text: &str) -> &str {
    text.trim_end_matches(['\r', '\n'])
}
pub fn ending(text: &str) -> &'static str {
    if text.ends_with("\r\n") {
        "CRLF"
    } else if text.ends_with('\r') {
        "CR"
    } else if text.ends_with('\n') {
        "LF"
    } else {
        "∅"
    }
}
pub fn pieces(text: &str, changes: &[TextRange]) -> Vec<(String, bool)> {
    let body = body(text);
    let mut ranges = changes.to_vec();
    ranges.sort_by_key(|r| r.start);
    if ranges.iter().any(|r| {
        r.start > r.end
            || r.end > text.len()
            || !text.is_char_boundary(r.start)
            || !text.is_char_boundary(r.end)
    }) {
        return vec![(body.into(), false)];
    }
    let mut result = Vec::new();
    let mut cursor = 0;
    for r in ranges {
        let start = r.start.max(cursor).min(body.len());
        let end = r.end.min(body.len());
        if start >= end {
            continue;
        }
        if cursor < start {
            result.push((body[cursor..start].into(), false));
        }
        result.push((body[start..end].into(), true));
        cursor = end;
    }
    if cursor < body.len() {
        result.push((body[cursor..].into(), false));
    }
    result
}
pub fn matches(lines: &[DiffLine], rows: &[Row], query: &str) -> Result<Vec<usize>, String> {
    if query.len() > 4096 {
        return Err("Search is limited to 4096 bytes".into());
    }
    if query.is_empty() {
        return Ok(Vec::new());
    }
    Ok(rows
        .iter()
        .enumerate()
        .filter_map(|(i, r)| {
            let hit = r.left.is_some_and(|n| lines[n].text.contains(query))
                || r.right.is_some_and(|n| {
                    lines[n]
                        .right_text
                        .as_deref()
                        .unwrap_or(&lines[n].text)
                        .contains(query)
                });
            hit.then_some(i)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn line(tag: &str, left: Option<usize>, right: Option<usize>, text: &str) -> DiffLine {
        DiffLine {
            intraline_limited: false,
            tag: tag.into(),
            old_line: left,
            new_line: right,
            text: text.into(),
            right_text: None,
            changes: Vec::new(),
        }
    }
    #[test]
    fn alignment_and_search_cover_all_rows_before_paging() {
        let mut lines = (0..600)
            .map(|i| line("equal", Some(i + 1), Some(i + 1), "same\n"))
            .collect::<Vec<_>>();
        lines.extend([
            line("delete", Some(601), None, "left\n"),
            line("delete", Some(602), None, "removed\n"),
            line("insert", None, Some(601), "target\n"),
        ]);
        let rows = align(&lines);
        assert_eq!(rows.len(), 602);
        assert_eq!(rows[600].left, Some(600));
        assert_eq!(rows[600].right, Some(602));
        assert_eq!(rows[601].right, None);
        assert_eq!(hunks(&rows), vec![600]);
        assert_eq!(matches(&lines, &rows, "target").unwrap(), vec![600]);
        assert_eq!(matches(&lines, &rows, "removed").unwrap(), vec![601]);
        assert!(matches(&lines, &rows, "TARGET").unwrap().is_empty());
        assert!(matches(&lines, &rows, &"x".repeat(4097)).is_err());
        lines[0].right_text = Some("normalized original\r\n".into());
        assert_eq!(
            matches(&lines, &align(&lines), "normalized").unwrap(),
            vec![0]
        );
    }
    #[test]
    fn marked_fragments_never_change_or_split_original_unicode() {
        let text = "a\u{4e2d}\u{1f642}b\r\n";
        let ranges = vec![
            TextRange { start: 1, end: 8 },
            TextRange { start: 4, end: 9 },
        ];
        let p = pieces(text, &ranges);
        assert_eq!(
            p.iter().map(|p| p.0.as_str()).collect::<String>(),
            body(text)
        );
        assert!(p.iter().any(|p| p.1));
        assert_eq!(
            pieces(text, &[TextRange { start: 2, end: 4 }]),
            vec![(body(text).to_owned(), false)]
        );
        assert_eq!(ending(text), "CRLF");
        assert_eq!(ending("x"), "∅");
    }
}

/// Cap layout work for a single very long line without changing the source model.
pub fn preview(text: &str, changes: &[TextRange]) -> (Vec<(String, bool)>, bool) {
    let Some((end, _)) = body(text).char_indices().nth(8192) else {
        return (pieces(text, changes), false);
    };
    if changes.iter().any(|r| {
        r.start > r.end
            || r.end > text.len()
            || !text.is_char_boundary(r.start)
            || !text.is_char_boundary(r.end)
    }) {
        return (vec![(text[..end].into(), false)], true);
    }
    let clipped = changes
        .iter()
        .filter_map(|r| {
            (r.start < end).then_some(TextRange {
                start: r.start,
                end: r.end.min(end),
            })
        })
        .collect::<Vec<_>>();
    (pieces(&text[..end], &clipped), true)
}
#[cfg(test)]
mod preview_tests {
    #[test]
    fn long_unicode_line_is_visibly_truncated_at_a_character_boundary() {
        let text = format!("{}END\r\n", "\u{1f642}".repeat(9000));
        let (pieces, truncated) = super::preview(
            &text,
            &[crate::TextRange {
                start: 0,
                end: text.len(),
            }],
        );
        assert!(truncated);
        let shown = pieces.iter().map(|p| p.0.as_str()).collect::<String>();
        assert_eq!(shown.chars().count(), 8192);
        assert!(pieces.iter().all(|p| p.1));
        assert!(text.ends_with("END\r\n"));
    }
}

pub fn significant_ending(left: Option<&DiffLine>, right: Option<&DiffLine>) -> bool {
    let Some((left, right)) = left.zip(right) else {
        return false;
    };
    let rtext = right.right_text.as_deref().unwrap_or(&right.text);
    ending(&left.text) != ending(rtext)
        && [left, right].iter().any(|line| {
            !line.intraline_limited && line.changes.iter().any(|r| r.end > body(&line.text).len())
        })
}
