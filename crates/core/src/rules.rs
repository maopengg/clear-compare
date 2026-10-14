use crate::{Error, Result};
use globset::{GlobBuilder, GlobMatcher};
use std::path::Path;

struct Rule {
    matcher: GlobMatcher,
    rooted: bool,
    directory: bool,
}

pub struct Rules(Vec<Rule>);

impl Rules {
    /// Semicolon/newline separated exclusions. ./ anchors at root; bare names
    /// match any component. Trailing slash restricts a rule to directories.
    pub fn parse(input: &str) -> Result<Self> {
        let mut rules = Vec::new();
        for raw in input
            .split([';', '\n'])
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            let raw = raw.strip_prefix('-').unwrap_or(raw);
            if raw.is_empty() {
                return Err(Error::Invalid("Empty exclusion rule".into()));
            }
            let rooted = raw.starts_with("./");
            let directory = raw.ends_with('/');
            let pattern = raw.trim_start_matches("./").trim_end_matches('/');
            if pattern.is_empty() || pattern.split('/').any(|p| p == "..") {
                return Err(Error::Invalid(format!("Invalid exclusion: {raw}")));
            }
            let matcher = GlobBuilder::new(pattern)
                .literal_separator(true)
                .backslash_escape(false)
                .build()
                .map_err(|e| Error::Invalid(e.to_string()))?
                .compile_matcher();
            rules.push(Rule {
                matcher,
                rooted: rooted || pattern.contains('/'),
                directory,
            });
        }
        Ok(Self(rules))
    }

    pub fn excludes(&self, relative: &Path, is_dir: bool) -> bool {
        let components: Vec<_> = relative.iter().collect();
        for rule in &self.0 {
            for i in 1..=components.len() {
                let directory = i < components.len() || is_dir;
                if rule.directory && !directory {
                    continue;
                }
                let candidate = if rule.rooted {
                    components[..i].iter().collect::<std::path::PathBuf>()
                } else {
                    std::path::PathBuf::from(components[i - 1])
                };
                if rule.matcher.is_match(candidate) {
                    return true;
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn root_names_and_subtrees_are_distinct() {
        let r = Rules::parse("-README.md;-./docs/;-__pycache__/;-./.skill/").unwrap();
        assert!(r.excludes(Path::new("a/README.md"), false));
        assert!(r.excludes(Path::new("docs/a.txt"), false));
        assert!(!r.excludes(Path::new("a/docs/a.txt"), false));
        assert!(r.excludes(Path::new("a/__pycache__/b.pyc"), false));
        assert!(!r.excludes(Path::new("skill/a.md"), false));
        assert!(!r.excludes(Path::new("__pycache__"), false));
    }
}
