//! Reviewed, identity-pinned source replacements with per-file recovery records.
use super::*;
use crate::{
    compare::{checked_path, reject_overlap},
    editing,
    sync::durable::{self, Object},
};
use clear_contracts::{Encoding, LineEnding, SaveOptions};

struct Change {
    file: files::FileCapability,
    root: PathBuf,
    object: Option<Object>,
    parent: Option<Object>,
    original: editing::OpenDocument,
    text: String,
    digest: String,
}
pub(super) struct Prepared {
    view: SourceReplacePreview,
    changes: BTreeMap<String, Change>,
    revisions: BTreeMap<String, String>,
    settings: BTreeMap<String, FileSettings>,
}
fn invalid(text: &str) -> Error {
    Error::Invalid(text.into())
}
fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}
enum ReplacementPart {
    Literal(String),
    Capture(usize),
}
fn replacement_parts(
    matcher: &regex::Regex,
    replacement: &str,
    expand: bool,
) -> Result<Vec<ReplacementPart>> {
    if !expand {
        return Ok(vec![ReplacementPart::Literal(replacement.into())]);
    }
    let mut parts = Vec::new();
    let mut rest = replacement;
    while let Some(at) = rest.find('$') {
        if at != 0 {
            parts.push(ReplacementPart::Literal(rest[..at].into()));
        }
        rest = &rest[at + 1..];
        if let Some(next) = rest.strip_prefix('$') {
            parts.push(ReplacementPart::Literal("$".into()));
            rest = next;
            continue;
        }
        let reference;
        if let Some(next) = rest.strip_prefix('{') {
            let end = next
                .find('}')
                .ok_or_else(|| invalid("Unclosed replacement capture reference"))?;
            reference = &next[..end];
            rest = &next[end + 1..];
        } else {
            let end = rest
                .find(|c: char| !c.is_ascii_alphanumeric() && c != '_')
                .unwrap_or(rest.len());
            reference = &rest[..end];
            rest = &rest[end..];
            if reference.is_empty() {
                parts.push(ReplacementPart::Literal("$".into()));
                continue;
            }
        }
        let index = reference
            .parse::<usize>()
            .ok()
            .filter(|index| *index < matcher.captures_len())
            .or_else(|| {
                matcher
                    .capture_names()
                    .position(|name| name == Some(reference))
            });
        parts.push(ReplacementPart::Capture(index.ok_or_else(|| {
            invalid("Replacement refers to an unknown capture group")
        })?));
    }
    if !rest.is_empty() {
        parts.push(ReplacementPart::Literal(rest.into()));
    }
    Ok(parts)
}
fn append(result: &mut String, text: &str) -> Result<()> {
    if result.len().saturating_add(text.len()) > clear_contracts::edit_history::MAX_TEXT_BYTES {
        return Err(invalid("Replacement exceeds the text size limit"));
    }
    result.push_str(text);
    Ok(())
}
fn replaced(
    matcher: &regex::Regex,
    text: &str,
    parts: &[ReplacementPart],
    cancel: &AtomicBool,
) -> Result<(String, usize)> {
    let mut result = String::new();
    let mut previous = 0;
    let mut count = 0;
    for captures in matcher.captures_iter(text) {
        check_cancel(cancel)?;
        let found = captures
            .get(0)
            .ok_or_else(|| invalid("Missing replacement match"))?;
        count += 1;
        if count > 200_000 {
            return Err(invalid("Replacement exceeds the per-file match limit"));
        }
        append(&mut result, &text[previous..found.start()])?;
        for part in parts {
            append(
                &mut result,
                match part {
                    ReplacementPart::Literal(text) => text,
                    ReplacementPart::Capture(index) => captures
                        .get(*index)
                        .map(|found| found.as_str())
                        .unwrap_or(""),
                },
            )?;
        }
        previous = found.end();
    }
    append(&mut result, &text[previous..])?;
    Ok((result, count))
}
fn original_bytes(document: &clear_contracts::TextDocument) -> usize {
    match document.encoding {
        Encoding::Utf8 => document.text.len() + if document.bom { 3 } else { 0 },
        _ => 2 + document.text.encode_utf16().count() * 2,
    }
}
impl Workspace {
    pub fn source_replace_preview(
        &mut self,
        request: SourceReplaceRequest,
        cancel: &AtomicBool,
    ) -> Result<SourceReplacePreview> {
        self.replace_plan = None;
        if request.query.mode != "content"
            || request.replacement.len() > 65_536
            || request.replacement.contains('\0')
            || request.line_ending == Some(LineEnding::Mixed)
        {
            return Err(invalid(
                "Replacement requires content search, at most 65536 replacement bytes and a valid line-ending policy",
            ));
        }
        let matcher = files::source_matcher(&request.query)?;
        let parts = replacement_parts(
            &matcher,
            &normalize(&request.replacement),
            request.query.regex,
        )?;
        let started = std::time::Instant::now();
        let found = self.source_search_mode(&request.query, cancel, true)?;
        if found.cancelled {
            return Err(Error::Cancelled);
        }
        if found.truncated {
            return Err(invalid(
                "Replacement scan was truncated; narrow the search scope before replacing",
            ));
        }
        let mut revisions = BTreeMap::new();
        let mut settings = BTreeMap::new();
        let mut conflicts = BTreeSet::new();
        for repository in found
            .hits
            .iter()
            .map(|hit| hit.repository_id.clone())
            .collect::<BTreeSet<_>>()
        {
            let state = self.open(&repository, cancel)?;
            for change in state.changes.iter().filter(|change| change.conflicted) {
                conflicts.insert((repository.clone(), change.path.clone()));
            }
            revisions.insert(repository.clone(), state.revision);
            settings.insert(
                repository.clone(),
                self.catalog
                    .file_settings
                    .get(&repository)
                    .cloned()
                    .unwrap_or_default(),
            );
        }
        let mut changes = BTreeMap::new();
        let mut files = Vec::new();
        let mut seen = BTreeSet::new();
        let mut bytes = 0usize;
        for hit in found.hits {
            check_cancel(cancel)?;
            if started.elapsed() > std::time::Duration::from_secs(60) {
                return Err(invalid(
                    "Replacement preview exceeded its time limit; narrow the scope",
                ));
            }
            let cap = self.file_cap(&hit.repository_id, &hit.file_id, Some("file"))?;
            let root = self.source_root(&hit.repository_id, cancel)?;
            let path = checked_path(&root, &cap.relative, false)?;
            if !seen.insert(path.clone()) {
                continue;
            }
            let object = Object::observe(&root, &cap.relative)?;
            let parent = Object::observe(&root, cap.relative.parent().unwrap_or(Path::new("")))?;
            let original = editing::open_file(&path)?;
            if hit.digest.as_deref() != Some(&original.view.digest)
                || Object::observe(&root, &cap.relative)? != object
            {
                return Err(invalid(
                    "A matching file changed during replacement preview; scan again",
                ));
            }
            let before = normalize(&original.view.text);
            let (text, count) = replaced(&matcher, &before, &parts, cancel)?;
            let mut format = original.view.clone();
            if let Some(ending) = &request.line_ending {
                format.line_ending = ending.clone();
            }
            let dirty = self.source_documents.values().any(|doc| {
                doc.file.repository == cap.repository
                    && doc.file.relative == cap.relative
                    && doc.dirty
            });
            let mut issue = if dirty {
                Some("unsaved_buffer")
            } else if conflicts.contains(&(
                cap.repository.clone(),
                crate::tree::display_path(&cap.relative),
            )) {
                Some("git_conflict")
            } else if fs::metadata(&path)?.permissions().readonly() {
                Some("read_only")
            } else if format.line_ending == LineEnding::Mixed {
                Some("mixed_line_endings")
            } else if before == text && request.line_ending.is_none() {
                Some("unchanged")
            } else {
                None
            };
            let mut encoded = if issue.is_none() {
                Some(editing::encode(&text, &format)?)
            } else {
                None
            };
            if encoded
                .as_ref()
                .is_some_and(|bytes| id(bytes) == original.view.digest)
            {
                issue = Some("unchanged");
                encoded = None;
            }
            let file_id = crate::unique_id();
            files.push(SourceReplaceFile {
                id: file_id.clone(),
                file_id: hit.file_id,
                repository_id: hit.repository_id,
                repository: hit.repository,
                path: hit.path,
                matches: count,
                before_bytes: original_bytes(&original.view),
                after_bytes: encoded
                    .as_ref()
                    .map_or(original_bytes(&original.view), Vec::len),
                encoding: original.view.encoding.clone(),
                line_ending: format.line_ending,
                issue: issue.map(str::to_owned),
            });
            if let Some(encoded) = encoded {
                bytes = bytes
                    .saturating_add(original.view.text.len())
                    .saturating_add(text.len());
                if bytes > 64 * 1024 * 1024 {
                    return Err(invalid(
                        "Replacement preview exceeds 64 MiB of decoded text; narrow the scope",
                    ));
                }
                changes.insert(
                    file_id,
                    Change {
                        file: cap,
                        root,
                        object,
                        parent,
                        original,
                        text,
                        digest: id(&encoded),
                    },
                );
            }
        }
        let view = SourceReplacePreview {
            id: crate::unique_id(),
            request,
            files,
            searched_files: found.searched_files,
            skipped_files: found.skipped_files,
            warnings: found.warnings,
        };
        self.replace_plan = Some(Prepared {
            view: view.clone(),
            changes,
            revisions,
            settings,
        });
        Ok(view)
    }
    pub fn source_replace_patch(
        &self,
        plan_id: &str,
        file: &str,
        offset: usize,
        cancel: &AtomicBool,
    ) -> Result<GitPatch> {
        check_cancel(cancel)?;
        let plan = self
            .replace_plan
            .as_ref()
            .filter(|plan| plan.view.id == plan_id)
            .ok_or_else(|| invalid("Replacement plan expired; preview again"))?;
        let change = plan
            .changes
            .get(file)
            .ok_or_else(|| invalid("This file has no applicable replacement"))?;
        let before = normalize(&change.original.view.text);
        let diff = similar::TextDiff::configure()
            .timeout(std::time::Duration::from_millis(500))
            .diff_lines(&before, &change.text);
        let patch = diff
            .unified_diff()
            .context_radius(3)
            .header("before", "after")
            .to_string();
        check_cancel(cancel)?;
        let lines = patch.lines().collect::<Vec<_>>();
        if offset > lines.len() {
            return Err(invalid("Invalid replacement diff page"));
        }
        let long_lines = lines
            .iter()
            .skip(offset)
            .take(500)
            .any(|line| line.chars().count() > 8192);
        let text = lines
            .iter()
            .skip(offset)
            .take(500)
            .map(|line| line.chars().take(8192).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n");
        Ok(GitPatch {
            path: crate::tree::display_path(&change.file.relative),
            text,
            binary: false,
            offset,
            total_lines: lines.len(),
            next_offset: (offset.saturating_add(500) < lines.len()).then_some(offset + 500),
            long_lines,
        })
    }
    fn check_replacement(&mut self, change: &Change, cancel: &AtomicBool) -> Result<()> {
        if self.source_root(&change.file.repository, cancel)? != change.root
            || Object::observe(&change.root, &change.file.relative)? != change.object
            || Object::observe(
                &change.root,
                change.file.relative.parent().unwrap_or(Path::new("")),
            )? != change.parent
            || !change.original.location_valid()
        {
            return Err(invalid(
                "Replacement target changed after preview; preview again",
            ));
        }
        if self.source_documents.values().any(|doc| {
            doc.file.repository == change.file.repository
                && doc.file.relative == change.file.relative
                && doc.dirty
        }) {
            return Err(invalid(
                "Save or close the matching unsaved source buffer before replacing",
            ));
        }
        Ok(())
    }
    pub fn source_replace_apply(
        &mut self,
        plan_id: &str,
        selected: &[String],
        base: &Path,
        cancel: &AtomicBool,
    ) -> Result<SourceReplaceResult> {
        self.source_replace_apply_progress(plan_id, selected, base, cancel, |_| {})
    }
    pub fn source_replace_apply_progress(
        &mut self,
        plan_id: &str,
        selected: &[String],
        base: &Path,
        cancel: &AtomicBool,
        mut progress: impl FnMut(SourceReplaceProgress),
    ) -> Result<SourceReplaceResult> {
        let plan = self
            .replace_plan
            .take()
            .filter(|plan| plan.view.id == plan_id)
            .ok_or_else(|| invalid("Replacement plan expired; preview again"))?;
        if selected.is_empty()
            || selected.len() > plan.changes.len()
            || selected.iter().collect::<BTreeSet<_>>().len() != selected.len()
            || selected.iter().any(|id| !plan.changes.contains_key(id))
        {
            return Err(invalid(
                "Select applicable replacement files without duplicates",
            ));
        }
        for repository in selected
            .iter()
            .map(|id| plan.changes[id].file.repository.clone())
            .collect::<BTreeSet<_>>()
        {
            if self.open(&repository, cancel)?.revision != plan.revisions[&repository]
                || self
                    .catalog
                    .file_settings
                    .get(&repository)
                    .cloned()
                    .unwrap_or_default()
                    != plan.settings[&repository]
            {
                return Err(invalid(
                    "Repository or file rules changed since replacement preview; preview again",
                ));
            }
        }
        fs::create_dir_all(base)?;
        let base = fs::canonicalize(base)?;
        for id in selected {
            check_cancel(cancel)?;
            self.check_replacement(&plan.changes[id], cancel)?;
            reject_overlap(&base, &plan.changes[id].root)?;
        }
        let run = base.join(&plan.view.id);
        fs::create_dir(&run)?;
        crate::sync::flush_directory(&base)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&run, fs::Permissions::from_mode(0o700))?;
        }
        fs::create_dir(run.join("files"))?;
        let journal = run.join("intent.json");
        let intent = serde_json::json!({"version":1,"operation":"source_replace","plan":plan.view,"selected":selected,"targets":selected.iter().map(|id|{let c=&plan.changes[id];serde_json::json!({"id":id,"root_native":crate::native_path::encode(&c.root),"relative_native":crate::native_path::encode(&c.file.relative),"original_digest":c.original.view.digest,"saved_digest":c.digest,"object":c.object,"backup_directory":format!("files/{id}")})}).collect::<Vec<_>>()});
        durable::write(&journal, &intent, 16 * 1024 * 1024)?;
        let mut result = SourceReplaceResult {
            status: "completed".into(),
            journal: journal.to_string_lossy().into(),
            outcomes: Vec::new(),
            updated_documents: Vec::new(),
            issue: None,
        };
        progress(SourceReplaceProgress {
            plan_id: plan.view.id.clone(),
            processed: 0,
            total: selected.len(),
            path: String::new(),
        });
        for (index, file_id) in selected.iter().enumerate() {
            if check_cancel(cancel).is_err() {
                result.status = "cancelled".into();
                break;
            }
            let change = &plan.changes[file_id];
            let backup = run.join("files").join(file_id);
            let saved = (|| {
                self.check_replacement(change, cancel)?;
                fs::create_dir(&backup)?;
                crate::sync::flush_directory(&run.join("files"))?;
                durable::write(
                    &run.join(format!("{index:04}-before.json")),
                    &serde_json::json!({"id":file_id,"state":"applying"}),
                    4096,
                )?;
                editing::save_with_options(
                    &change.original,
                    &change.text,
                    &backup,
                    &SaveOptions {
                        line_ending: plan.view.request.line_ending.clone(),
                        ..SaveOptions::default()
                    },
                )
            })();
            let mut outcome = SourceReplaceOutcome {
                id: file_id.clone(),
                status: "completed".into(),
                backup_directory: backup.is_dir().then(|| backup.to_string_lossy().into()),
                issue: None,
            };
            match saved {
                Ok((original, saved)) => {
                    let observed = Object::observe(&change.root, &change.file.relative);
                    if let Ok(object) = observed
                        && original.location_valid()
                    {
                        for (id, slot) in self.source_documents.iter_mut().filter(|(_, doc)| {
                            doc.file.repository == change.file.repository
                                && doc.file.relative == change.file.relative
                        }) {
                            let mut updated = original.clone();
                            updated.view.id = id.clone();
                            updated.view.path = crate::tree::display_path(&change.file.relative);
                            slot.original = updated;
                            slot.object = object.clone();
                            slot.dirty = false;
                            result.updated_documents.push(WorkspaceDocument {
                                repository_id: slot.file.repository.clone(),
                                file_id: self
                                    .file_caps
                                    .iter()
                                    .find(|(_, cap)| {
                                        cap.repository == slot.file.repository
                                            && cap.relative == slot.file.relative
                                    })
                                    .map(|(id, _)| id.clone())
                                    .unwrap_or_default(),
                                document: slot.original.view.clone(),
                            });
                        }
                    } else {
                        outcome.status = "uncertain".into();
                        outcome.issue = Some(
                            "Replacement file changed or could not be inspected after saving"
                                .into(),
                        );
                    }
                    if saved.document.digest != change.digest {
                        outcome.status = "uncertain".into();
                        outcome.issue =
                            Some("Saved replacement digest differs from the preview".into());
                    }
                }
                Err(error) => {
                    outcome.status = "failed_or_uncertain".into();
                    outcome.issue = Some(error.to_string());
                }
            }
            if let Err(error) = durable::write(
                &run.join(format!("{index:04}-after.json")),
                &outcome,
                128 * 1024,
            ) {
                outcome.status = "uncertain".into();
                outcome.issue = Some(format!("Could not record replacement result: {error}"));
            }
            let failed = outcome.status != "completed";
            if failed {
                result.issue = outcome.issue.clone();
                result.status = if result.outcomes.is_empty() {
                    "failed"
                } else {
                    "partial"
                }
                .into();
            }
            result.outcomes.push(outcome);
            progress(SourceReplaceProgress {
                plan_id: plan.view.id.clone(),
                processed: result.outcomes.len(),
                total: selected.len(),
                path: crate::tree::display_path(&change.file.relative),
            });
            if failed {
                break;
            }
        }
        if let Err(error) = durable::write(
            &run.join("result.json"),
            &serde_json::json!({"status":result.status,"journal":result.journal,"outcomes":result.outcomes,"issue":result.issue}),
            16 * 1024 * 1024,
        ) {
            result.issue = Some(format!("Replacement result record failed: {error}"));
            result.status = "uncertain".into();
        }
        Ok(result)
    }
}
