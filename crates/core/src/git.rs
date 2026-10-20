//! Read-only comparison of pinned commit trees and captured index records through bounded Git plumbing.
use crate::{
    Error, Result,
    compare::{canonical_root, check_cancel},
    rules::Rules,
    tree, unique_id,
};
use clear_contracts::{
    Comparison, Entry, GitCompareRequest, Kind, Page, Row, Status, Summary, TextDiff, TextOptions,
};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::AtomicBool,
    thread,
    time::{Duration, Instant},
};
mod worktree;
const MAX_LIST: u64 = 64 * 1024 * 1024;
const MAX_ENTRIES: usize = 200000;
fn command(repo: &Path, args: &[&str], limit: u64, cancel: &AtomicBool) -> Result<Vec<u8>> {
    run_command(repo, args, limit, cancel, false)
}
fn strict_command(repo: &Path, args: &[&str], limit: u64, cancel: &AtomicBool) -> Result<Vec<u8>> {
    run_command(repo, args, limit, cancel, true)
}
fn run_command(
    repo: &Path,
    args: &[&str],
    limit: u64,
    cancel: &AtomicBool,
    strict: bool,
) -> Result<Vec<u8>> {
    check_cancel(cancel)?;
    let mut cmd = Command::new("git");
    // Do not inherit repository redirection, trace destinations or caller-supplied config.
    for (name, _) in std::env::vars_os() {
        if name.to_str().is_some_and(|n| n.starts_with("GIT_")) {
            cmd.env_remove(name);
        }
    }
    cmd.current_dir(repo)
        .args([
            "--no-pager",
            "--no-optional-locks",
            "--no-replace-objects",
            "--no-lazy-fetch",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "advice.sparseIndexExpanded=false",
            "-c",
            "protocol.allow=never",
        ])
        .args(args)
        .env("GIT_NO_LAZY_FETCH", "1")
        .env("GIT_ALLOW_PROTOCOL", "")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_TRACE2", "0")
        .env("GIT_TRACE2_EVENT", "0")
        .env("GIT_TRACE2_PERF", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| {
        Error::Invalid(format!(
            "Cannot start Git. Install a Git version supporting --no-lazy-fetch: {e}"
        ))
    })?;
    let stdout = child.stdout.take().unwrap();
    let stderr = child.stderr.take().unwrap();
    let out = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let err = thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.take(65537).read_to_end(&mut bytes).map(|_| bytes)
    });
    let started = Instant::now();
    let status = loop {
        if let Err(e) = check_cancel(cancel) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = out.join();
            let _ = err.join();
            return Err(e);
        }
        if started.elapsed() > Duration::from_secs(15) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = out.join();
            let _ = err.join();
            return Err(Error::Invalid("Git command exceeded 15 seconds".into()));
        }
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => thread::sleep(Duration::from_millis(10)),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = out.join();
                let _ = err.join();
                return Err(e.into());
            }
        }
    };
    let output = out
        .join()
        .map_err(|_| Error::Invalid("Git stdout reader failed".into()))??;
    let errors = err
        .join()
        .map_err(|_| Error::Invalid("Git stderr reader failed".into()))??;
    if output.len() as u64 > limit || errors.len() > 65536 {
        return Err(Error::Invalid(
            "Git output exceeds the configured limit".into(),
        ));
    }
    if !status.success() || strict && !errors.is_empty() {
        return Err(Error::Invalid(format!(
            "Git failed: {}",
            String::from_utf8_lossy(&errors)
        )));
    }
    Ok(output)
}
fn valid_oid(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|b| b.is_ascii_hexdigit())
}
fn resolve(repo: &Path, revision: &str, cancel: &AtomicBool) -> Result<String> {
    if revision.is_empty() || revision.len() > 1024 || revision.chars().any(char::is_control) {
        return Err(Error::Invalid("Choose a valid Git commit revision".into()));
    }
    let expression = format!("{revision}^{{commit}}");
    let bytes = command(
        repo,
        &["rev-parse", "--verify", "--end-of-options", &expression],
        1024,
        cancel,
    )?;
    let oid = String::from_utf8(bytes)
        .map_err(|_| Error::Invalid("Invalid Git revision response".into()))?
        .trim()
        .to_owned();
    if !valid_oid(&oid) {
        return Err(Error::Invalid(
            "Git did not resolve one full commit ID".into(),
        ));
    }
    Ok(oid)
}

fn directory(repo: &Path, argument: &str, cancel: &AtomicBool) -> Result<PathBuf> {
    let output = command(
        repo,
        &["rev-parse", "--path-format=absolute", argument],
        32768,
        cancel,
    )?;
    let bytes = output.strip_suffix(b"\n").unwrap_or(&output);
    #[cfg(windows)]
    let bytes = bytes.strip_suffix(b"\r").unwrap_or(bytes);
    #[cfg(unix)]
    let path = {
        use std::os::unix::ffi::OsStringExt;
        PathBuf::from(std::ffi::OsString::from_vec(bytes.to_vec()))
    };
    #[cfg(not(unix))]
    let path = PathBuf::from(
        std::str::from_utf8(bytes)
            .map_err(|_| Error::Invalid("Git returned an unsupported repository path".into()))?,
    );
    let path = fs::canonicalize(path)?;
    if !path.is_dir() {
        return Err(Error::Invalid(
            "Git repository path is not a directory".into(),
        ));
    }
    Ok(path)
}
#[derive(Clone)]
struct GitEntry {
    working: Option<worktree::FileState>,
    oid: String,
    mode: String,
    entry: Entry,
}
pub(crate) fn path_from_bytes(bytes: &[u8]) -> Result<PathBuf> {
    if bytes.is_empty()
        || bytes.len() > 16384
        || bytes.starts_with(b"/")
        || bytes
            .split(|b| *b == b'/')
            .any(|c| c.is_empty() || c == b"." || c == b"..")
    {
        return Err(Error::Invalid("Invalid Git tree path".into()));
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStringExt;
        Ok(PathBuf::from(std::ffi::OsString::from_vec(bytes.to_vec())))
    }
    #[cfg(not(unix))]
    {
        let text = std::str::from_utf8(bytes).map_err(|_| {
            Error::Invalid("This Git tree has filenames unsupported on this OS".into())
        })?;
        if text.contains(['\\', ':']) {
            return Err(Error::Invalid(
                "This Git tree has filenames unsupported on this OS".into(),
            ));
        }
        Ok(PathBuf::from(text))
    }
}
fn load(
    repo: &Path,
    oid: &str,
    rules: &Rules,
    cancel: &AtomicBool,
) -> Result<BTreeMap<PathBuf, GitEntry>> {
    let output = command(
        repo,
        &["ls-tree", "--full-tree", "-r", "-t", "-z", "-l", oid, "--"],
        MAX_LIST,
        cancel,
    )?;
    if !output.is_empty() && output.last() != Some(&0) {
        return Err(Error::Invalid("Truncated Git tree response".into()));
    }
    let mut entries = BTreeMap::new();
    for record in output.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        check_cancel(cancel)?;
        if entries.len() >= MAX_ENTRIES {
            return Err(Error::Invalid("Git tree exceeds 200000 entries".into()));
        }
        let tab = record
            .iter()
            .position(|b| *b == b'\t')
            .ok_or_else(|| Error::Invalid("Malformed Git tree record".into()))?;
        let header = std::str::from_utf8(&record[..tab])
            .map_err(|_| Error::Invalid("Invalid Git tree header".into()))?
            .split_ascii_whitespace()
            .collect::<Vec<_>>();
        if header.len() != 4 || !valid_oid(header[2]) {
            return Err(Error::Invalid("Invalid Git tree object metadata".into()));
        }
        let kind = match (header[0], header[1]) {
            ("040000", "tree") => Kind::Directory,
            ("100644" | "100755", "blob") => Kind::File,
            ("120000", "blob") => Kind::Link,
            ("160000", "commit") => Kind::Other,
            _ => return Err(Error::Invalid("Unsupported Git tree mode".into())),
        };
        let relative = path_from_bytes(&record[tab + 1..])?;
        if rules.excludes(&relative, kind == Kind::Directory) {
            continue;
        }
        let size = if header[3] == "-" && matches!(kind, Kind::Directory | Kind::Other) {
            0
        } else {
            header[3]
                .parse::<u64>()
                .map_err(|_| Error::Invalid("Invalid Git object size".into()))?
        };
        let error = if kind == Kind::Other {
            Some("Git submodule references are not expanded".into())
        } else {
            None
        };
        let entry = Entry {
            kind,
            bytes: size.to_string(),
            modified_ms: None,
            digest: None,
            error,
        };
        if entries
            .insert(
                relative,
                GitEntry {
                    working: None,
                    oid: header[2].into(),
                    mode: header[0].into(),
                    entry,
                },
            )
            .is_some()
        {
            return Err(Error::Invalid("Git tree contains duplicate paths".into()));
        }
    }
    Ok(entries)
}
fn load_index(
    repo: &Path,
    rules: &Rules,
    cancel: &AtomicBool,
) -> Result<(String, BTreeMap<PathBuf, GitEntry>)> {
    let output = command(
        repo,
        &[
            "ls-files",
            "--cached",
            "--full-name",
            "-z",
            "--format=%(objectmode) %(objecttype) %(objectname) %(objectsize) %(stage)%x09%(path)",
            "--",
        ],
        MAX_LIST,
        cancel,
    )?;
    if !output.is_empty() && output.last() != Some(&0) {
        return Err(Error::Invalid("Truncated Git index response".into()));
    }
    let identity = format!("Index @ {:x}", Sha256::digest(&output));
    let mut paths = BTreeMap::<PathBuf, BTreeMap<u8, GitEntry>>::new();
    let mut records = 0;
    for record in output.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        check_cancel(cancel)?;
        records += 1;
        if records > MAX_ENTRIES * 3 {
            return Err(Error::Invalid(
                "Git index exceeds 600000 stage records".into(),
            ));
        }
        let tab = record
            .iter()
            .position(|b| *b == b'\t')
            .ok_or_else(|| Error::Invalid("Malformed Git index record".into()))?;
        let header = std::str::from_utf8(&record[..tab])
            .map_err(|_| Error::Invalid("Invalid Git index metadata".into()))?
            .split_ascii_whitespace()
            .collect::<Vec<_>>();
        if header.len() != 5 || !valid_oid(header[2]) {
            return Err(Error::Invalid("Invalid Git index object metadata".into()));
        }
        let kind = match (header[0], header[1]) {
            ("100644" | "100755", "blob") => Kind::File,
            ("120000", "blob") => Kind::Link,
            ("160000", "commit") => Kind::Other,
            _ => {
                return Err(Error::Invalid(
                    "Unsupported or unexpanded sparse Git index mode".into(),
                ));
            }
        };
        let path = path_from_bytes(&record[tab + 1..])?;
        if rules.excludes(&path, false) {
            continue;
        }
        let stage = header[4]
            .parse::<u8>()
            .map_err(|_| Error::Invalid("Invalid Git index stage".into()))?;
        if stage > 3 {
            return Err(Error::Invalid("Unsupported Git index stage".into()));
        }
        let size = if kind == Kind::Other && header[3] == "-" {
            0
        } else {
            header[3]
                .parse::<u64>()
                .map_err(|_| Error::Invalid("Invalid Git index object size".into()))?
        };
        let error =
            (kind == Kind::Other).then(|| "Git submodule references are not expanded".into());
        let entry = GitEntry {
            working: None,
            oid: header[2].into(),
            mode: header[0].into(),
            entry: Entry {
                kind,
                bytes: size.to_string(),
                modified_ms: None,
                digest: None,
                error,
            },
        };
        if paths
            .entry(path)
            .or_default()
            .insert(stage, entry)
            .is_some()
        {
            return Err(Error::Invalid("Duplicate Git index stage".into()));
        }
        if paths.len() > MAX_ENTRIES {
            return Err(Error::Invalid("Git index exceeds 200000 paths".into()));
        }
    }
    let mut entries = BTreeMap::new();
    for (path, mut stages) in paths {
        check_cancel(cancel)?;
        let entry = if let Some(entry) = stages.remove(&0) {
            if !stages.is_empty() {
                return Err(Error::Invalid(
                    "Git index mixes merged and unmerged stages".into(),
                ));
            }
            entry
        } else {
            let labels = stages
                .keys()
                .map(u8::to_string)
                .collect::<Vec<_>>()
                .join(", ");
            let mut entry = stages
                .into_values()
                .next()
                .ok_or_else(|| Error::Invalid("Empty index stage group".into()))?;
            entry.entry.kind = Kind::Other;
            entry.entry.error = Some(format!(
                "Unmerged Git index path (stages {labels}); resolve the conflict before previewing"
            ));
            entry
        };
        entries.insert(path, entry);
    }
    let files = entries.keys().cloned().collect::<Vec<_>>();
    for path in files {
        for parent in path
            .ancestors()
            .skip(1)
            .filter(|p| !p.as_os_str().is_empty())
        {
            check_cancel(cancel)?;
            entries
                .entry(parent.to_owned())
                .or_insert_with(|| GitEntry {
                    working: None,
                    oid: String::new(),
                    mode: "040000".into(),
                    entry: Entry {
                        kind: Kind::Directory,
                        bytes: "0".into(),
                        modified_ms: None,
                        digest: None,
                        error: None,
                    },
                });
            if entries.len() > MAX_ENTRIES {
                return Err(Error::Invalid(
                    "Git index with directories exceeds 200000 entries".into(),
                ));
            }
        }
    }
    Ok((identity, entries))
}

fn status(a: Option<&GitEntry>, b: Option<&GitEntry>) -> Status {
    let ordinary = crate::archive::status(a.map(|e| &e.entry), b.map(|e| &e.entry));
    if matches!(
        ordinary,
        Status::Error | Status::LeftOnly | Status::RightOnly | Status::TypeConflict
    ) {
        return ordinary;
    }
    match (a, b) {
        (Some(a), Some(_)) if a.entry.kind == Kind::Directory => Status::Same,
        (Some(a), Some(b))
            if a.entry.kind == Kind::Link && (a.oid.is_empty() || b.oid.is_empty()) =>
        {
            Status::MetadataMatch
        }
        (Some(a), Some(b)) if a.oid != b.oid || a.mode != b.mode => Status::Changed,
        (Some(a), Some(_)) if a.entry.kind == Kind::File => Status::Same,
        _ => Status::MetadataMatch,
    }
}
pub struct Snapshot {
    report_roots: Vec<PathBuf>,
    pub report: Comparison,
    repo: PathBuf,
    worktree: Option<PathBuf>,
    blobs: BTreeMap<String, (Option<GitEntry>, Option<GitEntry>)>,
}
pub fn compare(request: &GitCompareRequest, cancel: &AtomicBool) -> Result<Snapshot> {
    let repo = canonical_root(&request.repository)?;
    if fs::symlink_metadata(&request.repository)?
        .file_type()
        .is_symlink()
    {
        return Err(Error::Invalid("Choose a real repository directory".into()));
    }
    let rules = Rules::parse(&request.exclusions)?;
    let has_worktree = request.left_source == clear_contracts::GitSource::WorkingTree
        || request.right_source == clear_contracts::GitSource::WorkingTree;
    if request.include_untracked && !has_worktree {
        return Err(Error::Invalid(
            "Untracked paths require a working-tree source".into(),
        ));
    }

    let git_dir = directory(&repo, "--absolute-git-dir", cancel)?;
    let bare = command(&repo, &["rev-parse", "--is-bare-repository"], 32, cancel)?;
    let common_dir = directory(&repo, "--git-common-dir", cancel)?;
    let mut report_roots = vec![git_dir];
    let mut worktree = None;
    if !report_roots.contains(&common_dir) {
        report_roots.push(common_dir);
    }
    if bare == b"false\n" || bare == b"false\r\n" {
        let root = directory(&repo, "--show-toplevel", cancel)?;
        worktree = Some(root.clone());
        report_roots.push(root);
    } else if bare != b"true\n" && bare != b"true\r\n" {
        return Err(Error::Invalid(
            "Invalid Git repository classification".into(),
        ));
    }
    let index = if request.left_source == clear_contracts::GitSource::Index
        || request.right_source == clear_contracts::GitSource::Index
    {
        Some(load_index(
            worktree.as_deref().ok_or_else(|| {
                Error::Invalid("Index comparison requires a non-bare worktree".into())
            })?,
            &rules,
            cancel,
        )?)
    } else {
        None
    };
    let working = if has_worktree {
        Some(worktree::load(
            worktree.as_deref().ok_or_else(|| {
                Error::Invalid("Working-tree comparison requires a non-bare worktree".into())
            })?,
            &rules,
            request.include_untracked,
            cancel,
        )?)
    } else {
        None
    };
    let has_index = index.is_some();
    let source = |mode: clear_contracts::GitSource,
                  revision: &str|
     -> Result<(String, BTreeMap<PathBuf, GitEntry>)> {
        match mode {
            clear_contracts::GitSource::Commit => {
                let oid = resolve(&repo, revision, cancel)?;
                Ok((
                    format!("{revision} @ {oid}"),
                    load(&repo, &oid, &rules, cancel)?,
                ))
            }
            clear_contracts::GitSource::Index => index
                .clone()
                .ok_or_else(|| Error::Invalid("Index source unavailable".into())),
            clear_contracts::GitSource::WorkingTree => working
                .clone()
                .ok_or_else(|| Error::Invalid("Working-tree source unavailable".into())),
        }
    };
    let (left, l) = source(request.left_source, &request.left_revision)?;
    let (right, r) = source(request.right_source, &request.right_revision)?;
    drop(index);
    drop(working);
    let all = l.keys().chain(r.keys()).cloned().collect::<BTreeSet<_>>();
    if all.len() > MAX_ENTRIES {
        return Err(Error::Invalid("Git comparison exceeds 200000 paths".into()));
    }
    let mut rows = Vec::new();
    let mut paths = BTreeMap::new();
    let mut blobs = BTreeMap::new();
    let mut summary = Summary::default();
    for (i, path) in all.into_iter().enumerate() {
        check_cancel(cancel)?;
        let a = l.get(&path).cloned();
        let b = r.get(&path).cloned();
        let state = status(a.as_ref(), b.as_ref());
        if !a
            .as_ref()
            .or(b.as_ref())
            .is_some_and(|e| e.entry.kind == Kind::Directory)
            || matches!(state, Status::Error | Status::TypeConflict)
        {
            match state {
                Status::Same => summary.same += 1,
                Status::Changed => summary.changed += 1,
                Status::LeftOnly => summary.left_only += 1,
                Status::RightOnly => summary.right_only += 1,
                Status::MetadataMatch => summary.unverified += 1,
                _ => summary.errors += 1,
            }
        }
        let id = i.to_string();
        paths.insert(id.clone(), path.clone());
        rows.push(Row {
            id: id.clone(),
            path: tree::display_path(&path),
            directory_summary: None,
            status: state,
            left: a.as_ref().map(|e| e.entry.clone()),
            right: b.as_ref().map(|e| e.entry.clone()),
        });
        blobs.insert(id, (a, b));
    }
    tree::aggregate(&mut rows, &paths);
    let mut warnings=vec!["Read-only Git objects; equality uses object identity, not timestamps. Executable-mode changes count as changes. Links are not followed, submodules are not expanded, and LFS pointers are not dereferenced. Untracked inclusion is explicit for working-tree sources.".into()];
    if has_index {
        warnings.push("Index records are captured once and pinned to object IDs; the displayed SHA-256 identifies the listing, not the index file. Unmerged stages are errors. Intent-to-add records use their indexed empty blob, not working-tree contents. Later staging changes require a new comparison.".into());
    }
    if has_worktree {
        warnings.push("Working-tree files are streamed and hashed as raw Git blobs, without clean/smudge/EOL filters. This is a physical file scan, not a point-in-time snapshot; sparse/skip-worktree missing paths remain absent. Text previews verify the captured content and metadata. Ignored untracked paths are excluded; unmerged index stages do not replace working-tree bytes.".into());
    }
    Ok(Snapshot {
        report_roots,
        repo,
        worktree,
        blobs,
        report: Comparison {
            policy: Some(clear_contracts::ComparisonPolicy {
                exclusions: request.exclusions.clone(),
                verification: clear_contracts::VerificationBasis::GitObjectIdentity,
            }),
            session: unique_id(),
            left,
            right,
            summary,
            rows,
            warnings,
        },
    })
}
impl Snapshot {
    pub fn report_roots(&self) -> Vec<PathBuf> {
        self.report_roots.clone()
    }

    pub fn page(&self, offset: usize, search: &str, differences_only: bool) -> Result<Page> {
        crate::archive::page_report(&self.report, offset, search, differences_only)
    }
    fn text(&self, entry: Option<&GitEntry>, cancel: &AtomicBool) -> Result<String> {
        let Some(entry) = entry else {
            return Ok(String::new());
        };
        if entry.entry.kind != Kind::File || entry.entry.error.is_some() {
            return Err(Error::Invalid("Choose a regular Git text file".into()));
        }
        if entry.entry.bytes.parse::<u64>().unwrap_or(u64::MAX)
            > crate::editing::MAX_DOCUMENT_BYTES as u64
        {
            return Err(Error::Invalid("Git text preview exceeds 4 MiB".into()));
        }
        let bytes = if let Some(state) = &entry.working {
            worktree::preview(
                self.worktree
                    .as_deref()
                    .ok_or_else(|| Error::Invalid("Working-tree root unavailable".into()))?,
                state,
                entry,
                cancel,
            )?
        } else {
            command(
                &self.repo,
                &["cat-file", "blob", &entry.oid],
                crate::editing::MAX_DOCUMENT_BYTES as u64,
                cancel,
            )?
        };
        if bytes.len().to_string() != entry.entry.bytes {
            return Err(Error::Invalid(
                "Git blob size changed or object is corrupt".into(),
            ));
        }
        Ok(crate::editing::decode(&bytes)?.0)
    }
    pub fn text_diff(
        &self,
        id: &str,
        options: &TextOptions,
        cancel: &AtomicBool,
    ) -> Result<TextDiff> {
        let (a, b) = self
            .blobs
            .get(id)
            .ok_or_else(|| Error::Invalid("Unknown Git entry ID".into()))?;
        let row = self
            .report
            .rows
            .iter()
            .find(|r| r.id == id)
            .ok_or_else(|| Error::Invalid("Unknown Git row".into()))?;
        let left = self.text(a.as_ref(), cancel)?;
        let right = self.text(b.as_ref(), cancel)?;
        check_cancel(cancel)?;
        crate::text::compare_strings(&row.path, &left, &right, options)
    }
}
