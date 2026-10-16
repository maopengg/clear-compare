//! Pinned history queries and opaque commit-file selections. No checkout or filters.
use super::*;

pub(super) struct CapturedDetail {
    repository: String,
    target: String,
    parent: Option<String>,
    files: BTreeMap<String, Vec<String>>,
}

fn invalid(message: &str) -> Error {
    Error::Invalid(message.into())
}
fn value(value: &str, limit: usize) -> Result<()> {
    if value.len() > limit || value.chars().any(char::is_control) {
        return Err(invalid("Invalid Git history filter"));
    }
    Ok(())
}
fn sha(root: &Path, reference: &str, cancel: &AtomicBool) -> Result<String> {
    value(reference, 2048)?;
    if reference.is_empty() || reference.starts_with('-') {
        return Err(invalid("Invalid revision"));
    }
    runner::text(
        root,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{reference}^{{commit}}"),
        ],
        cancel,
    )
}
fn parse_commits(raw: &[u8]) -> Result<Vec<Commit>> {
    let mut fields = raw.split(|b| *b == 0).collect::<Vec<_>>();
    if fields.last() == Some(&b"".as_slice()) {
        fields.pop();
    }
    if fields.len() % 7 != 0 {
        return Err(invalid("Malformed Git history output"));
    }
    fields
        .as_chunks::<7>()
        .0
        .iter()
        .map(|f| {
            let text = |index: usize| String::from_utf8_lossy(f[index]).to_string();
            let id = text(0);
            if !oid(&id) {
                return Err(invalid("Malformed commit identity"));
            }
            Ok(Commit {
                id,
                parents: text(1).split_whitespace().map(String::from).collect(),
                author: text(2),
                email: text(3),
                date: text(4),
                subject: text(5),
                decorations: text(6),
            })
        })
        .collect()
}
fn oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|b| b.is_ascii_hexdigit())
}
const FORMAT: &str = "--format=%H%x00%P%x00%an%x00%ae%x00%aI%x00%s%x00%D";
fn graph(commits: Vec<Commit>, filtered: bool) -> Result<Vec<GraphCommit>> {
    let mut lanes = Vec::<String>::new();
    let visible = commits
        .iter()
        .map(|c| c.id.clone())
        .collect::<BTreeSet<_>>();
    commits
        .into_iter()
        .map(|commit| {
            let column = match lanes.iter().position(|p| p == &commit.id) {
                Some(i) => i,
                None => {
                    let column = lanes.len();
                    lanes.push(commit.id.clone());
                    column
                }
            };
            let before = lanes.clone();
            lanes.remove(column);
            for parent in commit.parents.iter().rev() {
                if filtered && !visible.contains(parent) {
                    continue;
                }
                if !lanes.contains(parent) {
                    lanes.insert(column.min(lanes.len()), parent.clone());
                }
            }
            if lanes.len() > 512 {
                return Err(invalid(
                    "History graph exceeds 512 concurrent lanes; narrow references",
                ));
            }
            let mut edges = Vec::new();
            for (from, node) in before.iter().enumerate() {
                if node == &commit.id {
                    for parent in &commit.parents {
                        if let Some(to) = lanes.iter().position(|p| p == parent) {
                            edges.push(GraphEdge { from, to });
                        }
                    }
                } else if let Some(to) = lanes.iter().position(|p| p == node) {
                    edges.push(GraphEdge { from, to });
                }
            }
            Ok(GraphCommit {
                commit,
                column,
                width: before.len().max(lanes.len()).max(1),
                edges,
            })
        })
        .collect()
}
impl Workspace {
    pub fn history(
        &mut self,
        repository: &str,
        query: &HistoryQuery,
        cancel: &AtomicBool,
    ) -> Result<HistoryPage> {
        if query.offset > 20_000 {
            return Err(invalid(
                "History paging is limited to 20000 commits; narrow filters",
            ));
        }
        for filter in [&query.search, &query.author, &query.since, &query.until] {
            value(filter, 2048)?;
        }
        let state = self.open(repository, cancel)?;
        let repo = self
            .repositories
            .get(repository)
            .ok_or_else(|| invalid("Unknown repository"))?;
        let roots = if let Some(reference) = &query.reference {
            vec![sha(&repo.root, reference, cancel)?]
        } else {
            let mut roots = state
                .references
                .iter()
                .map(|r| r.target.clone())
                .collect::<BTreeSet<_>>();
            roots.extend(state.repository.head);
            roots.into_iter().collect::<Vec<_>>()
        };
        let snapshot = id(&serde_json::to_vec(&roots)?);
        if query.snapshot.as_ref().is_some_and(|s| s != &snapshot) {
            return Err(invalid("History references changed; refresh before paging"));
        }
        if roots.is_empty() {
            return Ok(HistoryPage {
                snapshot,
                offset: query.offset,
                has_more: false,
                commits: Vec::new(),
            });
        }
        let mut args = vec![
            "log".into(),
            "--topo-order".into(),
            "-z".into(),
            FORMAT.into(),
            format!("--max-count={}", query.offset + 101),
            "--fixed-strings".into(),
        ];
        if !query.author.is_empty() {
            args.push(format!("--author={}", query.author));
        }
        if !query.since.is_empty() {
            args.push(format!("--since={}", query.since));
        }
        if !query.until.is_empty() {
            args.push(format!("--until={}", query.until));
        }
        let search_hash = !query.search.is_empty()
            && query.search.len() >= 4
            && query.search.bytes().all(|b| b.is_ascii_hexdigit());
        let exact = if search_hash {
            sha(&repo.root, &query.search, cancel).ok()
        } else {
            None
        };
        if let Some(exact) = exact {
            args.push("--no-walk".into());
            args.push(exact);
        } else {
            if !query.search.is_empty() {
                args.push(format!("--grep={}", query.search));
            }
            args.extend(roots);
        }
        args.push("--".into());
        if let Some(path) = &query.path {
            value(path, 8192)?;
            if !path.is_empty() {
                if Path::new(path)
                    .components()
                    .any(|c| !matches!(c, std::path::Component::Normal(_)))
                {
                    return Err(invalid("History path must be relative to the repository"));
                }
                args.push(path.clone());
            }
        }
        let commits = parse_commits(&runner::run_owned(&repo.root, &args, cancel)?)?;
        let has_more = commits.len() > query.offset + 100;
        let filtered = !query.search.is_empty()
            || !query.author.is_empty()
            || !query.since.is_empty()
            || !query.until.is_empty()
            || query.path.as_ref().is_some_and(|p| !p.is_empty());
        let commits = graph(commits, filtered)?
            .into_iter()
            .skip(query.offset)
            .take(100)
            .collect();
        Ok(HistoryPage {
            snapshot,
            offset: query.offset,
            has_more,
            commits,
        })
    }
    pub fn commit_detail(
        &mut self,
        repository: &str,
        reference: &str,
        parent_index: usize,
        cancel: &AtomicBool,
    ) -> Result<CommitDetail> {
        self.open(repository, cancel)?;
        let repo = self
            .repositories
            .get(repository)
            .ok_or_else(|| invalid("Unknown repository"))?;
        let target = sha(&repo.root, reference, cancel)?;
        let mut commits = parse_commits(&runner::run(
            &repo.root,
            &["log", "-1", "-z", FORMAT, &target, "--"],
            cancel,
        )?)?;
        let commit = commits.pop().ok_or_else(|| invalid("Commit unavailable"))?;
        let parent = if commit.parents.is_empty() && parent_index == 0 {
            None
        } else {
            Some(
                commit
                    .parents
                    .get(parent_index)
                    .ok_or_else(|| invalid("Unknown merge parent"))?
                    .clone(),
            )
        };
        let message = runner::text(
            &repo.root,
            &["show", "--no-patch", "--format=%B", &target, "--"],
            cancel,
        )?;
        let mut args = vec![
            "diff-tree".into(),
            "--no-commit-id".into(),
            "--raw".into(),
            "-r".into(),
            "-z".into(),
            "--no-abbrev".into(),
            "--no-ext-diff".into(),
            "--no-textconv".into(),
            "-M".into(),
        ];
        if let Some(parent) = &parent {
            args.push(parent.clone());
        } else {
            args.push("--root".into());
        }
        args.extend([target.clone(), "--".into()]);
        let raw = runner::run_owned(&repo.root, &args, cancel)?;
        let mut records = raw.split(|b| *b == 0).filter(|r| !r.is_empty());
        let mut files = Vec::new();
        let mut captured = BTreeMap::new();
        while let Some(record) = records.next() {
            check_cancel(cancel)?;
            let fields = record.split(|b| *b == b' ').collect::<Vec<_>>();
            if fields.len() != 5 || !fields[0].starts_with(b":") {
                return Err(invalid("Malformed commit file record"));
            }
            let path = records
                .next()
                .ok_or_else(|| invalid("Missing commit path"))?;
            let status = String::from_utf8_lossy(fields[4]).to_string();
            let renamed = status.starts_with(['R', 'C']);
            let next = if renamed {
                records
                    .next()
                    .ok_or_else(|| invalid("Missing rename destination"))?
            } else {
                path
            };
            let display = String::from_utf8_lossy(next).to_string();
            let original = renamed.then(|| String::from_utf8_lossy(path).to_string());
            let file_id = id(&[record, b"\0", path, b"\0", next].concat());
            // Exact native byte paths are required for subsequent pathspecs.
            let paths = [path, next]
                .into_iter()
                .map(|p| {
                    std::str::from_utf8(p)
                        .map(String::from)
                        .map_err(|_| invalid("History diff requires a Unicode filename"))
                })
                .collect::<Result<Vec<_>>>()?;
            captured.insert(file_id.clone(), paths);
            files.push(CommitFile {
                id: file_id,
                path: display,
                original_path: original,
                status,
                old_mode: String::from_utf8_lossy(&fields[0][1..]).to_string(),
                new_mode: String::from_utf8_lossy(fields[1]).to_string(),
            });
            if files.len() > 50_000 {
                return Err(invalid("Commit contains more than 50000 files"));
            }
        }
        let id = crate::unique_id();
        if self.history_details.len() >= 8 {
            self.history_details.clear();
        }
        self.history_details.insert(
            id.clone(),
            CapturedDetail {
                repository: repository.into(),
                target,
                parent: parent.clone(),
                files: captured,
            },
        );
        Ok(CommitDetail {
            id,
            commit,
            message,
            parent,
            files,
        })
    }
    pub fn commit_patch(
        &self,
        repository: &str,
        detail_id: &str,
        file_id: &str,
        cancel: &AtomicBool,
    ) -> Result<GitPatch> {
        self.commit_patch_page(repository, detail_id, file_id, 0, cancel)
    }
    pub fn commit_patch_page(
        &self,
        repository: &str,
        detail_id: &str,
        file_id: &str,
        offset: usize,
        cancel: &AtomicBool,
    ) -> Result<GitPatch> {
        let detail = self
            .history_details
            .get(detail_id)
            .filter(|d| d.repository == repository)
            .ok_or_else(|| invalid("Commit selection expired; reopen details"))?;
        let paths = detail
            .files
            .get(file_id)
            .ok_or_else(|| invalid("Unknown commit file"))?;
        let repo = self
            .repositories
            .get(repository)
            .ok_or_else(|| invalid("Unknown repository"))?;
        let mut args = vec![
            "show".into(),
            "--format=".into(),
            "--no-ext-diff".into(),
            "--no-textconv".into(),
            "--no-color".into(),
            "--unified=3".into(),
        ];
        if let Some(parent) = &detail.parent {
            args[0] = "diff".into();
            args.remove(1);
            args.push(parent.clone());
        } else {
            args.push("--root".into());
        }
        args.extend([detail.target.clone(), "--".into()]);
        args.extend(paths.iter().cloned());
        let raw = runner::run_owned(&repo.root, &args, cancel)?;
        let text = String::from_utf8_lossy(&raw).to_string();
        let binary = text.lines().any(|l| l.starts_with("Binary files "));
        let total_lines = text.lines().count();
        if offset > total_lines {
            return Err(invalid("Patch page is out of range"));
        }
        let mut long_lines = false;
        let page = text
            .lines()
            .skip(offset)
            .take(500)
            .map(|line| {
                if line.chars().count() > 8192 {
                    long_lines = true;
                    format!("{}…", line.chars().take(8192).collect::<String>())
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>();
        let next = offset + page.len();
        Ok(GitPatch {
            path: paths.last().cloned().unwrap_or_default(),
            binary,
            text: page.join("\n"),
            offset,
            total_lines,
            next_offset: (next < total_lines).then_some(next),
            long_lines,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn filtered_history_does_not_allocate_lanes_for_hidden_parents() {
        let commits = (0..700)
            .map(|n| Commit {
                id: format!("{n:040x}"),
                parents: vec![format!("{:040x}", n + 1000)],
                author: String::new(),
                email: String::new(),
                date: String::new(),
                subject: String::new(),
                decorations: String::new(),
            })
            .collect();
        let graph = graph(commits, true).unwrap();
        assert_eq!(graph.len(), 700);
        assert!(
            graph
                .iter()
                .all(|c| c.width == 1 && c.edges.is_empty() && c.commit.parents.len() == 1)
        );
    }
}
