//! File-scoped immutable history and porcelain blame, with native rename paths.
use super::*;
use std::io::{Read, Seek};
#[derive(Clone)]
struct Target {
    commit: String,
    path: PathBuf,
    line: usize,
}
struct Record {
    view: FileHistoryEntry,
    path: PathBuf,
}
struct Document {
    target: Target,
    lines: Vec<Vec<u8>>,
}
pub(super) struct CapturedHistory {
    repository: String,
    pub(super) source_session: Option<String>,
    anchor: Target,
    status: String,
    shallow: bool,
    graph: String,
    truncated: bool,
    entries: Vec<Record>,
    line_targets: BTreeMap<String, Target>,
    documents: BTreeMap<String, Document>,
}
fn invalid(message: &str) -> Error {
    Error::Invalid(message.into())
}
fn oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64)
        && value.bytes().all(|b| b.is_ascii_hexdigit())
        && value.bytes().any(|b| b != b'0')
}
fn path_text(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| invalid("File history currently requires Unicode paths"))
}
fn record_path(raw: &[u8]) -> Result<PathBuf> {
    crate::git::path_from_bytes(raw)
}
fn graph_fingerprint(workspace: &Workspace, repository: &str) -> Result<String> {
    let repo = workspace
        .repositories
        .get(repository)
        .ok_or_else(|| invalid("Unknown file history repository"))?;
    let mut evidence = Vec::new();
    for relative in ["shallow", "info/grafts"] {
        let relative = Path::new(relative);
        let path = crate::compare::checked_path(&repo.common, relative, true)?;
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_file() && !meta.file_type().is_symlink() => {
                let mut bytes = Vec::new();
                fs::File::open(&path)?
                    .take(1024 * 1024 + 1)
                    .read_to_end(&mut bytes)?;
                if bytes.len() > 1024 * 1024 {
                    return Err(invalid("File history graph metadata exceeds 1 MiB"));
                }
                evidence.push((relative.to_path_buf(), Some(id(&bytes))));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                evidence.push((relative.to_path_buf(), None))
            }
            Ok(_) => return Err(invalid("File history graph metadata is not a regular file")),
            Err(error) => return Err(error.into()),
        }
    }
    Ok(id(&serde_json::to_vec(&evidence)?))
}
fn check_graph(workspace: &Workspace, repository: &str, expected: &str) -> Result<()> {
    if graph_fingerprint(workspace, repository)? != expected {
        return Err(invalid(
            "File history graph changed; reopen the file history",
        ));
    }
    Ok(())
}
fn parse_history(raw: &[u8], initial: &Path, cancel: &AtomicBool) -> Result<Vec<Record>> {
    let fields = raw.split(|b| *b == 0).collect::<Vec<_>>();
    let mut at = 0;
    let mut current = initial.to_path_buf();
    let mut entries = Vec::new();
    while at < fields.len() {
        check_cancel(cancel)?;
        while at < fields.len() && fields[at].is_empty() {
            at += 1;
        }
        if at == fields.len() {
            break;
        }
        if at + 6 > fields.len() {
            return Err(invalid("Malformed file history metadata"));
        }
        let values = &fields[at..at + 6];
        at += 6;
        let text = |i: usize| String::from_utf8_lossy(values[i]).into_owned();
        let commit = text(0);
        if !oid(&commit) {
            return Err(invalid("Invalid commit identity in file history"));
        }
        let parents = text(1)
            .split_whitespace()
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if parents.iter().any(|parent| !oid(parent)) {
            return Err(invalid("Invalid parent identity in file history"));
        }
        let mut path = current.clone();
        let mut previous = None;
        let mut deleted = false;
        while at < fields.len() {
            while at < fields.len() && fields[at].is_empty() {
                at += 1;
            }
            if at == fields.len() {
                break;
            }
            let status = fields[at].strip_prefix(b"\n").unwrap_or(fields[at]);
            if oid(&String::from_utf8_lossy(status)) {
                break;
            }
            if status.is_empty() || !b"AMDRCTUXB".contains(&status[0]) {
                return Err(invalid("Malformed file history change record"));
            }
            at += 1;
            let first = record_path(
                fields
                    .get(at)
                    .ok_or_else(|| invalid("Missing file history path"))?,
            )?;
            at += 1;
            if matches!(status[0], b'R' | b'C') {
                let next = record_path(
                    fields
                        .get(at)
                        .ok_or_else(|| invalid("Missing renamed file history path"))?,
                )?;
                at += 1;
                if next == current {
                    path = next;
                    previous = Some(first.clone());
                    current = first;
                }
            } else if first == current {
                path = first;
                deleted = status[0] == b'D';
            }
        }
        entries.push(Record {
            view: FileHistoryEntry {
                id: crate::unique_id(),
                commit: Commit {
                    id: commit,
                    parents,
                    author: text(2),
                    email: text(3),
                    date: text(4),
                    subject: text(5),
                    decorations: String::new(),
                },
                path: crate::tree::display_path(&path),
                previous_path: previous.as_deref().map(crate::tree::display_path),
                deleted,
            },
            path,
        });
        if entries.len() > 5001 {
            return Err(invalid("File history exceeds its commit budget"));
        }
    }
    Ok(entries)
}
fn c_path(raw: &[u8]) -> Result<PathBuf> {
    if !raw.starts_with(b"\"") {
        return record_path(raw);
    }
    if !raw.ends_with(b"\"") || raw.len() < 2 {
        return Err(invalid("Malformed quoted blame filename"));
    }
    let raw = &raw[1..raw.len() - 1];
    let mut bytes = Vec::new();
    let mut at = 0;
    while at < raw.len() {
        let byte = raw[at];
        at += 1;
        if byte != b'\\' {
            bytes.push(byte);
            continue;
        }
        let next = *raw
            .get(at)
            .ok_or_else(|| invalid("Incomplete blame path escape"))?;
        at += 1;
        bytes.push(match next {
            b'a' => 7,
            b'b' => 8,
            b't' => 9,
            b'n' => 10,
            b'v' => 11,
            b'f' => 12,
            b'r' => 13,
            b'\\' => b'\\',
            b'"' => b'"',
            b'0'..=b'3' => {
                let digits = raw
                    .get(at..at + 2)
                    .ok_or_else(|| invalid("Incomplete blame octal path escape"))?;
                if digits.iter().any(|b| !(b'0'..=b'7').contains(b)) {
                    return Err(invalid("Invalid blame octal path escape"));
                }
                at += 2;
                (next - b'0') * 64 + (digits[0] - b'0') * 8 + (digits[1] - b'0')
            }
            _ => return Err(invalid("Unsupported blame path escape")),
        });
    }
    record_path(&bytes)
}
fn blob_at(
    root: &Path,
    target: &Target,
    cancel: &AtomicBool,
) -> Result<Option<(String, u64, String)>> {
    let path = path_text(&target.path)?;
    let raw = runner::run(
        root,
        &[
            "ls-tree",
            "--full-tree",
            "-z",
            "-l",
            &target.commit,
            "--",
            &path,
        ],
        cancel,
    )?;
    if raw.is_empty() {
        return Ok(None);
    }
    let records = raw
        .split(|b| *b == 0)
        .filter(|r| !r.is_empty())
        .collect::<Vec<_>>();
    if records.len() != 1 || raw.last() != Some(&0) {
        return Err(invalid("Historical file lookup is ambiguous"));
    }
    let record = records[0];
    let tab = record
        .iter()
        .position(|b| *b == b'\t')
        .ok_or_else(|| invalid("Malformed historical file lookup"))?;
    if record_path(&record[tab + 1..])? != target.path {
        return Err(invalid("Historical file lookup returned a different path"));
    }
    let fields = std::str::from_utf8(&record[..tab])
        .map_err(|_| invalid("Invalid historical file header"))?
        .split_ascii_whitespace()
        .collect::<Vec<_>>();
    if fields.len() != 4 || !oid(fields[2]) {
        return Err(invalid("Invalid historical file object"));
    }
    let size = fields[3].parse::<u64>().unwrap_or(u64::MAX);
    Ok(Some((fields[2].into(), size, fields[0].into())))
}
impl CapturedHistory {
    fn target(&self, target: &TraceTarget) -> Result<Target> {
        match target {
            TraceTarget::Anchor => Ok(self.anchor.clone()),
            TraceTarget::Commit { id } => {
                let entry = self
                    .entries
                    .iter()
                    .find(|entry| entry.view.id == *id)
                    .ok_or_else(|| invalid("Unknown file history commit"))?;
                if entry.view.deleted {
                    return Err(invalid(
                        "File is deleted at this commit; select its parent or an earlier version",
                    ));
                }
                Ok(Target {
                    commit: entry.view.commit.id.clone(),
                    path: entry.path.clone(),
                    line: 1,
                })
            }
            TraceTarget::Line { id } => self
                .line_targets
                .get(id)
                .cloned()
                .ok_or_else(|| invalid("Blame line selection expired; read the page again")),
        }
    }
    fn page(&self, id: &str, offset: usize) -> Result<FileHistoryPage> {
        if offset > self.entries.len() {
            return Err(invalid("Invalid file history page"));
        }
        Ok(FileHistoryPage {
            id: id.into(),
            repository_id: self.repository.clone(),
            anchor: self.anchor.commit.clone(),
            path: crate::tree::display_path(&self.anchor.path),
            status: self.status.clone(),
            shallow: self.shallow,
            truncated: self.truncated,
            offset,
            total: self.entries.len(),
            entries: self
                .entries
                .iter()
                .skip(offset)
                .take(100)
                .map(|entry| entry.view.clone())
                .collect(),
        })
    }
}
impl Workspace {
    pub fn file_history_open(
        &mut self,
        source: FileHistorySource,
        cancel: &AtomicBool,
    ) -> Result<FileHistoryPage> {
        let (repository, commit, path, source_session) = match source {
            FileHistorySource::Source {
                repository_id,
                file_id,
            } => {
                let cap = self.file_cap(&repository_id, &file_id, None)?;
                if !matches!(cap.kind.as_str(), "file" | "link") {
                    return Err(invalid("Select a source file to inspect its history"));
                }
                let root = revision::root(self, &repository_id, cancel)?;
                let output = runner::optional(
                    &root,
                    &[
                        "rev-parse".into(),
                        "--verify".into(),
                        "--quiet".into(),
                        "HEAD^{commit}".into(),
                    ],
                    &[],
                    cancel,
                )?;
                let commit = String::from_utf8_lossy(&output).trim().to_string();
                (repository_id, commit, cap.relative, None)
            }
            FileHistorySource::Revision {
                repository_id,
                session,
                file_id,
            } => {
                let (commit, path) =
                    self.revision_file_selection(&repository_id, &session, &file_id)?;
                (repository_id, commit, path, Some(session))
            }
        };
        let root = revision::root(self, &repository, cancel)?;
        let target = Target {
            commit,
            path,
            line: 1,
        };
        let path = path_text(&target.path)?;
        let status = if target.commit.is_empty() {
            "unborn"
        } else if blob_at(&root, &target, cancel)?.is_none() {
            "untracked_at_revision"
        } else {
            "tracked"
        };
        let graph = graph_fingerprint(self, &repository)?;
        let shallow =
            runner::text(&root, &["rev-parse", "--is-shallow-repository"], cancel)? == "true";
        let mut entries = if status == "tracked" {
            let raw = runner::run(
                &root,
                &[
                    "log",
                    "--follow",
                    "-M",
                    "--name-status",
                    "-z",
                    "--no-color",
                    "--no-ext-diff",
                    "--no-textconv",
                    "--max-count=5001",
                    "--format=%H%x00%P%x00%an%x00%ae%x00%aI%x00%s%x00",
                    &target.commit,
                    "--",
                    &path,
                ],
                cancel,
            )?;
            parse_history(&raw, &target.path, cancel)?
        } else {
            Vec::new()
        };
        let truncated = entries.len() > 5000;
        entries.truncate(5000);
        check_graph(self, &repository, &graph)?;
        let history = CapturedHistory {
            repository,
            source_session,
            anchor: target,
            status: status.into(),
            shallow,
            graph,
            truncated,
            entries,
            line_targets: BTreeMap::new(),
            documents: BTreeMap::new(),
        };
        let id = crate::unique_id();
        let page = history.page(&id, 0)?;
        if self.file_histories.len() >= 4 {
            self.file_histories.clear();
        }
        self.file_histories.insert(id, history);
        Ok(page)
    }
    pub fn file_history_close(&mut self, history_id: &str) -> Result<()> {
        self.file_histories.remove(history_id);
        Ok(())
    }
    pub fn file_history_page(
        &self,
        history_id: &str,
        offset: usize,
        cancel: &AtomicBool,
    ) -> Result<FileHistoryPage> {
        let history = self
            .file_histories
            .get(history_id)
            .ok_or_else(|| invalid("File history expired; open it again"))?;
        revision::root(self, &history.repository, cancel)?;
        check_graph(self, &history.repository, &history.graph)?;
        history.page(history_id, offset)
    }
    pub fn file_history_version(
        &mut self,
        history_id: &str,
        target: &TraceTarget,
        cancel: &AtomicBool,
    ) -> Result<RevisionFocus> {
        let history = self
            .file_histories
            .get(history_id)
            .ok_or_else(|| invalid("File history expired; open it again"))?;
        let repository = history.repository.clone();
        let graph = history.graph.clone();
        check_graph(self, &repository, &graph)?;
        let target = history.target(target)?;
        if target.commit.is_empty() {
            return Err(invalid("File has no committed version"));
        }
        let focused = self.revision_focus(
            &repository,
            &target.commit,
            &target.path,
            target.line,
            cancel,
        )?;
        check_graph(self, &repository, &graph)?;
        Ok(focused)
    }
    pub fn file_blame(
        &mut self,
        history_id: &str,
        target: &TraceTarget,
        offset: usize,
        cancel: &AtomicBool,
    ) -> Result<FileBlamePage> {
        let history = self
            .file_histories
            .get(history_id)
            .ok_or_else(|| invalid("File history expired; open it again"))?;
        let repository = history.repository.clone();
        let graph = history.graph.clone();
        check_graph(self, &repository, &graph)?;
        let target = history.target(target)?;
        if target.commit.is_empty() {
            return Err(invalid("File has no committed version"));
        }
        let root = revision::root(self, &repository, cancel)?;
        let key = id(&serde_json::to_vec(&(
            &target.commit,
            crate::native_path::encode(&target.path),
        ))?);
        if !self.file_histories[history_id].documents.contains_key(&key) {
            let (object, size, mode) = blob_at(&root, &target, cancel)?
                .ok_or_else(|| invalid("File is absent from this historical commit"))?;
            if !matches!(mode.as_str(), "100644" | "100755") {
                return Err(invalid("Line blame requires a regular committed text file"));
            }
            if size > crate::editing::MAX_DOCUMENT_BYTES as u64 {
                return Err(invalid("Line blame exceeds 4 MiB"));
            }
            let mut file = runner::blob(&root, &object, size, cancel)?;
            revision::verify_blob(file.as_file_mut(), &object, size, cancel)?;
            let mut bytes = Vec::new();
            file.as_file_mut().rewind()?;
            file.read_to_end(&mut bytes)?;
            if bytes.contains(&0) || std::str::from_utf8(&bytes).is_err() {
                return Err(invalid(
                    "Line blame requires UTF-8 text; use file history to view other formats",
                ));
            }
            let count = bytes.iter().filter(|b| **b == b'\n').count()
                + usize::from(!bytes.is_empty() && !bytes.ends_with(b"\n"));
            if count > 100_000 {
                return Err(invalid("Line blame exceeds 100000 physical lines"));
            }
            let mut lines = bytes
                .split_inclusive(|b| *b == b'\n')
                .map(|line| line.strip_suffix(b"\n").unwrap_or(line).to_vec())
                .collect::<Vec<_>>();
            if lines.len() > 100_000 {
                return Err(invalid("Line blame exceeds 100000 physical lines"));
            }
            if bytes.is_empty() {
                lines.clear();
            }
            let history = self.file_histories.get_mut(history_id).unwrap();
            if history.documents.len() >= 2 {
                history.documents.clear();
            }
            history.documents.insert(
                key.clone(),
                Document {
                    target: target.clone(),
                    lines,
                },
            );
        }
        let doc = &self.file_histories[history_id].documents[&key];
        let total = doc.lines.len();
        if offset > total {
            return Err(invalid("Invalid blame page"));
        }
        let end = (offset + 200).min(total);
        let lines = if offset == end {
            Vec::new()
        } else {
            let raw = runner::run(
                &root,
                &[
                    "-c",
                    "core.quotePath=false",
                    "blame",
                    "--line-porcelain",
                    "--no-progress",
                    "--encoding=UTF-8",
                    "-L",
                    &format!("{},{}", offset + 1, end),
                    &doc.target.commit,
                    "--",
                    &path_text(&doc.target.path)?,
                ],
                cancel,
            )?;
            parse_blame(&raw, &doc.lines, offset, end, cancel)?
        };
        check_graph(self, &repository, &graph)?;
        let mut targets = Vec::new();
        let mut views = Vec::new();
        for (view, path) in lines {
            targets.push((
                view.id.clone(),
                Target {
                    commit: view.commit.clone(),
                    path,
                    line: view.original_line,
                },
            ));
            views.push(view);
        }
        let history = self.file_histories.get_mut(history_id).unwrap();
        if history.line_targets.len() + targets.len() > 4000 {
            history.line_targets.clear();
        }
        history.line_targets.extend(targets);
        Ok(FileBlamePage {
            history_id: history_id.into(),
            commit: target.commit,
            path: crate::tree::display_path(&target.path),
            offset,
            total,
            lines: views,
            next_offset: (end < total).then_some(end),
        })
    }
}
fn parse_blame(
    raw: &[u8],
    expected: &[Vec<u8>],
    start: usize,
    end: usize,
    cancel: &AtomicBool,
) -> Result<Vec<(BlameLine, PathBuf)>> {
    let records = raw.split(|b| *b == b'\n').collect::<Vec<_>>();
    let mut at = 0;
    let mut rows = Vec::new();
    while at < records.len() {
        check_cancel(cancel)?;
        if records[at].is_empty() {
            at += 1;
            continue;
        }
        let header = std::str::from_utf8(records[at])
            .map_err(|_| invalid("Invalid blame header"))?
            .split_whitespace()
            .collect::<Vec<_>>();
        at += 1;
        if !(3..=4).contains(&header.len()) || !oid(header[0]) {
            return Err(invalid("Invalid blame commit identity"));
        }
        let original_line = header[1]
            .parse::<usize>()
            .map_err(|_| invalid("Invalid original blame line"))?;
        let line = header[2]
            .parse::<usize>()
            .map_err(|_| invalid("Invalid final blame line"))?;
        if original_line == 0 || line != start + rows.len() + 1 || line > end {
            return Err(invalid("Blame line order differs from the requested file"));
        }
        let mut fields = BTreeMap::new();
        let mut boundary = false;
        let mut path = None;
        let mut body = None;
        while at < records.len() {
            let record = records[at];
            at += 1;
            if let Some(text) = record.strip_prefix(b"\t") {
                body = Some(text);
                break;
            }
            if record == b"boundary" {
                boundary = true;
            } else if let Some(value) = record.strip_prefix(b"filename ") {
                path = Some(c_path(value)?);
            } else if let Some(space) = record.iter().position(|b| *b == b' ') {
                fields.insert(
                    String::from_utf8_lossy(&record[..space]).into_owned(),
                    String::from_utf8_lossy(&record[space + 1..]).into_owned(),
                );
            }
        }
        let body = body.ok_or_else(|| invalid("Missing blame source line"))?;
        if body != expected[line - 1] {
            return Err(invalid(
                "Blame contents differ from the captured Git object",
            ));
        }
        let field = |key: &str| {
            fields
                .get(key)
                .cloned()
                .ok_or_else(|| invalid("Incomplete blame metadata"))
        };
        let timestamp = field("author-time")?;
        timestamp
            .parse::<i64>()
            .map_err(|_| invalid("Invalid blame author timestamp"))?;
        let timezone = field("author-tz")?;
        if timezone.len() != 5
            || !timezone.starts_with(['+', '-'])
            || !timezone[1..].bytes().all(|b| b.is_ascii_digit())
        {
            return Err(invalid("Invalid blame author timezone"));
        }
        let path = path.ok_or_else(|| invalid("Missing blame original filename"))?;
        let text = std::str::from_utf8(body)
            .map_err(|_| invalid("Invalid UTF-8 blame text"))?
            .strip_suffix('\r')
            .unwrap_or(std::str::from_utf8(body).unwrap());
        let text = if line == 1 {
            text.strip_prefix('\u{feff}').unwrap_or(text)
        } else {
            text
        };
        rows.push((
            BlameLine {
                id: crate::unique_id(),
                commit: header[0].into(),
                original_line,
                line,
                author: field("author")?,
                email: field("author-mail")?,
                timestamp,
                timezone,
                subject: field("summary")?,
                original_path: crate::tree::display_path(&path),
                boundary,
                text: text.chars().take(8192).collect(),
                long_line: text.chars().count() > 8192,
            },
            path,
        ));
    }
    if rows.len() != end - start {
        return Err(invalid("Blame response is incomplete"));
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn c_quoted_paths_decode_utf8_octal_without_arithmetic_overflow() {
        assert_eq!(
            c_path(br#""\344\270\255.txt""#).unwrap(),
            Path::new("中.txt")
        );
        assert!(c_path(br#""\777""#).is_err());
        assert!(c_path(br#""bad\q""#).is_err());
        assert!(c_path(br#""../outside""#).is_err());
    }
}
