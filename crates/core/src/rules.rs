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
        if input.len() > 32 * 1024 {
            return Err(Error::Invalid("Exclusion rules exceed 32 KiB".into()));
        }
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
            if rules.len() > 256 {
                return Err(Error::Invalid(
                    "At most 256 exclusion rules are supported".into(),
                ));
            }
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

/// Build an exact, root-anchored rule from captured native path components.
pub fn exclude_entry(
    snapshot: &crate::compare::Snapshot,
    id: &str,
    current: &str,
) -> Result<String> {
    let relative = snapshot
        .paths
        .get(id)
        .ok_or_else(|| Error::Invalid("Unknown entry".into()))?;
    let row = snapshot
        .report
        .rows
        .iter()
        .find(|r| r.id == id)
        .ok_or_else(|| Error::Invalid("Unknown entry".into()))?;
    let parts = relative
        .components()
        .map(|p| match p {
            std::path::Component::Normal(name) => name.to_str().ok_or_else(|| {
                Error::Invalid("Non-UTF-8 entry cannot be represented by a text exclusion".into())
            }),
            _ => Err(Error::Invalid("Invalid relative path".into())),
        })
        .collect::<Result<Vec<_>>>()?;
    if parts.iter().any(|p| p.contains([';', '\n', '\r'])) {
        return Err(Error::Invalid(
            "This name contains an exclusion separator; add a custom pattern instead.".into(),
        ));
    }
    let pattern = parts
        .into_iter()
        .map(|p| globset::escape(p).replace(' ', "[ ]"))
        .collect::<Vec<_>>()
        .join("/");
    let directory =
        crate::tree::is_directory(row) && row.status != clear_contracts::Status::TypeConflict;
    let rule = format!("-./{pattern}{}", if directory { "/" } else { "" });
    let mut rules = current
        .split([';', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if !rules.contains(&rule) {
        rules.push(rule);
    }
    let result = rules.join(";\n");
    Rules::parse(&result)?;
    Ok(result)
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
