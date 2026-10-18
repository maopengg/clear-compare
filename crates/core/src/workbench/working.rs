//! Captured working/index differences and scoped cached patch selections.
use super::*;
use crate::compare::checked_path;
#[derive(Clone)]
pub(super) struct SelectionGuard {
    pub args: Vec<String>,
    pub digest: String,
}
impl SelectionGuard {
    pub fn check(&self, root: &Path, cancel: &AtomicBool) -> Result<()> {
        if id(&runner::optional(root, &self.args, &[], cancel)?) != self.digest {
            return Err(invalid(
                "Selected file changed; refresh its diff before staging",
            ));
        }
        Ok(())
    }
}
struct Line {
    id: Option<String>,
    kind: char,
    text: String,
    suffix: String,
    before: Option<usize>,
    after: Option<usize>,
}
struct Hunk {
    id: String,
    before: usize,
    after: usize,
    lines: Vec<Line>,
}
pub(super) struct CapturedDiff {
    repository: String,
    file: String,
    revision: String,
    path: String,
    area: String,
    raw: String,
    guard: SelectionGuard,
    headers: Vec<String>,
    hunks: Vec<Hunk>,
    partial: bool,
    reason: Option<String>,
    token: String,
}
fn invalid(message: &str) -> Error {
    Error::Invalid(message.into())
}
fn start(value: &str, prefix: char) -> Result<usize> {
    value
        .strip_prefix(prefix)
        .and_then(|s| s.split(',').next())
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| invalid("Malformed Git hunk range"))
}
fn read_args(
    paths: &[PathBuf],
    area: &str,
    untracked: bool,
    has_head: bool,
) -> Result<Vec<String>> {
    if !matches!(area, "worktree" | "index" | "head") {
        return Err(invalid("Unknown Git difference area"));
    }
    let mut args = vec![
        "-c".into(),
        "core.quotePath=true".into(),
        "diff".into(),
        "--no-ext-diff".into(),
        "--no-textconv".into(),
        "--no-color".into(),
        "--full-index".into(),
        "--src-prefix=a/".into(),
        "--dst-prefix=b/".into(),
        "--unified=3".into(),
    ];
    if untracked || (area == "head" && !has_head) {
        if area == "index" {
            return Err(invalid("Untracked files are not in the index"));
        }
        args.extend(["--no-index".into(), "--".into(), "/dev/null".into()]);
        args.push(
            paths[0]
                .to_str()
                .ok_or_else(|| invalid("Git diff currently requires a Unicode path"))?
                .into(),
        );
    } else {
        if area == "index" {
            args.push("--cached".into());
        } else if area == "head" {
            args.push("HEAD".into());
        }
        args.push("--".into());
        for path in paths {
            args.push(
                path.to_str()
                    .ok_or_else(|| invalid("Git diff currently requires Unicode paths"))?
                    .into(),
            );
        }
    }
    Ok(args)
}
fn parse(
    repository: &str,
    file: &str,
    state: &RepositoryState,
    area: &str,
    args: Vec<String>,
    bytes: Vec<u8>,
) -> Result<CapturedDiff> {
    if bytes.len() > 4 * 1024 * 1024 {
        return Err(invalid(
            "Selected Git diff exceeds 4 MiB; use whole-file staging or an external viewer",
        ));
    }
    let raw = String::from_utf8_lossy(&bytes).into_owned();
    let change = state
        .changes
        .iter()
        .find(|c| c.id == file)
        .ok_or_else(|| invalid("Changed file selection expired"))?;
    let token = crate::unique_id();
    let mut headers = Vec::new();
    let mut hunks = Vec::<Hunk>::new();
    let mut line_id = 0usize;
    let mut old_line = 0;
    let mut new_line = 0;
    let mut reason = if change.conflicted {
        Some("conflict")
    } else if change.original_path.is_some() {
        Some("rename")
    } else if area == "head" {
        Some("head")
    } else {
        None
    };
    if bytes.contains(&0)
        || std::str::from_utf8(&bytes).is_err()
        || raw
            .lines()
            .any(|l| l.starts_with("Binary files ") || l == "GIT binary patch")
    {
        reason = Some("binary");
    }
    if raw.lines().any(|l| {
        (l.starts_with("index ") && (l.ends_with(" 120000") || l.ends_with(" 160000")))
            || l.ends_with("mode 120000")
            || l.ends_with("mode 160000")
    }) {
        reason = Some("unsupported_mode");
    }
    if raw.lines().filter(|l| l.starts_with("diff --git ")).count() > 1 {
        reason = Some("rename");
    }
    if reason.is_none() || reason == Some("head") {
        for line in raw.split_inclusive('\n') {
            if line.starts_with("@@ ") {
                let fields = line.split_whitespace().collect::<Vec<_>>();
                if fields.len() < 4 {
                    return Err(invalid("Malformed Git hunk"));
                }
                old_line = start(fields[1], '-')?;
                new_line = start(fields[2], '+')?;
                hunks.push(Hunk {
                    id: format!("{token}-h{}", hunks.len()),
                    before: start(fields[1], '-')?,
                    after: start(fields[2], '+')?,
                    lines: Vec::new(),
                });
            } else if let Some(hunk) = hunks.last_mut() {
                if line.starts_with("\\ No newline") {
                    if let Some(last) = hunk.lines.last_mut() {
                        last.suffix.push_str(line);
                    }
                    continue;
                }
                let kind = line.chars().next().unwrap_or(' ');
                if !matches!(kind, ' ' | '-' | '+') {
                    return Err(invalid("Unsupported Git patch body"));
                }
                let before = if kind == '+' { None } else { Some(old_line) };
                let after = if kind == '-' { None } else { Some(new_line) };
                if kind != '+' {
                    old_line += 1;
                }
                if kind != '-' {
                    new_line += 1;
                }
                hunk.lines.push(Line {
                    id: if kind == ' ' {
                        None
                    } else {
                        Some(format!("{token}-l{line_id}"))
                    },
                    kind,
                    text: line[1..].into(),
                    suffix: String::new(),
                    before,
                    after,
                });
                line_id += 1;
                if line_id > 50_000 {
                    return Err(invalid("Selected diff exceeds 50000 lines"));
                }
            } else {
                headers.push(line.into());
            }
        }
    }
    if reason.is_none() && hunks.is_empty() {
        reason = Some("metadata");
    }
    Ok(CapturedDiff {
        repository: repository.into(),
        file: file.into(),
        revision: state.revision.clone(),
        path: change.path.clone(),
        area: area.into(),
        guard: SelectionGuard {
            args,
            digest: id(&bytes),
        },
        headers,
        hunks,
        partial: reason.is_none(),
        reason: reason.map(String::from),
        token,
        raw,
    })
}
fn ordered_lines(hunk: &Hunk) -> Vec<&Line> {
    let mut ordered = Vec::with_capacity(hunk.lines.len());
    let mut position = 0;
    while position < hunk.lines.len() {
        if hunk.lines[position].kind == ' ' {
            ordered.push(&hunk.lines[position]);
            position += 1;
            continue;
        }
        let start = position;
        while position < hunk.lines.len() && hunk.lines[position].kind != ' ' {
            position += 1;
        }
        let removed = hunk.lines[start..position]
            .iter()
            .filter(|l| l.kind == '-')
            .collect::<Vec<_>>();
        let added = hunk.lines[start..position]
            .iter()
            .filter(|l| l.kind == '+')
            .collect::<Vec<_>>();
        for i in 0..removed.len().max(added.len()) {
            if let Some(line) = removed.get(i) {
                ordered.push(*line);
            }
            if let Some(line) = added.get(i) {
                ordered.push(*line);
            }
        }
    }
    ordered
}
impl CapturedDiff {
    fn page(&self, offset: usize) -> Result<WorkingDiff> {
        let total = if self.hunks.is_empty() {
            self.raw.lines().count()
        } else {
            self.hunks.iter().map(|h| h.lines.len()).sum()
        };
        if offset > total {
            return Err(invalid("Diff page is out of range"));
        }
        let mut hunks = Vec::new();
        let mut position = 0;
        for hunk in &self.hunks {
            let begin = offset.saturating_sub(position).min(hunk.lines.len());
            let end = (offset + 500)
                .saturating_sub(position)
                .min(hunk.lines.len());
            if begin < end {
                hunks.push(WorkingHunk {
                    id: hunk.id.clone(),
                    before: hunk.before,
                    after: hunk.after,
                    continued: begin != 0,
                    lines: hunk.lines[begin..end]
                        .iter()
                        .map(|l| WorkingLine {
                            id: l.id.clone(),
                            kind: match l.kind {
                                '-' => "delete",
                                '+' => "add",
                                _ => "context",
                            }
                            .into(),
                            before: l.before,
                            after: l.after,
                            text: l
                                .text
                                .trim_end_matches(['\r', '\n'])
                                .chars()
                                .take(8192)
                                .collect(),
                            no_newline: !l.suffix.is_empty(),
                            truncated: l.text.chars().count() > 8192,
                        })
                        .collect(),
                });
            }
            position += hunk.lines.len();
        }
        let raw = if self.hunks.is_empty() {
            self.raw
                .lines()
                .skip(offset)
                .take(500)
                .map(|l| l.chars().take(8192).collect::<String>())
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            String::new()
        };
        Ok(WorkingDiff {
            id: self.token.clone(),
            repository_id: self.repository.clone(),
            file_id: self.file.clone(),
            revision: self.revision.clone(),
            path: self.path.clone(),
            area: self.area.clone(),
            partial: self.partial,
            reason: self.reason.clone(),
            hunks,
            raw,
            offset,
            total_lines: total,
            next_offset: if offset + 500 < total {
                Some(offset + 500)
            } else {
                None
            },
        })
    }
}
fn opposite(label: &str, from: &str, to: &str) -> Result<String> {
    if let Some(rest) = label.strip_prefix(from) {
        Ok(format!("{to}{rest}"))
    } else if let Some(rest) = label.strip_prefix(&format!("\"{from}")) {
        Ok(format!("\"{to}{rest}"))
    } else {
        Err(invalid("Unsupported Git patch filename"))
    }
}
impl Workspace {
    pub fn change_file(
        &mut self,
        repository: &str,
        file: &str,
        cancel: &AtomicBool,
    ) -> Result<WorkspaceFile> {
        self.open(repository, cancel)?;
        let repo = self
            .repositories
            .get(repository)
            .ok_or_else(|| invalid("Unknown repository"))?;
        let relative = repo
            .files
            .get(file)
            .and_then(|p| p.first())
            .cloned()
            .ok_or_else(|| invalid("Changed file selection expired"))?;
        let root = repo.root.clone();
        let path = checked_path(&root, &relative, false)?;
        if !fs::symlink_metadata(path)?.is_file() {
            return Err(invalid(
                "Open this folder or special entry from the project file tree",
            ));
        }
        self.capture_file(repository, &root, relative)
    }
    pub fn working_diff(
        &mut self,
        repository: &str,
        file: &str,
        area: &str,
        token: Option<&str>,
        offset: usize,
        cancel: &AtomicBool,
    ) -> Result<WorkingDiff> {
        if let Some(token) = token {
            let current = self.open(repository, cancel)?;
            let captured = self
                .working_diff
                .as_ref()
                .filter(|d| {
                    d.token == token
                        && d.repository == repository
                        && d.file == file
                        && d.area == area
                })
                .ok_or_else(|| invalid("Diff selection expired; open it again"))?;
            if current.revision != captured.revision {
                return Err(invalid("Repository changed; refresh this diff"));
            }
            return captured.page(offset);
        }
        let state = self.open(repository, cancel)?;
        let change = state
            .changes
            .iter()
            .find(|c| c.id == file)
            .ok_or_else(|| invalid("Changed file selection expired"))?;
        let repo = self
            .repositories
            .get(repository)
            .ok_or_else(|| invalid("Unknown repository"))?;
        let paths = repo
            .files
            .get(file)
            .ok_or_else(|| invalid("File selection expired"))?;
        if change.untracked {
            let path = checked_path(&repo.root, &paths[0], false)?;
            let meta = fs::symlink_metadata(path)?;
            if !meta.is_file() || meta.file_type().is_symlink() {
                return Err(invalid(
                    "Expand untracked folders in the file workspace before selecting a file",
                ));
            }
        }
        let args = read_args(
            paths,
            area,
            change.untracked,
            state.repository.head.is_some(),
        )?;
        let bytes = runner::optional(&repo.root, &args, &[], cancel)?;
        let captured = parse(repository, file, &state, area, args, bytes)?;
        let view = captured.page(offset)?;
        self.working_diff = Some(captured);
        Ok(view)
    }
    pub(super) fn prepare_selection(
        &self,
        repository: &str,
        revision: &str,
        diff_id: &str,
        hunks: &[String],
        lines: &[String],
        cancel: &AtomicBool,
    ) -> Result<(Vec<u8>, SelectionGuard, Option<String>)> {
        let captured = self
            .working_diff
            .as_ref()
            .filter(|d| {
                d.repository == repository
                    && d.token == diff_id
                    && d.revision == revision
                    && d.partial
            })
            .ok_or_else(|| invalid("Diff is stale or does not allow partial staging"))?;
        if hunks.len() > 50_000 || lines.len() > 50_000 || hunks.is_empty() && lines.is_empty() {
            return Err(invalid("Select hunks or changed lines"));
        }
        let selected_hunks = hunks.iter().collect::<BTreeSet<_>>();
        let selected_lines = lines.iter().collect::<BTreeSet<_>>();
        let valid_hunks = captured
            .hunks
            .iter()
            .map(|h| &h.id)
            .collect::<BTreeSet<_>>();
        let valid_lines = captured
            .hunks
            .iter()
            .flat_map(|h| h.lines.iter().filter_map(|l| l.id.as_ref()))
            .collect::<BTreeSet<_>>();
        if !selected_hunks.is_subset(&valid_hunks) || !selected_lines.is_subset(&valid_lines) {
            return Err(invalid("Selected hunk or line is not in this diff"));
        }
        let reverse = captured.area == "index";
        let mut body = String::new();
        let mut selected = 0;
        let mut changed = 0;
        for hunk in &captured.hunks {
            let mut text = String::new();
            let mut old_count = 0;
            let mut new_count = 0;
            let mut count = 0;
            for line in ordered_lines(hunk) {
                let choose = line.id.as_ref().is_some_and(|id| {
                    selected_hunks.contains(&hunk.id) || selected_lines.contains(id)
                });
                if line.kind != ' ' {
                    changed += 1;
                    if choose {
                        selected += 1;
                        count += 1;
                    }
                }
                let prefix = match (line.kind, choose, reverse) {
                    (' ', _, _) => Some(' '),
                    ('-', true, _) => Some('-'),
                    ('+', true, _) => Some('+'),
                    ('-', false, false) | ('+', false, true) => Some(' '),
                    _ => None,
                };
                if let Some(prefix) = prefix {
                    if prefix != '+' {
                        old_count += 1;
                    }
                    if prefix != '-' {
                        new_count += 1;
                    }
                    text.push(prefix);
                    text.push_str(&line.text);
                    text.push_str(&line.suffix);
                }
            }
            if count > 0 {
                body.push_str(&format!(
                    "@@ -{},{} +{},{} @@\n",
                    hunk.before, old_count, hunk.after, new_count
                ));
                body.push_str(&text);
            }
        }
        if selected == 0 {
            return Err(invalid("No changed lines selected"));
        }
        let partial = selected < changed;
        let regularize = partial
            && ((reverse
                && captured
                    .headers
                    .iter()
                    .any(|l| l.starts_with("new file mode ")))
                || (!reverse
                    && captured
                        .headers
                        .iter()
                        .any(|l| l.starts_with("deleted file mode "))));
        let mut header = String::new();
        for line in &captured.headers {
            if line.starts_with("old mode ") || line.starts_with("new mode ") {
                continue;
            }
            if regularize
                && (line.starts_with("new file mode ") || line.starts_with("deleted file mode "))
            {
                continue;
            }
            if regularize && line.starts_with("--- /dev/null") {
                let other = captured
                    .headers
                    .iter()
                    .find_map(|l| l.strip_prefix("+++ "))
                    .ok_or_else(|| invalid("Missing patch file label"))?;
                header.push_str(&format!("--- {}", opposite(other, "b/", "a/")?));
            } else if regularize && line.starts_with("+++ /dev/null") {
                let other = captured
                    .headers
                    .iter()
                    .find_map(|l| l.strip_prefix("--- "))
                    .ok_or_else(|| invalid("Missing patch file label"))?;
                header.push_str(&format!("+++ {}", opposite(other, "a/", "b/")?));
            } else {
                header.push_str(line);
            }
        }
        header.push_str(&body);
        let patch = header.into_bytes();
        let repo = self
            .repositories
            .get(repository)
            .ok_or_else(|| invalid("Unknown repository"))?;
        captured.guard.check(&repo.root, cancel)?;
        let mut args = vec![
            "apply".into(),
            "--cached".into(),
            "--check".into(),
            "--recount".into(),
            "--unidiff-zero".into(),
            "--whitespace=nowarn".into(),
        ];
        if reverse {
            args.push("--reverse".into());
        }
        runner::mutate_input(&repo.root, &args, &patch, cancel)?;
        let backup = captured
            .headers
            .iter()
            .find_map(|l| l.strip_prefix("index "))
            .and_then(|l| l.split_whitespace().next())
            .and_then(|s| s.split_once(".."))
            .map(|(old, new)| if reverse { new } else { old })
            .filter(|s| !s.bytes().all(|c| c == b'0'))
            .map(String::from);
        Ok((patch, captured.guard.clone(), backup))
    }
    pub(super) fn selection_reversed(&self, diff_id: &str) -> bool {
        self.working_diff
            .as_ref()
            .is_some_and(|d| d.token == diff_id && d.area == "index")
    }
}
