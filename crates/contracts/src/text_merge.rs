//! Pure, bounded hunk selection. Filesystem writes remain in the native editor.
use crate::{DiffLine, LineEnding, Side, TextDiff, edit_history, text_view};
use std::{
    collections::{BTreeMap, BTreeSet, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
};
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Preview {
    pub diff: TextDiff,
    /// Display row -> original hunk ID, including continuation rows on later pages.
    pub hunks: BTreeMap<usize, usize>,
}
pub fn text(diff: &TextDiff, side: &Side) -> String {
    diff.lines
        .iter()
        .filter_map(|line| match side {
            Side::Left => line.old_line.map(|_| line.text.as_str()),
            Side::Right => line
                .new_line
                .map(|_| line.right_text.as_deref().unwrap_or(&line.text)),
        })
        .collect()
}
fn copied(text: &str, ending: Option<&LineEnding>) -> String {
    let suffix = match ending {
        Some(LineEnding::Lf) => "\n",
        Some(LineEnding::CrLf) => "\r\n",
        Some(LineEnding::Cr) => "\r",
        _ => return text.into(),
    };
    if text_view::ending(text) == "∅" {
        text.into()
    } else {
        format!("{}{suffix}", text_view::body(text))
    }
}
pub fn preview(
    original: &TextDiff,
    selected: &BTreeSet<usize>,
    target: &Side,
    ending: Option<&LineEnding>,
) -> Result<Preview, String> {
    let empty = BTreeSet::new();
    if *target == Side::Left {
        preview_both(original, selected, &empty, ending, None)
    } else {
        preview_both(original, &empty, selected, None, ending)
    }
}

pub fn preview_both(
    original: &TextDiff,
    left: &BTreeSet<usize>,
    right: &BTreeSet<usize>,
    left_ending: Option<&LineEnding>,
    right_ending: Option<&LineEnding>,
) -> Result<Preview, String> {
    if !left.is_disjoint(right) {
        return Err("A hunk cannot target both sides".into());
    }
    let mut lines = Vec::new();
    let mut hunks = BTreeMap::new();
    let (mut i, mut group, mut display_row) = (0, 0, 0);
    while i < original.lines.len() {
        if original.lines[i].tag == "equal" {
            lines.push(original.lines[i].clone());
            i += 1;
            display_row += 1;
            continue;
        }
        let start = i;
        while i < original.lines.len() && original.lines[i].tag != "equal" {
            i += 1;
        }
        let block = &original.lines[start..i];
        if left.contains(&group) || right.contains(&group) {
            let target = if left.contains(&group) {
                &Side::Left
            } else {
                &Side::Right
            };
            let ending = if *target == Side::Left {
                left_ending
            } else {
                right_ending
            };
            for line in block.iter().filter(|line| {
                if *target == Side::Right {
                    line.old_line.is_some()
                } else {
                    line.new_line.is_some()
                }
            }) {
                let source = if *target == Side::Right {
                    line.text.as_str()
                } else {
                    line.right_text.as_deref().unwrap_or(&line.text)
                };
                let destination = copied(source, ending);
                let (left, right) = if *target == Side::Right {
                    (source.to_owned(), destination)
                } else {
                    (destination, source.to_owned())
                };
                let changes = if text_view::ending(&left) != text_view::ending(&right) {
                    vec![crate::TextRange {
                        start: text_view::body(&left).len(),
                        end: left.len(),
                    }]
                } else {
                    Vec::new()
                };
                lines.push(DiffLine {
                    tag: "equal".into(),
                    old_line: Some(0),
                    new_line: Some(0),
                    text: left,
                    right_text: Some(right),
                    changes,
                    intraline_limited: false,
                });
                display_row += 1;
            }
        } else {
            let count = block
                .iter()
                .filter(|l| l.old_line.is_some())
                .count()
                .max(block.iter().filter(|l| l.new_line.is_some()).count());
            for row in display_row..display_row + count {
                hunks.insert(row, group);
            }
            display_row += count;
            lines.extend_from_slice(block);
        }
        group += 1;
    }
    if left.iter().chain(right).any(|id| *id >= group) {
        return Err("Unknown text hunk".into());
    }
    let (mut old, mut new) = (0, 0);
    for line in &mut lines {
        if line.old_line.is_some() {
            old += 1;
            line.old_line = Some(old);
        }
        if line.new_line.is_some() {
            new += 1;
            line.new_line = Some(new);
        }
    }
    let mut generation = DefaultHasher::new();
    left.hash(&mut generation);
    right.hash(&mut generation);
    format!("{left_ending:?}:{right_ending:?}").hash(&mut generation);
    let diff = TextDiff {
        id: format!("{}:both:{}", original.id, generation.finish()),
        path: original.path.clone(),
        intraline_limited: original.intraline_limited,
        lines,
    };
    if text(&diff, &Side::Left).len() > edit_history::MAX_TEXT_BYTES
        || text(&diff, &Side::Right).len() > edit_history::MAX_TEXT_BYTES
    {
        return Err("Merged text exceeds 8 MiB".into());
    }
    Ok(Preview { diff, hunks })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn line(tag: &str, text: &str) -> DiffLine {
        DiffLine {
            tag: tag.into(),
            text: text.into(),
            old_line: (tag != "insert").then_some(1),
            new_line: (tag != "delete").then_some(1),
            right_text: None,
            changes: Vec::new(),
            intraline_limited: false,
        }
    }
    fn fixture() -> TextDiff {
        TextDiff {
            id: "test".into(),
            path: "example.txt".into(),
            intraline_limited: false,
            lines: vec![
                line("equal", "head\n"),
                line("delete", "左🙂\r\n"),
                line("delete", "extra\n"),
                line("insert", "right\n"),
                line("equal", "middle\n"),
                line("insert", "tail"),
            ],
        }
    }
    #[test]
    fn independent_targets_preserve_both_buffers_and_their_line_endings() {
        let d = fixture();
        let p = preview_both(
            &d,
            &BTreeSet::from([1]),
            &BTreeSet::from([0]),
            Some(&LineEnding::CrLf),
            Some(&LineEnding::Lf),
        )
        .unwrap();
        assert_eq!(
            text(&p.diff, &Side::Left),
            "head\n左🙂\r\nextra\nmiddle\ntail"
        );
        assert_eq!(
            text(&p.diff, &Side::Right),
            "head\n左🙂\nextra\nmiddle\ntail"
        );
        assert!(p.hunks.is_empty());
        assert!(preview_both(&d, &BTreeSet::from([0]), &BTreeSet::from([0]), None, None).is_err());
        assert!(preview_both(&d, &BTreeSet::from([9]), &BTreeSet::new(), None, None).is_err());
    }
    #[test]
    fn selected_hunk_preserves_unrelated_changes_and_renumbers() {
        let d = fixture();
        let p = preview(
            &d,
            &BTreeSet::from([0]),
            &Side::Right,
            Some(&LineEnding::Lf),
        )
        .unwrap();
        assert_eq!(
            text(&p.diff, &Side::Right),
            "head\n左🙂\nextra\nmiddle\ntail"
        );
        assert_eq!(text(&p.diff, &Side::Left), text(&d, &Side::Left));
        assert_eq!(p.hunks, BTreeMap::from([(4, 1)]));
        assert!(text_view::significant_ending(
            Some(&p.diff.lines[1]),
            Some(&p.diff.lines[1])
        ));
        assert_eq!(p.diff.lines.last().unwrap().new_line, Some(5));
    }
    #[test]
    fn insertion_and_deletion_in_both_directions_are_exact() {
        let d = fixture();
        let p = preview(
            &d,
            &BTreeSet::from([1]),
            &Side::Left,
            Some(&LineEnding::CrLf),
        )
        .unwrap();
        assert_eq!(
            text(&p.diff, &Side::Left),
            "head\n左🙂\r\nextra\nmiddle\ntail"
        );
        let p = preview(&d, &BTreeSet::from([1]), &Side::Right, None).unwrap();
        assert_eq!(text(&p.diff, &Side::Right), "head\nright\nmiddle\n");
        assert!(preview(&d, &BTreeSet::from([2]), &Side::Right, None).is_err());
    }
    #[test]
    fn ignored_equal_text_and_page_continuations_survive() {
        let mut d = fixture();
        d.lines[0].right_text = Some("HEAD\r\n".into());
        assert_eq!(
            text(
                &preview(&d, &BTreeSet::new(), &Side::Left, None)
                    .unwrap()
                    .diff,
                &Side::Right
            ),
            text(&d, &Side::Right)
        );
        d.lines = vec![line("delete", "x\n"); 600];
        let p = preview(&d, &BTreeSet::new(), &Side::Right, None).unwrap();
        assert_eq!(p.hunks[&500], 0);
        let p = preview(&d, &BTreeSet::from([0]), &Side::Right, None).unwrap();
        assert_eq!(text(&p.diff, &Side::Right), "x\n".repeat(600));
    }
}
