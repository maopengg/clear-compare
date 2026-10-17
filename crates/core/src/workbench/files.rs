//! Captured working-tree file capabilities, editor buffers and bounded source search.
use super::*;
use crate::{compare::checked_path, editing, rules::Rules, sync::durable::Object};
use clear_contracts::{SaveOptions, SaveResult};
use std::{
    fs::File,
    io::Read,
    time::{Duration, Instant},
};

#[derive(Clone)]
pub(super) struct FileCapability {
    repository: String,
    relative: PathBuf,
    kind: String,
}
#[derive(Clone)]
pub(super) struct Document {
    file: FileCapability,
    original: editing::OpenDocument,
    object: Option<Object>,
    dirty: bool,
    generation: u64,
}
fn invalid(message: &str) -> Error {
    Error::Invalid(message.into())
}
fn protected(path: &Path) -> bool {
    path.components()
        .any(|c| c.as_os_str().to_string_lossy().eq_ignore_ascii_case(".git"))
}
impl Workspace {
    fn source_root(&mut self, repository: &str, cancel: &AtomicBool) -> Result<PathBuf> {
        if !self.repositories.contains_key(repository) {
            self.open(repository, cancel)?;
        }
        let repo = self
            .repositories
            .get(repository)
            .ok_or_else(|| invalid("Unknown repository"))?;
        if repo.view.kind == "bare" {
            return Err(invalid("Bare repositories have no working-tree files"));
        }
        if Object::observe(&repo.root, Path::new(""))? != repo.root_identity
            || Object::observe(&repo.git, Path::new(""))? != repo.git_identity
        {
            return Err(invalid("Repository identity changed; scan again"));
        }
        Ok(repo.root.clone())
    }
    fn file_cap(
        &self,
        repository: &str,
        entry: &str,
        kind: Option<&str>,
    ) -> Result<FileCapability> {
        let cap = self
            .file_caps
            .get(entry)
            .filter(|f| f.repository == repository && kind.is_none_or(|k| k == f.kind))
            .ok_or_else(|| invalid("File selection expired; refresh the file tree"))?;
        if protected(&cap.relative) {
            return Err(invalid("Git metadata is protected"));
        }
        Ok(cap.clone())
    }
    fn capture_file(
        &mut self,
        repository: &str,
        root: &Path,
        relative: PathBuf,
    ) -> Result<WorkspaceFile> {
        if protected(&relative) {
            return Err(invalid("Git metadata is protected"));
        }
        let meta = fs::symlink_metadata(root.join(&relative))?;
        let kind = if meta.file_type().is_symlink() {
            "link"
        } else if meta.is_dir() {
            "directory"
        } else if meta.is_file() {
            "file"
        } else {
            "special"
        };
        let key = id(&serde_json::to_vec(&(
            repository,
            crate::native_path::encode(&relative),
        ))?);
        if self.file_caps.len() >= 250_000 && !self.file_caps.contains_key(&key) {
            return Err(invalid(
                "File capability limit reached; restart after saving buffers",
            ));
        }
        self.file_caps.insert(
            key.clone(),
            FileCapability {
                repository: repository.into(),
                relative: relative.clone(),
                kind: kind.into(),
            },
        );
        Ok(WorkspaceFile {
            id: key,
            name: relative
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            path: crate::tree::display_path(&relative),
            kind: kind.into(),
            bytes: meta.len().to_string(),
        })
    }
    pub fn file_settings(
        &mut self,
        repository: &str,
        settings: FileSettings,
        cancel: &AtomicBool,
    ) -> Result<FileSettings> {
        self.source_root(repository, cancel)?;
        Rules::parse(&settings.exclusions)?;
        self.catalog
            .file_settings
            .insert(repository.into(), settings.clone());
        Ok(settings)
    }
    pub fn file_tree(
        &mut self,
        repository: &str,
        directory: Option<&str>,
        offset: usize,
        cancel: &AtomicBool,
    ) -> Result<FilePage> {
        let root = self.source_root(repository, cancel)?;
        let relative = directory
            .map(|id| {
                self.file_cap(repository, id, Some("directory"))
                    .map(|c| c.relative)
            })
            .transpose()?
            .unwrap_or_default();
        let path = checked_path(&root, &relative, false)?;
        let settings = self
            .catalog
            .file_settings
            .get(repository)
            .cloned()
            .unwrap_or_default();
        let rules = Rules::parse(&settings.exclusions)?;
        let mut children = Vec::new();
        let warnings = Vec::new();
        for entry in fs::read_dir(&path)? {
            check_cancel(cancel)?;
            let entry = entry?;
            let name = entry.file_name();
            let next = relative.join(&name);
            if protected(&next)
                || (!settings.show_hidden && name.to_string_lossy().starts_with('.'))
            {
                continue;
            }
            let meta = fs::symlink_metadata(entry.path())?;
            if rules.excludes(&next, meta.is_dir()) {
                continue;
            }
            children.push(next);
            if children.len() > 50_000 {
                return Err(invalid("A directory contains more than 50000 entries"));
            }
        }
        if settings.respect_gitignore && !children.is_empty() {
            let mut ignored = BTreeSet::new();
            let mut input = Vec::new();
            for path in &children {
                #[cfg(unix)]
                let bytes = {
                    use std::os::unix::ffi::OsStrExt;
                    path.as_os_str().as_bytes()
                };
                #[cfg(not(unix))]
                let bytes = path
                    .to_str()
                    .ok_or_else(|| invalid("Git ignore requires Unicode paths on this platform"))?
                    .as_bytes();
                input.extend_from_slice(b"./");
                input.extend_from_slice(bytes);
                input.push(0);
            }
            for path in runner::optional(
                &root,
                &["check-ignore".into(), "-z".into(), "--stdin".into()],
                &input,
                cancel,
            )?
            .split(|b| *b == 0)
            .filter(|p| !p.is_empty())
            {
                ignored.insert(status::native_path(
                    path.strip_prefix(b"./").unwrap_or(path),
                )?);
            }
            children.retain(|p| !ignored.contains(p));
        }
        let mut files = children
            .into_iter()
            .map(|p| self.capture_file(repository, &root, p))
            .collect::<Result<Vec<_>>>()?;
        files.sort_by(|a, b| {
            (a.kind != "directory")
                .cmp(&(b.kind != "directory"))
                .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                .then(a.id.cmp(&b.id))
        });
        let total = files.len();
        if offset > total {
            return Err(invalid("File page is out of range"));
        }
        let parent = if relative.as_os_str().is_empty() {
            None
        } else {
            Some(
                self.capture_file(
                    repository,
                    &root,
                    relative.parent().unwrap_or(Path::new("")).into(),
                )?
                .id,
            )
        };
        Ok(FilePage {
            parent,
            path: crate::tree::display_path(&relative),
            files: files.into_iter().skip(offset).take(300).collect(),
            offset,
            total,
            warnings,
        })
    }
    pub fn source_open(
        &mut self,
        repository: &str,
        file: &str,
        expected: Option<&str>,
        cancel: &AtomicBool,
    ) -> Result<WorkspaceDocument> {
        let root = self.source_root(repository, cancel)?;
        let cap = self.file_cap(repository, file, Some("file"))?;
        let path = checked_path(&root, &cap.relative, false)?;
        if let Some(existing) = self
            .source_documents
            .values()
            .find(|d| d.file.repository == repository && d.file.relative == cap.relative)
        {
            if expected.is_some_and(|digest| digest != existing.original.view.digest)
                || (expected.is_some() && existing.dirty)
                || (expected.is_some() && !existing.original.location_valid())
            {
                return Err(invalid(
                    "Save or reload the open buffer before navigating this search result",
                ));
            }
            return Ok(WorkspaceDocument {
                repository_id: repository.into(),
                file_id: file.into(),
                document: existing.original.view.clone(),
            });
        }
        if self.source_documents.len() >= 32 {
            return Err(invalid("At most 32 source documents can be open"));
        }
        if fs::metadata(&path)?.len() > editing::MAX_DOCUMENT_BYTES as u64 {
            return Err(invalid(
                "Text editing is limited to 4 MiB; use a large-file viewer",
            ));
        }
        let object = Object::observe(&root, &cap.relative)?;
        let mut original = editing::open_file(&path)?;
        if expected.is_some_and(|d| d != original.view.digest) {
            return Err(invalid(
                "Search result changed on disk; search again before navigating",
            ));
        }
        if Object::observe(&root, &cap.relative)? != object {
            return Err(invalid("File changed while opening"));
        }
        original.view.path = crate::tree::display_path(&cap.relative);
        let view = WorkspaceDocument {
            repository_id: repository.into(),
            file_id: file.into(),
            document: original.view.clone(),
        };
        self.source_documents.insert(
            original.view.id.clone(),
            Document {
                file: cap,
                original,
                object,
                dirty: false,
                generation: 0,
            },
        );
        Ok(view)
    }
    pub fn source_dirty(&mut self, document: &str, dirty: bool, generation: u64) -> Result<()> {
        let doc = self
            .source_documents
            .get_mut(document)
            .ok_or_else(|| invalid("Unknown source document"))?;
        if generation >= doc.generation {
            doc.dirty = dirty;
            doc.generation = generation;
        }
        Ok(())
    }
    pub fn has_dirty_sources(&self) -> bool {
        self.source_documents.values().any(|d| d.dirty)
    }
    pub(super) fn dirty_repository(&self, repository: &str) -> bool {
        self.source_documents
            .values()
            .any(|d| d.dirty && d.file.repository == repository)
    }
    pub(super) fn open_sources_in(&self, repository: &str) -> bool {
        self.source_documents
            .values()
            .any(|d| d.file.repository == repository)
    }
    pub(super) fn retain_source_roots(&self, next: &mut Self) {
        for doc in self.source_documents.values() {
            if let Some(repo) = self.repositories.get(&doc.file.repository) {
                next.repositories
                    .entry(doc.file.repository.clone())
                    .or_insert_with(|| repo.clone());
            }
        }
    }
    pub fn source_close(&mut self, document: &str, discard: bool) -> Result<()> {
        if self.source_documents.get(document).is_some_and(|d| d.dirty) && !discard {
            return Err(invalid(
                "Save or explicitly discard this source buffer before closing",
            ));
        }
        if let Some(doc) = self.source_documents.remove(document)
            && !self.open_sources_in(&doc.file.repository)
            && !self
                .catalog
                .repositories
                .iter()
                .any(|r| r.id == doc.file.repository)
        {
            self.repositories.remove(&doc.file.repository);
            self.file_caps
                .retain(|_, cap| cap.repository != doc.file.repository);
        }
        Ok(())
    }
    pub fn source_save(
        &mut self,
        document: &str,
        text: &str,
        options: &SaveOptions,
        backup: &Path,
        generation: u64,
        cancel: &AtomicBool,
    ) -> Result<SaveResult> {
        let slot = self
            .source_documents
            .get(document)
            .cloned()
            .ok_or_else(|| invalid("Unknown source document"))?;
        if generation < slot.generation {
            return Err(invalid(
                "Source buffer changed while save was pending; save again",
            ));
        }
        let root = self.source_root(&slot.file.repository, cancel)?;
        checked_path(&root, &slot.file.relative, false)?;
        if Object::observe(&root, &slot.file.relative)? != slot.object {
            return Err(invalid(
                "Source file identity changed; reopen before saving",
            ));
        }
        fs::create_dir_all(backup)?;
        crate::compare::reject_overlap(&fs::canonicalize(backup)?, &root)?;
        let (mut original, mut result) =
            editing::save_with_options(&slot.original, text, backup, options)?;
        original.view.id = document.into();
        result.document.id = document.into();
        let object = Object::observe(&root, &slot.file.relative)?;
        self.source_documents.insert(
            document.into(),
            Document {
                file: slot.file,
                original,
                object,
                dirty: false,
                generation: slot.generation.max(generation),
            },
        );
        Ok(result)
    }
    pub fn source_check(&mut self, cancel: &AtomicBool) -> Result<Vec<DocumentStatus>> {
        let mut results = Vec::new();
        for (id, doc) in &self.source_documents {
            check_cancel(cancel)?;
            let changed = self
                .repositories
                .get(&doc.file.repository)
                .is_none_or(|repo| {
                    Object::observe(&repo.root, &doc.file.relative).ok() != Some(doc.object.clone())
                        || !doc.original.location_valid()
                });
            results.push(DocumentStatus {
                id: id.clone(),
                changed,
            });
        }
        Ok(results)
    }
    pub fn source_create(
        &mut self,
        repository: &str,
        directory: Option<&str>,
        name: &str,
        folder: bool,
        cancel: &AtomicBool,
    ) -> Result<WorkspaceFile> {
        let root = self.source_root(repository, cancel)?;
        if name.is_empty()
            || name.len() > 255
            || name == "."
            || name == ".."
            || name.eq_ignore_ascii_case(".git")
            || name
                .chars()
                .any(|c| c.is_control() || "/\\:*?\"<>|".contains(c))
            || name.ends_with(['.', ' '])
        {
            return Err(invalid("Use a single portable file or folder name"));
        }
        let parent = directory
            .map(|id| {
                self.file_cap(repository, id, Some("directory"))
                    .map(|c| c.relative)
            })
            .transpose()?
            .unwrap_or_default();
        let path = checked_path(&root, &parent, false)?.join(name);
        if folder {
            fs::create_dir(&path)?;
        } else {
            File::create_new(&path)?.sync_all()?;
        }
        #[cfg(unix)]
        File::open(path.parent().ok_or_else(|| invalid("Invalid parent"))?)?.sync_all()?;
        self.capture_file(repository, &root, parent.join(name))
    }
    pub fn source_search(
        &mut self,
        query: &SourceSearch,
        cancel: &AtomicBool,
    ) -> Result<SourceResults> {
        if query.query.is_empty()
            || query.query.len() > 8192
            || query.repositories.is_empty()
            || query.repositories.len() > 4096
            || !matches!(query.mode.as_str(), "name" | "path" | "content")
        {
            return Err(invalid(
                "Choose repositories, a valid search mode and a non-empty query up to 8192 bytes",
            ));
        }
        if (query.directory.is_some() || query.file.is_some()) && query.repositories.len() != 1 {
            return Err(invalid("A directory/file scope requires one repository"));
        }
        if query.repositories.iter().any(|id| {
            !self.catalog.repositories.iter().any(|r| &r.id == id)
                && !self.repositories.contains_key(id)
        }) {
            return Err(invalid("Unknown search repository"));
        }
        let pattern = if query.regex {
            query.query.clone()
        } else {
            regex::escape(&query.query)
        };
        let pattern = if query.whole_word {
            format!(r"\b(?:{pattern})\b")
        } else {
            pattern
        };
        let matcher = regex::RegexBuilder::new(&pattern)
            .case_insensitive(!query.case_sensitive)
            .multi_line(query.multiline)
            .dot_matches_new_line(query.multiline)
            .size_limit(4 * 1024 * 1024)
            .build()
            .map_err(|e| invalid(&e.to_string()))?;
        let includes = Rules::parse(&query.include)?;
        let excludes = Rules::parse(&query.exclusions)?;
        let mut result = SourceResults {
            hits: Vec::new(),
            searched_files: 0,
            skipped_files: 0,
            truncated: false,
            cancelled: false,
            warnings: Vec::new(),
        };
        let started = Instant::now();
        let mut seen = BTreeSet::new();
        'repositories: for repository in &query.repositories {
            if started.elapsed() > Duration::from_secs(60) {
                result.truncated = true;
                break;
            }
            if !seen.insert(repository.clone()) {
                continue;
            }
            if check_cancel(cancel).is_err() {
                result.cancelled = true;
                break;
            }
            let root = match self.source_root(repository, cancel) {
                Ok(root) => root,
                Err(Error::Cancelled) => {
                    result.cancelled = true;
                    break;
                }
                Err(e) => {
                    if result.warnings.len() < 64 {
                        let name = self
                            .catalog
                            .repositories
                            .iter()
                            .find(|r| &r.id == repository)
                            .map(|r| r.name.as_str())
                            .unwrap_or("Repository");
                        result.warnings.push(format!("{name}: {e}"));
                    }
                    continue;
                }
            };
            let settings = self
                .catalog
                .file_settings
                .get(repository)
                .cloned()
                .unwrap_or_default();
            let custom = Rules::parse(&settings.exclusions)?;
            let scope = query
                .directory
                .as_ref()
                .map(|id| {
                    self.file_cap(repository, id, Some("directory"))
                        .map(|c| c.relative)
                })
                .transpose()?
                .unwrap_or_default();
            let mut paths = if let Some(file) = &query.file {
                vec![self.file_cap(repository, file, Some("file"))?.relative]
            } else if query.respect_gitignore {
                let mut args = vec![
                    "ls-files".into(),
                    "--cached".into(),
                    "--others".into(),
                    "--exclude-standard".into(),
                    "-z".into(),
                    "--".into(),
                ];
                if !scope.as_os_str().is_empty() {
                    args.push(
                        scope
                            .to_str()
                            .ok_or_else(|| {
                                invalid("Git search scope requires a Unicode directory")
                            })?
                            .into(),
                    );
                }
                let raw = match runner::run_owned(&root, &args, cancel) {
                    Ok(raw) => raw,
                    Err(Error::Cancelled) => {
                        result.cancelled = true;
                        break;
                    }
                    Err(e) => return Err(e),
                };
                raw.split(|b| *b == 0)
                    .filter(|p| !p.is_empty())
                    .map(status::native_path)
                    .collect::<Result<Vec<_>>>()?
            } else {
                let base = checked_path(&root, &scope, false)?;
                let mut paths = Vec::new();
                let mut visited = 0;
                for entry in WalkDir::new(&base)
                    .follow_links(false)
                    .max_depth(128)
                    .into_iter()
                    .filter_entry(|e| {
                        !protected(e.path().strip_prefix(&root).unwrap_or(e.path()))
                            && !custom.excludes(
                                e.path().strip_prefix(&root).unwrap_or(e.path()),
                                e.file_type().is_dir(),
                            )
                            && !excludes.excludes(
                                e.path().strip_prefix(&root).unwrap_or(e.path()),
                                e.file_type().is_dir(),
                            )
                    })
                {
                    visited += 1;
                    if visited > 250_000 || started.elapsed() > Duration::from_secs(60) {
                        result.truncated = true;
                        break;
                    }
                    if check_cancel(cancel).is_err() {
                        result.cancelled = true;
                        break 'repositories;
                    }
                    match entry {
                        Ok(e) if e.file_type().is_file() => paths.push(
                            e.path()
                                .strip_prefix(&root)
                                .map_err(|_| invalid("Search escaped repository"))?
                                .to_path_buf(),
                        ),
                        Ok(_) => {}
                        Err(e) => {
                            if result.warnings.len() < 64 {
                                result.warnings.push(e.to_string());
                            }
                        }
                    }
                    if paths.len() >= 250_000 {
                        result.truncated = true;
                        break;
                    }
                }
                paths
            };
            paths.sort();
            paths.dedup();
            for relative in paths {
                if check_cancel(cancel).is_err() {
                    result.cancelled = true;
                    break 'repositories;
                }
                if started.elapsed() > Duration::from_secs(60)
                    || result.searched_files >= 250_000
                    || result.hits.len() >= 5000
                {
                    result.truncated = true;
                    break 'repositories;
                }
                if protected(&relative)
                    || custom.excludes(&relative, false)
                    || excludes.excludes(&relative, false)
                    || (!query.include.trim().is_empty() && !includes.excludes(&relative, false))
                {
                    continue;
                }
                let path = match checked_path(&root, &relative, false) {
                    Ok(p) => p,
                    Err(_) => {
                        result.skipped_files += 1;
                        continue;
                    }
                };
                let meta = match fs::symlink_metadata(&path) {
                    Ok(m) if m.is_file() => m,
                    _ => {
                        result.skipped_files += 1;
                        continue;
                    }
                };
                result.searched_files += 1;
                let display = crate::tree::display_path(&relative);
                let repository_name = self
                    .repositories
                    .get(repository)
                    .map(|r| r.view.name.clone())
                    .unwrap_or_default();
                if query.mode != "content" {
                    let candidate = if query.mode == "name" {
                        relative
                            .file_name()
                            .map(|s| s.to_string_lossy().to_string())
                            .unwrap_or_default()
                    } else {
                        root.join(&relative).to_string_lossy().to_string()
                    };
                    if matcher.is_match(&candidate) {
                        let file = self.capture_file(repository, &root, relative)?;
                        result.hits.push(SearchHit {
                            file_id: file.id,
                            repository_id: repository.clone(),
                            repository: repository_name,
                            path: display,
                            line: 0,
                            column: 0,
                            offset_utf16: 0,
                            length_utf16: 0,
                            preview: candidate,
                            digest: None,
                        });
                    }
                    continue;
                }
                if meta.len() > editing::MAX_DOCUMENT_BYTES as u64 {
                    result.skipped_files += 1;
                    continue;
                }
                let mut bytes = Vec::new();
                match File::open(&path).and_then(|file| {
                    file.take((editing::MAX_DOCUMENT_BYTES + 1) as u64)
                        .read_to_end(&mut bytes)
                }) {
                    Ok(_) if bytes.len() <= editing::MAX_DOCUMENT_BYTES => {}
                    _ => {
                        result.skipped_files += 1;
                        continue;
                    }
                }
                let text = match editing::decode(&bytes) {
                    Ok((text, _, _)) => text,
                    Err(_) => {
                        result.skipped_files += 1;
                        continue;
                    }
                };
                // Editor offsets refer to LF-normalized text and UTF-16 code units.
                let text = text.replace("\r\n", "\n").replace('\r', "\n");
                let digest = id(&bytes);
                let mut previous = 0;
                let mut utf16 = 0;
                let mut line = 1;
                let mut column = 1;
                let mut count = 0;
                let mut file_id: Option<String> = None;
                for found in matcher.find_iter(&text) {
                    if check_cancel(cancel).is_err() {
                        result.cancelled = true;
                        break 'repositories;
                    }
                    for c in text[previous..found.start()].chars() {
                        utf16 += c.len_utf16();
                        if c == '\n' {
                            line += 1;
                            column = 1;
                        } else {
                            column += 1;
                        }
                    }
                    let length = found.as_str().encode_utf16().count();
                    let cap = match &file_id {
                        Some(id) => id.clone(),
                        None => {
                            let cap = self.capture_file(repository, &root, relative.clone())?.id;
                            file_id = Some(cap.clone());
                            cap
                        }
                    };
                    let preview = text[found.start()..]
                        .split('\n')
                        .next()
                        .unwrap_or_default()
                        .chars()
                        .take(240)
                        .collect();
                    result.hits.push(SearchHit {
                        file_id: cap,
                        repository_id: repository.clone(),
                        repository: repository_name.clone(),
                        path: display.clone(),
                        line,
                        column,
                        offset_utf16: utf16,
                        length_utf16: length,
                        preview,
                        digest: Some(digest.clone()),
                    });
                    for c in found.as_str().chars() {
                        utf16 += c.len_utf16();
                        if c == '\n' {
                            line += 1;
                            column = 1;
                        } else {
                            column += 1;
                        }
                    }
                    previous = found.end();
                    count += 1;
                    if count >= 200 || result.hits.len() >= 5000 {
                        result.truncated = true;
                        break;
                    }
                }
            }
        }
        Ok(result)
    }
}
