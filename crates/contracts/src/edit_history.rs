//! Bounded UTF-8 splice history shared by the editor and native tests.
use std::collections::VecDeque;
pub const MAX_TEXT_BYTES: usize = 8 * 1024 * 1024;
#[derive(Debug)]
struct Edit {
    start: usize,
    removed: String,
    inserted: String,
}
impl Edit {
    fn bytes(&self) -> usize {
        self.removed.len() + self.inserted.len()
    }
}
#[derive(Debug)]
pub struct History {
    current: String,
    undo: VecDeque<Edit>,
    redo: Vec<Edit>,
    bytes: usize,
    max_bytes: usize,
    max_steps: usize,
}
impl Default for History {
    fn default() -> Self {
        Self::with_limits(16 * 1024 * 1024, 128)
    }
}
impl History {
    pub fn with_limits(max_bytes: usize, max_steps: usize) -> Self {
        Self {
            current: String::new(),
            undo: VecDeque::new(),
            redo: Vec::new(),
            bytes: 0,
            max_bytes,
            max_steps,
        }
    }
    pub fn reset(&mut self, text: &str) {
        self.current = text.into();
        self.undo.clear();
        self.redo.clear();
        self.bytes = 0;
    }
    pub fn current(&self) -> &str {
        &self.current
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
    pub fn record(&mut self, text: &str) -> Result<(), String> {
        if text.len() > MAX_TEXT_BYTES {
            return Err("Editor text exceeds 8 MiB decoded UTF-8".into());
        }
        if text == self.current {
            return Ok(());
        }
        let mut prefix = self
            .current
            .bytes()
            .zip(text.bytes())
            .take_while(|(a, b)| a == b)
            .count();
        while !self.current.is_char_boundary(prefix) || !text.is_char_boundary(prefix) {
            prefix -= 1;
        }
        let remaining = (self.current.len() - prefix).min(text.len() - prefix);
        let mut suffix = self
            .current
            .as_bytes()
            .iter()
            .rev()
            .zip(text.as_bytes().iter().rev())
            .take(remaining)
            .take_while(|(a, b)| a == b)
            .count();
        while !self.current.is_char_boundary(self.current.len() - suffix)
            || !text.is_char_boundary(text.len() - suffix)
        {
            suffix -= 1;
        }
        for edit in self.redo.drain(..) {
            self.bytes -= edit.bytes();
        }
        let edit = Edit {
            start: prefix,
            removed: self.current[prefix..self.current.len() - suffix].into(),
            inserted: text[prefix..text.len() - suffix].into(),
        };
        self.bytes += edit.bytes();
        self.undo.push_back(edit);
        self.current = text.into();
        while self.undo.len() > self.max_steps || self.bytes > self.max_bytes {
            let Some(edit) = self.undo.pop_front() else {
                break;
            };
            self.bytes -= edit.bytes();
        }
        Ok(())
    }
    pub fn undo(&mut self) -> bool {
        let Some(edit) = self.undo.pop_back() else {
            return false;
        };
        self.current
            .replace_range(edit.start..edit.start + edit.inserted.len(), &edit.removed);
        self.redo.push(edit);
        true
    }
    pub fn redo(&mut self) -> bool {
        let Some(edit) = self.redo.pop() else {
            return false;
        };
        self.current
            .replace_range(edit.start..edit.start + edit.removed.len(), &edit.inserted);
        self.undo.push_back(edit);
        true
    }
}
pub fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}
pub fn conflict_tag(token: &str) -> String {
    format!("[cc:{token}]")
}
pub fn has_conflict(text: &str, token: &str) -> bool {
    text.contains(&conflict_tag(token))
}
fn tags(text: &str) -> impl Iterator<Item = &str> {
    text.match_indices("[cc:").filter_map(|(start, _)| {
        let tail = &text[start + 4..];
        let end = tail.as_bytes().iter().take(129).position(|b| *b == b']')?;
        Some(&tail[..end])
    })
}
pub fn conflict_tokens(text: &str) -> Vec<String> {
    text.lines()
        .filter(|line| {
            line.starts_with("<<<<<<< ")
                || line.starts_with("||||||| ")
                || line.starts_with("======= ")
                || line.starts_with(">>>>>>> ")
        })
        .flat_map(tags)
        .filter(|token| {
            token.len() >= 8 && token.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-')
        })
        .map(str::to_owned)
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect()
}
pub fn has_any_conflict(text: &str, tokens: &[String]) -> bool {
    let known = tokens
        .iter()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    tags(text).any(|token| known.contains(token))
}
pub fn generic_markers(text: &str) -> usize {
    text.lines()
        .filter(|l| {
            l.starts_with("<<<<<<< ") || l.starts_with("||||||| ") || l.starts_with(">>>>>>> ")
        })
        .count()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unicode_splices_undo_redo_and_branch_correctly() {
        let mut h = History::default();
        h.reset("中文🙂");
        h.record("中法🙂").unwrap();
        h.record("中法🙂 end").unwrap();
        assert!(h.undo());
        assert_eq!(h.current(), "中法🙂");
        assert!(h.undo());
        assert_eq!(h.current(), "中文🙂");
        assert!(h.redo());
        h.record("新内容").unwrap();
        assert!(!h.can_redo());
        h.undo();
        assert_eq!(h.current(), "中法🙂");
    }
    #[test]
    fn bounded_history_prunes_old_steps_and_preserves_current() {
        let mut h = History::with_limits(4, 2);
        h.reset("a");
        h.record("ab").unwrap();
        h.record("abc").unwrap();
        h.record("abcd").unwrap();
        h.undo();
        h.undo();
        assert_eq!(h.current(), "ab");
        assert!(!h.can_undo());
        h.redo();
        assert_eq!(h.current(), "abc");
        h.record("long replacement").unwrap();
        assert!(!h.can_undo());
        assert_eq!(h.current(), "long replacement");
    }
    #[test]
    fn line_endings_normalize_without_changing_unicode() {
        assert_eq!(normalize("中\r\n文\rend\n"), "中\n文\nend\n");
        assert!(has_conflict("======= [cc:abc]", "abc"));
        assert!(!has_conflict("=======", "abc"));
    }
}

#[cfg(test)]
mod more_tests {
    use super::*;
    #[test]
    fn unicode_edits_round_trip_across_different_byte_boundaries() {
        let samples = [
            "",
            "é",
            "e\u{301}",
            "中文",
            "🙂",
            "🙃",
            "a🙂b",
            "a🧑‍💻b",
            "中文\n🙂",
        ];
        for before in samples {
            for after in samples {
                let mut h = History::default();
                h.reset(before);
                h.record(after).unwrap();
                assert_eq!(h.current(), after);
                if before != after {
                    assert!(h.undo());
                    assert_eq!(h.current(), before);
                    assert!(h.redo());
                    assert_eq!(h.current(), after);
                } else {
                    assert!(!h.can_undo());
                }
            }
        }
    }
    #[test]
    fn generated_tags_are_detected_without_confusing_plain_separator_lines() {
        let text = "<<<<<<< LEFT [cc:01234567-ab]\na\n||||||| BASE [cc:01234567-ab]\nb\n======= [cc:01234567-ab]\nc\n>>>>>>> RIGHT [cc:01234567-ab]\n";
        let tokens = conflict_tokens(text);
        assert_eq!(tokens, vec!["01234567-ab"]);
        assert!(has_any_conflict(text, &tokens));
        assert!(!has_any_conflict("Title\n=======\n", &tokens));
    }
}
