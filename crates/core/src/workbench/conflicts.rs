//! Captured index stages and saved-result validation for conflict resolution.
use super::*;
use crate::{compare::checked_path, editing, sync::durable::Object};
#[derive(Clone)]
pub(super) struct Guard {
    relative: PathBuf,
    stages: Vec<u8>,
    control: String,
}
#[derive(Clone)]
pub(super) struct ResolvedGuard {
    pub(super) document: String,
    conflict: Guard,
    original: editing::OpenDocument,
    object: Option<Object>,
}
fn invalid(message: &str) -> Error {
    Error::Invalid(message.into())
}
fn stages(repo: &Captured, path: &Path, cancel: &AtomicBool) -> Result<Vec<u8>> {
    runner::run(
        &repo.root,
        &[
            "ls-files",
            "--unmerged",
            "-z",
            "--",
            path.to_str()
                .ok_or_else(|| invalid("Conflict inspection requires a Unicode path"))?,
        ],
        cancel,
    )
}
fn control(repo: &Captured, cancel: &AtomicBool) -> Result<String> {
    let mut data = runner::run(&repo.root, &["rev-parse", "--verify", "HEAD"], cancel)?;
    for name in [
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "rebase-merge/head-name",
        "rebase-merge/onto",
        "rebase-merge/stopped-sha",
        "rebase-apply/head-name",
        "rebase-apply/onto",
        "rebase-apply/next",
    ] {
        check_cancel(cancel)?;
        let path = repo.git.join(name);
        match fs::symlink_metadata(&path) {
            Ok(meta)
                if meta.is_file()
                    && !meta.file_type().is_symlink()
                    && meta.len() <= 1024 * 1024 =>
            {
                data.extend_from_slice(name.as_bytes());
                data.extend_from_slice(
                    id(&crate::sync::durable::bytes(&path, 1024 * 1024)?).as_bytes(),
                );
            }
            Ok(_) => {
                return Err(invalid(
                    "Git operation metadata is not a bounded regular file",
                ));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => data.push(0),
            Err(e) => return Err(e.into()),
        }
    }
    Ok(id(&data))
}
impl Guard {
    pub(super) fn check(&self, repo: &Captured, cancel: &AtomicBool) -> Result<()> {
        if git_directory(&repo.root, &["rev-parse", "--absolute-git-dir"], cancel)? != repo.git
            || git_directory(
                &repo.root,
                &["rev-parse", "--path-format=absolute", "--git-common-dir"],
                cancel,
            )? != repo.common
            || stages(repo, &self.relative, cancel)? != self.stages
            || control(repo, cancel)? != self.control
        {
            return Err(invalid(
                "Conflict stages or Git operation changed; reopen the conflict before saving or resolving",
            ));
        }
        Ok(())
    }
}
impl ResolvedGuard {
    pub(super) fn journal(
        &self,
        repo: &Captured,
        operation: &str,
        cancel: &AtomicBool,
    ) -> Result<PathBuf> {
        self.check(repo, cancel)?;
        let base = repo.common.join("clear-compare-conflict-operations");
        super::remotes::directory(&base)?;
        let directory = base.join(operation);
        super::remotes::directory(&directory)?;
        let index = repo.git.join("index");
        let meta = fs::symlink_metadata(&index)?;
        if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 32 * 1024 * 1024 {
            return Err(invalid(
                "Conflict index backup requires a regular index below 32 MiB",
            ));
        }
        let original = crate::sync::durable::bytes(&index, 32 * 1024 * 1024)?;
        let mut references = Vec::new();
        for record in self
            .conflict
            .stages
            .split(|b| *b == 0)
            .filter(|r| !r.is_empty())
        {
            let header = record
                .split(|b| *b == b'\t')
                .next()
                .ok_or_else(|| invalid("Invalid captured conflict"))?;
            let parts = std::str::from_utf8(header)
                .map_err(|_| invalid("Invalid captured conflict"))?
                .split(' ')
                .collect::<Vec<_>>();
            if parts.len() != 3 {
                return Err(invalid("Invalid captured conflict"));
            }
            references.push((
                format!(
                    "refs/clear-compare/conflicts/{operation}/stage-{}",
                    parts[2]
                ),
                parts[1].to_string(),
            ));
        }
        super::remotes::write(
            &directory.join("intent.json"),
            &serde_json::to_vec_pretty(&serde_json::json!({
                "version":1,"repository":repo.view.id,"path":crate::native_path::encode(self.path()),
                "index_digest":id(&original),"result_digest":self.original.view.digest,"references":references
            }))?,
        )?;
        super::remotes::write(&directory.join("index.original"), &original)?;
        super::remotes::write(&directory.join("unmerged.original"), &self.conflict.stages)?;
        for (reference, object) in references {
            check_cancel(cancel)?;
            runner::run(&repo.root, &["update-ref", &reference, &object, ""], cancel)?;
        }
        self.check(repo, cancel)?;
        if crate::sync::durable::bytes(&index, 32 * 1024 * 1024)? != original {
            return Err(invalid(
                "Git index changed while recording conflict recovery",
            ));
        }
        Ok(directory)
    }
    pub(super) fn path(&self) -> &Path {
        &self.conflict.relative
    }
    pub(super) fn check(&self, repo: &Captured, cancel: &AtomicBool) -> Result<()> {
        self.conflict.check(repo, cancel)?;
        checked_path(&repo.root, self.path(), false)?;
        if Object::observe(&repo.root, self.path())? != self.object
            || !self.original.location_valid()
        {
            return Err(invalid(
                "Saved conflict result changed; save or reopen before marking resolved",
            ));
        }
        Ok(())
    }
}
fn version(
    repo: &Captured,
    mode: &str,
    object: &str,
    stage: u8,
    cancel: &AtomicBool,
) -> Result<ConflictVersion> {
    let mut view = ConflictVersion {
        stage,
        object: object.into(),
        mode: mode.into(),
        text: None,
        issue: None,
    };
    if !matches!(mode, "100644" | "100755") {
        view.issue = Some("unsupported_mode".into());
        return Ok(view);
    }
    let size = runner::text(&repo.root, &["cat-file", "-s", object], cancel)?
        .parse::<usize>()
        .map_err(|_| invalid("Invalid conflict blob size"))?;
    if size > editing::MAX_DOCUMENT_BYTES {
        view.issue = Some("too_large".into());
        return Ok(view);
    }
    let bytes = runner::run(&repo.root, &["cat-file", "blob", object], cancel)?;
    match editing::decode(&bytes) {
        Ok((text, _, _)) if crate::text::split_lines(&text).len() <= 30_000 => {
            view.text = Some(text)
        }
        Ok(_) => view.issue = Some("too_many_lines".into()),
        Err(_) => view.issue = Some("binary_or_encoding".into()),
    }
    Ok(view)
}
impl Workspace {
    pub fn conflict_open(
        &mut self,
        repository: &str,
        file: &str,
        cancel: &AtomicBool,
    ) -> Result<ConflictDocument> {
        let state = self.open(repository, cancel)?;
        let change = state
            .changes
            .iter()
            .find(|c| c.id == file && c.conflicted)
            .ok_or_else(|| invalid("Selected file no longer has unresolved Git stages"))?;
        let repo = self
            .repositories
            .get(repository)
            .ok_or_else(|| invalid("Unknown repository"))?
            .clone();
        self.source_root(repository, cancel)?;
        let relative = repo
            .files
            .get(file)
            .filter(|p| p.len() == 1)
            .and_then(|p| p.first())
            .cloned()
            .ok_or_else(|| invalid("Conflict file selection expired"))?;
        if relative
            .components()
            .any(|p| p.as_os_str().to_string_lossy().eq_ignore_ascii_case(".git"))
        {
            return Err(invalid("Git metadata is protected"));
        }
        let guard = Guard {
            stages: stages(&repo, &relative, cancel)?,
            control: control(&repo, cancel)?,
            relative: relative.clone(),
        };
        let mut versions = [None, None, None];
        for record in guard.stages.split(|b| *b == 0).filter(|r| !r.is_empty()) {
            let tab = record
                .iter()
                .position(|b| *b == b'\t')
                .ok_or_else(|| invalid("Malformed unmerged index entry"))?;
            let (header, tail) = record.split_at(tab);
            let name = &tail[1..];
            if status::native_path(name)? != relative {
                return Err(invalid("Unexpected conflict path"));
            }
            let fields = std::str::from_utf8(header)
                .map_err(|_| invalid("Invalid conflict index header"))?
                .split(' ')
                .collect::<Vec<_>>();
            if fields.len() != 3
                || !matches!(fields[1].len(), 40 | 64)
                || !fields[1].bytes().all(|c| c.is_ascii_hexdigit())
            {
                return Err(invalid("Invalid conflict index header"));
            }
            let stage = fields[2]
                .parse::<u8>()
                .map_err(|_| invalid("Invalid conflict stage"))?;
            if !(1..=3).contains(&stage) || versions[stage as usize - 1].is_some() {
                return Err(invalid("Duplicate or invalid conflict stage"));
            }
            versions[stage as usize - 1] =
                Some(version(&repo, fields[0], fields[1], stage, cancel)?);
        }
        if versions.iter().all(Option::is_none) {
            return Err(invalid("Conflict stages disappeared; refresh repository"));
        }
        let mut issue = versions.iter().flatten().find_map(|v| v.issue.clone());
        let suggestion = if issue.is_none() {
            let text = |i: usize| {
                versions[i]
                    .as_ref()
                    .and_then(|v| v.text.as_deref())
                    .unwrap_or("")
            };
            // Absence is visible in the version metadata; it is never claimed to be an empty blob.
            Some(crate::merge::three_way(text(0), text(1), text(2))?)
        } else {
            None
        };
        guard.check(&repo, cancel)?;
        let path = checked_path(&repo.root, &relative, true)?;
        let document = if issue.is_none()
            && fs::symlink_metadata(&path).is_ok_and(|m| m.is_file() && !m.file_type().is_symlink())
        {
            let file_cap = self.capture_file(repository, &repo.root, relative)?;
            let existed = self
                .source_documents
                .values()
                .any(|d| d.file.repository == repository && d.file.relative == guard.relative);
            let doc = self.source_open(repository, &file_cap.id, None, cancel)?;
            let saved = self
                .source_documents
                .get(&doc.document.id)
                .ok_or_else(|| invalid("Source document unavailable"))?;
            if saved.dirty || !saved.original.location_valid() {
                if !existed {
                    self.source_close(&doc.document.id, false)?;
                }
                return Err(invalid(
                    "Save or reload the existing source buffer before opening this conflict",
                ));
            }
            if let Err(error) = guard.check(&repo, cancel) {
                if !existed {
                    self.source_close(&doc.document.id, false)?;
                }
                return Err(error);
            }
            self.conflict_documents
                .insert(doc.document.id.clone(), guard.clone());
            Some(doc)
        } else {
            if issue.is_none() {
                issue = Some("missing_or_special_result".into());
            }
            None
        };
        let [base, current, incoming] = versions;
        Ok(ConflictDocument {
            repository_id: repository.into(),
            file_id: file.into(),
            path: change.path.clone(),
            operation: state.operation,
            base,
            current,
            incoming,
            document,
            suggestion,
            issue,
        })
    }
    pub(super) fn conflict_resolution(
        &self,
        repository: &str,
        document: &str,
        cancel: &AtomicBool,
    ) -> Result<ResolvedGuard> {
        let saved = self
            .source_documents
            .get(document)
            .filter(|d| d.file.repository == repository)
            .ok_or_else(|| invalid("Unknown conflict document"))?;
        let guard = self
            .conflict_documents
            .get(document)
            .ok_or_else(|| invalid("Open this file through the Git conflict resolver first"))?;
        if saved.dirty {
            return Err(invalid("Save the conflict result before marking resolved"));
        }
        if saved.original.view.text.lines().any(|line| {
            ["<<<<<<<", "|||||||", "=======", ">>>>>>>"]
                .iter()
                .any(|m| line.starts_with(m))
        }) {
            return Err(invalid(
                "Unresolved conflict markers remain in the saved result",
            ));
        }
        let resolved = ResolvedGuard {
            document: document.into(),
            conflict: guard.clone(),
            original: saved.original.clone(),
            object: saved.object.clone(),
        };
        resolved.check(
            self.repositories
                .get(repository)
                .ok_or_else(|| invalid("Unknown repository"))?,
            cancel,
        )?;
        Ok(resolved)
    }
}
