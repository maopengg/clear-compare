//! Tagged conflict navigation and exact fragment selection, shared by native tests and WASM.
use crate::MergeConflict;
use std::{collections::BTreeSet, ops::Range};
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub id: usize,
    pub range: Range<usize>,
    pub line: usize,
}
#[derive(Debug, Clone, Copy)]
pub enum Choice {
    Left,
    Base,
    Right,
    Both,
}
pub fn marker(label: &str, token: &str, id: usize) -> String {
    format!(
        "{label} {} [block:{id}]",
        crate::edit_history::conflict_tag(token)
    )
}
pub fn render(token: &str, c: &MergeConflict) -> String {
    let mut out = String::new();
    for (label, text) in [
        ("<<<<<<< LEFT", c.left.as_str()),
        ("||||||| BASE", c.base.as_str()),
        ("=======", c.right.as_str()),
    ] {
        out.push_str(&marker(label, token, c.id));
        out.push('\n');
        out.push_str(text);
        if !text.is_empty() && !text.ends_with(['\r', '\n']) {
            out.push('\n');
        }
    }
    out.push_str(&marker(">>>>>>> RIGHT", token, c.id));
    out.push('\n');
    out
}
pub fn blocks(text: &str, token: &str) -> Result<Vec<Block>, String> {
    if text.len() > crate::edit_history::MAX_TEXT_BYTES {
        return Err("Merge text exceeds 8 MiB".into());
    }
    let tag = crate::edit_history::conflict_tag(token);
    let prefixes = ["<<<<<<< LEFT", "||||||| BASE", "=======", ">>>>>>> RIGHT"];
    let mut result = Vec::new();
    let mut active = None::<(usize, usize, usize, usize)>;
    let mut seen = BTreeSet::new();
    let bytes = text.as_bytes();
    let mut start = 0;
    let mut line = 1;
    while start < bytes.len() {
        let mut end = start;
        while end < bytes.len() && !matches!(bytes[end], b'\r' | b'\n') {
            end += 1;
        }
        let label = &text[start..end];
        let mut next = end;
        if bytes.get(next) == Some(&b'\r') {
            next += 1;
        }
        if bytes.get(next) == Some(&b'\n') {
            next += 1;
        }
        if label.contains(&tag) {
            let parsed=prefixes.iter().enumerate().find_map(|(step,p)|{
                let prefix=format!("{p} {tag} [block:");let value=label.strip_prefix(&prefix)?.strip_suffix(']')?;
                let id=value.parse::<usize>().ok()?;
                (id<30000 && value==id.to_string()).then_some((step,id))
            }).ok_or("A conflict marker was edited or is malformed; resolve it manually or undo the edit")?;
            match (active,parsed) {
                (None,(0,id)) if seen.insert(id)=>active=Some((id,start,line,1)),
                (Some((id,begin,first,expected)),(step,found)) if id==found && step==expected=>{
                    if step==3{result.push(Block{id,range:begin..next,line:first});active=None;}else{active=Some((id,begin,first,expected+1));}
                }
                _=>return Err("Conflict markers are incomplete, nested or duplicated; resolve manually or undo".into()),
            }
        }
        start = next;
        line += 1;
    }
    if active.is_some() {
        return Err("A conflict has missing closing markers; resolve manually or undo".into());
    }
    Ok(result)
}
pub fn resolve(
    text: &str,
    token: &str,
    conflicts: &[MergeConflict],
    id: usize,
    choice: Choice,
) -> Result<String, String> {
    let block = blocks(text, token)?
        .into_iter()
        .find(|b| b.id == id)
        .ok_or("Selected conflict no longer exists")?;
    let original = conflicts
        .iter()
        .find(|c| c.id == id)
        .ok_or("Conflict provenance is unavailable")?;
    if text[block.range.clone()] != render(token, original) {
        return Err("This conflict was manually edited. Undo the edit before choosing an original version, or finish it manually".into());
    }
    let replacement = match choice {
        Choice::Left => original.left.clone(),
        Choice::Base => original.base.clone(),
        Choice::Right => original.right.clone(),
        Choice::Both => format!("{}{}", original.left, original.right),
    };
    let size = text.len() - block.range.len() + replacement.len();
    if size > crate::edit_history::MAX_TEXT_BYTES {
        return Err("Merge text exceeds 8 MiB".into());
    }
    let mut result = String::with_capacity(size);
    result.push_str(&text[..block.range.start]);
    result.push_str(&replacement);
    result.push_str(&text[block.range.end..]);
    Ok(result)
}

/// DOM textarea offsets count UTF-16 code units rather than UTF-8 bytes.
pub fn selection(text: &str, block: &Block) -> Result<(u32, u32), String> {
    if text.len() > crate::edit_history::MAX_TEXT_BYTES
        || block.range.start > block.range.end
        || block.range.end > text.len()
        || !text.is_char_boundary(block.range.start)
        || !text.is_char_boundary(block.range.end)
    {
        return Err("Conflict selection is stale".into());
    }
    Ok((
        text[..block.range.start].encode_utf16().count() as u32,
        text[..block.range.end].encode_utf16().count() as u32,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    const TOKEN: &str = "12345678-ab";
    fn conflict(id: usize) -> MergeConflict {
        MergeConflict {
            id,
            left: "left".into(),
            base: "base\r\n".into(),
            right: "right\n".into(),
        }
    }
    #[test]
    fn choices_preserve_exact_fragments_including_final_newlines() {
        let c = conflict(0);
        let text = render(TOKEN, &c);
        for (choice, expected) in [
            (Choice::Left, "left"),
            (Choice::Base, "base\r\n"),
            (Choice::Right, "right\n"),
            (Choice::Both, "leftright\n"),
        ] {
            assert_eq!(
                resolve(&text, TOKEN, std::slice::from_ref(&c), 0, choice).unwrap(),
                expected
            );
        }
    }
    #[test]
    fn ids_survive_prior_resolution_and_unicode_selection_uses_utf16_units() {
        let versions = vec![conflict(0), conflict(1)];
        let prefix = "\u{4e2d}\u{1f642}\n";
        let text = format!(
            "{prefix}{}keep\n{}",
            render(TOKEN, &versions[0]),
            render(TOKEN, &versions[1])
        );
        let found = blocks(&text, TOKEN).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].line, 2);
        assert_eq!(selection(&text, &found[0]).unwrap().0, 4);
        let next = resolve(&text, TOKEN, &versions, 0, Choice::Right).unwrap();
        let found = blocks(&next, TOKEN).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, 1);
        let next = resolve(&next, TOKEN, &versions, 1, Choice::Left).unwrap();
        assert_eq!(next, format!("{prefix}right\nkeep\nleft"));
        assert!(blocks(&next, TOKEN).unwrap().is_empty());
    }
    #[test]
    fn malformed_or_manual_changes_never_get_automatically_discarded() {
        let c = conflict(0);
        let text = render(TOKEN, &c);
        assert!(
            resolve(
                &text.replace("left", "manual"),
                TOKEN,
                std::slice::from_ref(&c),
                0,
                Choice::Right
            )
            .is_err()
        );
        for bad in [
            format!("{text}{text}"),
            text.replace("||||||| BASE", "broken"),
            text.replace("[block:0]", "[block:01]"),
            text.replace("=======", "<<<<<<"),
            format!("{text}[cc:{TOKEN}] malformed\n"),
        ] {
            assert!(blocks(&bad, TOKEN).is_err());
        }
        assert!(
            blocks("Title\n=======\n<<<<<<< unrelated\n", TOKEN)
                .unwrap()
                .is_empty()
        );
        assert!(
            selection(
                "hi",
                &Block {
                    id: 0,
                    range: 0..9,
                    line: 1
                }
            )
            .is_err()
        );
    }
    #[test]
    fn resolutions_are_regular_undoable_edits_and_reintroduce_guarded_markers() {
        let c = conflict(0);
        let text = render(TOKEN, &c);
        let next = resolve(&text, TOKEN, &[c], 0, Choice::Left).unwrap();
        let mut h = crate::edit_history::History::default();
        h.reset(&text);
        h.record(&next).unwrap();
        assert!(!crate::edit_history::has_conflict(h.current(), TOKEN));
        h.undo();
        assert_eq!(h.current(), text);
        assert!(crate::edit_history::has_conflict(h.current(), TOKEN));
        h.redo();
        assert_eq!(h.current(), "left");
    }
}
