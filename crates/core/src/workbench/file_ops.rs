//! One-use, identity-pinned file operations with verified backups and durable journals.
use super::*;
use crate::{
    compare::{checked_path, hash_file, reject_overlap},
    sync::durable::{self, Object},
};
use std::io::{Read, Write};
#[derive(Clone, PartialEq, Eq)]
struct Pin {
    object: Object,
    digest: Option<String>,
}
pub(super) struct Prepared {
    view: SourceFilePlan,
    action: SourceFileAction,
    repository: String,
    relative: PathBuf,
    root: PathBuf,
    root_object: Option<Object>,
    manifest: BTreeMap<PathBuf, Pin>,
    target: Option<Target>,
}
struct Target {
    repository: String,
    root: PathBuf,
    relative: PathBuf,
    parent_object: Option<Object>,
    root_object: Option<Object>,
}
fn at(base: &Path, relative: &Path) -> PathBuf {
    if relative.as_os_str().is_empty() {
        base.to_path_buf()
    } else {
        base.join(relative)
    }
}
fn invalid(message: &str) -> Error {
    Error::Invalid(message.into())
}
fn manifest(path: &Path, cancel: &AtomicBool) -> Result<BTreeMap<PathBuf, Pin>> {
    let mut result = BTreeMap::new();
    let parent = path
        .parent()
        .ok_or_else(|| invalid("File parent unavailable"))?;
    for entry in WalkDir::new(path).follow_links(false).max_depth(128) {
        check_cancel(cancel)?;
        let entry = entry.map_err(|e| invalid(&e.to_string()))?;
        if result.len() >= 50_000 {
            return Err(invalid("File operation exceeds 50000 entries"));
        }
        if entry
            .file_name()
            .to_string_lossy()
            .eq_ignore_ascii_case(".git")
        {
            return Err(invalid(
                "Folders containing Git metadata cannot be moved or copied by file operations",
            ));
        }
        let relative = entry
            .path()
            .strip_prefix(path)
            .map_err(|_| invalid("Invalid file operation path"))?
            .to_path_buf();
        let object = Object::observe(
            parent,
            entry
                .path()
                .strip_prefix(parent)
                .map_err(|_| invalid("Invalid source path"))?,
        )?
        .ok_or_else(|| invalid("File disappeared"))?;
        let digest = if object.kind == clear_contracts::Kind::File {
            Some(hash_file(entry.path(), cancel)?)
        } else {
            None
        };
        if entry.depth() == 128 && entry.file_type().is_dir() {
            return Err(invalid("File operation exceeds directory depth limit"));
        }
        result.insert(relative, Pin { object, digest });
    }
    Ok(result)
}
fn same_copy(expected: &BTreeMap<PathBuf, Pin>, actual: &BTreeMap<PathBuf, Pin>) -> bool {
    expected.len() == actual.len()
        && expected.iter().all(|(path, pin)| {
            actual.get(path).is_some_and(|p| {
                p.digest == pin.digest
                    && p.object.kind == pin.object.kind
                    && p.object.permissions == pin.object.permissions
            })
        })
}
fn copy_tree(
    source: &Path,
    target: &Path,
    pins: &BTreeMap<PathBuf, Pin>,
    cancel: &AtomicBool,
) -> Result<()> {
    for (relative, pin) in pins {
        check_cancel(cancel)?;
        let from = at(source, relative);
        let to = at(target, relative);
        if pin.object.kind == clear_contracts::Kind::Directory {
            fs::create_dir(&to)?;
        } else {
            let mut input = fs::File::open(&from)?;
            let mut output = fs::File::create_new(&to)?;
            let mut buffer = [0u8; 65536];
            loop {
                check_cancel(cancel)?;
                let n = input.read(&mut buffer)?;
                if n == 0 {
                    break;
                }
                output.write_all(&buffer[..n])?;
            }
            fs::set_permissions(&to, fs::metadata(&from)?.permissions())?;
            output.sync_all()?;
        }
    }
    for (relative, pin) in pins.iter().rev() {
        if pin.object.kind == clear_contracts::Kind::Directory {
            fs::set_permissions(
                at(target, relative),
                fs::metadata(at(source, relative))?.permissions(),
            )?;
        }
    }
    #[cfg(unix)]
    for (relative, pin) in pins.iter().rev() {
        if pin.object.kind == clear_contracts::Kind::Directory {
            fs::File::open(at(target, relative))?.sync_all()?;
        }
    }
    sync_parent(target)?;
    if !same_copy(pins, &manifest(target, cancel)?) {
        return Err(invalid("Copied contents failed verification"));
    }
    Ok(())
}
fn sync_parent(_path: &Path) -> Result<()> {
    #[cfg(unix)]
    fs::File::open(_path.parent().ok_or_else(|| invalid("Parent missing"))?)?.sync_all()?;
    Ok(())
}
impl Workspace {
    pub fn source_file_exclude(
        &mut self,
        repository: &str,
        file: &str,
        base: &Path,
        cancel: &AtomicBool,
    ) -> Result<FileSettings> {
        self.source_file_path(repository, file, cancel)?;
        let cap = self.file_cap(repository, file, None)?;
        let mut settings = self
            .catalog
            .file_settings
            .get(repository)
            .cloned()
            .unwrap_or_default();
        settings.exclusions = crate::rules::exclude_path(
            &cap.relative,
            cap.kind == "directory",
            &settings.exclusions,
        )?;
        self.file_settings_persisted(repository, settings, base, cancel)
    }

    pub fn source_file_path(
        &mut self,
        repository: &str,
        file: &str,
        cancel: &AtomicBool,
    ) -> Result<PathBuf> {
        let root = self.source_root(repository, cancel)?;
        let file = self.file_cap(repository, file, None)?;
        checked_path(&root, &file.relative, false)
    }
    pub fn source_file_info(
        &mut self,
        repository: &str,
        file: &str,
        cancel: &AtomicBool,
    ) -> Result<SourceFileInfo> {
        let path = self.source_file_path(repository, file, cancel)?;
        let cap = self.file_cap(repository, file, None)?;
        Ok(SourceFileInfo {
            name: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into(),
            relative_path: crate::tree::display_path(&cap.relative),
            absolute_path: path.to_string_lossy().into(),
        })
    }
    pub fn source_file_prepare(
        &mut self,
        repository: &str,
        file: &str,
        action: SourceFileAction,
        cancel: &AtomicBool,
    ) -> Result<SourceFilePlan> {
        self.file_plan = None;
        let root = self.source_root(repository, cancel)?;
        let cap = self.file_cap(repository, file, None)?;
        if !matches!(cap.kind.as_str(), "file" | "directory") {
            return Err(invalid("File operation requires a regular file or folder"));
        }
        if !matches!(action, SourceFileAction::Copy { .. })
            && self.open_sources_under(repository, &cap.relative)
        {
            return Err(invalid(
                "Close open files inside this item before moving, renaming or deleting it; unsaved buffers are retained",
            ));
        }
        let source = checked_path(&root, &cap.relative, false)?;
        let pins = manifest(&source, cancel)?;
        let mut bytes = 0u64;
        let mut files = 0;
        for (relative, pin) in &pins {
            if pin.digest.is_some() {
                files += 1;
                bytes = bytes
                    .checked_add(fs::metadata(at(&source, relative))?.len())
                    .ok_or_else(|| invalid("Byte count overflow"))?;
            }
        }
        let (operation, target) = match &action {
            SourceFileAction::Trash => ("trash", None),
            SourceFileAction::Rename { name } => {
                if name.eq_ignore_ascii_case(".git") {
                    return Err(invalid("Git metadata is protected"));
                }
                crate::native_path::single_name(std::ffi::OsStr::new(name))?;
                let relative = cap.relative.parent().unwrap_or(Path::new("")).join(name);
                (
                    "rename",
                    Some((repository.to_string(), root.clone(), relative)),
                )
            }
            SourceFileAction::Copy {
                repository,
                directory,
            }
            | SourceFileAction::Move {
                repository,
                directory,
            } => {
                let destination = self.source_root(repository, cancel)?;
                let parent = directory
                    .as_deref()
                    .map(|id| {
                        self.file_cap(repository, id, Some("directory"))
                            .map(|f| f.relative)
                    })
                    .transpose()?
                    .unwrap_or_default();
                let relative = parent.join(
                    cap.relative
                        .file_name()
                        .ok_or_else(|| invalid("Source name missing"))?,
                );
                (
                    if matches!(action, SourceFileAction::Copy { .. }) {
                        "copy"
                    } else {
                        "move"
                    },
                    Some((repository.clone(), destination, relative)),
                )
            }
        };
        let target = target
            .map(|(repository, target_root, relative)| -> Result<Target> {
                let destination = checked_path(&target_root, &relative, true)?;
                if destination == source
                    || destination.starts_with(&source)
                    || source.starts_with(&destination)
                {
                    return Err(invalid("Source and destination overlap"));
                }
                if fs::symlink_metadata(&destination).is_ok() {
                    return Err(invalid(
                        "Destination already exists; choose another folder or name",
                    ));
                }
                let parent = relative
                    .parent()
                    .ok_or_else(|| invalid("Target parent missing"))?;
                let parent_object = Object::observe(&target_root, parent)?;
                if parent_object
                    .as_ref()
                    .is_none_or(|p| p.kind != clear_contracts::Kind::Directory)
                {
                    return Err(invalid("Target parent is not a folder"));
                }
                let root_object = Object::observe(&target_root, Path::new(""))?;
                Ok(Target {
                    repository,
                    root: target_root,
                    relative,
                    parent_object,
                    root_object,
                })
            })
            .transpose()?;
        let view = SourceFilePlan {
            id: crate::unique_id(),
            operation: operation.into(),
            source: source.to_string_lossy().into(),
            destination: target
                .as_ref()
                .map(|t| t.root.join(&t.relative).to_string_lossy().into()),
            files,
            directories: pins.len() - files,
            bytes: bytes.to_string(),
        };
        self.file_plan = Some(Prepared {
            view: view.clone(),
            action,
            repository: repository.into(),
            relative: cap.relative,
            root_object: Object::observe(&root, Path::new(""))?,
            root,
            manifest: pins,
            target,
        });
        Ok(view)
    }
    pub fn source_file_apply(
        &mut self,
        id: &str,
        base: &Path,
        cancel: &AtomicBool,
        recycle: impl FnOnce(&Path) -> Result<()>,
    ) -> Result<SourceFileResult> {
        let plan = self
            .file_plan
            .take()
            .filter(|p| p.view.id == id)
            .ok_or_else(|| invalid("File plan expired; prepare again"))?;
        if !matches!(plan.action, SourceFileAction::Copy { .. })
            && self.open_sources_under(&plan.repository, &plan.relative)
        {
            return Err(invalid(
                "Close open files inside this item before applying the operation",
            ));
        }
        let root = self.source_root(&plan.repository, cancel)?;
        if root != plan.root || Object::observe(&root, Path::new(""))? != plan.root_object {
            return Err(invalid("Source root changed"));
        }
        let source = checked_path(&root, &plan.relative, false)?;
        if manifest(&source, cancel)? != plan.manifest {
            return Err(invalid("Source changed after preview; prepare again"));
        }
        fs::create_dir_all(base)?;
        let base = fs::canonicalize(base)?;
        reject_overlap(&base, &root)?;
        if let Some(target) = &plan.target {
            reject_overlap(&base, &target.root)?;
            self.source_root(&target.repository, cancel)?;
        }
        let backup = base.join(&plan.view.id);
        fs::create_dir(&backup)?;
        let journal = backup.join("journal.json");
        let mut record = serde_json::json!({"version":1,"plan":plan.view,"source_native":crate::native_path::encode(&source),"destination_native":plan.target.as_ref().map(|t|crate::native_path::encode(&t.root.join(&t.relative))),"state":"backing_up"});
        record["manifest"]=plan.manifest.iter().map(|(path,pin)|serde_json::json!({"relative_native":crate::native_path::encode(path),"object":pin.object,"sha256":pin.digest})).collect::<Vec<_>>().into();
        durable::write(&backup.join("intent.json"), &record, 32 * 1024 * 1024)?;
        let result = (|| -> Result<Option<WorkspaceFile>> {
            copy_tree(&source, &backup.join("source"), &plan.manifest, cancel)?;
            record["state"] = "ready".into();
            durable::write(
                &backup.join("backup-verified.json"),
                &serde_json::json!({"id":plan.view.id,"state":"backup_verified"}),
                1024,
            )?;
            if manifest(&source, cancel)? != plan.manifest {
                return Err(invalid("Source changed while backing up"));
            }
            durable::write(
                &backup.join("applying.json"),
                &serde_json::json!({"id":plan.view.id,"state":"applying"}),
                1024,
            )?;
            if let Some(target) = &plan.target {
                if Object::observe(&target.root, Path::new(""))? != target.root_object
                    || Object::observe(
                        &target.root,
                        target.relative.parent().unwrap_or(Path::new("")),
                    )? != target.parent_object
                {
                    return Err(invalid("Destination folder changed"));
                }
                let destination = checked_path(&target.root, &target.relative, true)?;
                if fs::symlink_metadata(&destination).is_ok() {
                    return Err(invalid("Destination was created after preview"));
                }
                if matches!(plan.action, SourceFileAction::Copy { .. }) {
                    let staging = tempfile::Builder::new()
                        .prefix(".clear-compare-copy-")
                        .tempdir_in(
                            destination
                                .parent()
                                .ok_or_else(|| invalid("Target parent missing"))?,
                        )?;
                    let payload = staging.path().join("payload");
                    copy_tree(&backup.join("source"), &payload, &plan.manifest, cancel)?;
                    check_cancel(cancel)?;
                    crate::no_replace::rename(&payload, &destination)?;
                } else {
                    check_cancel(cancel)?;
                    crate::no_replace::rename(&source, &destination)?;
                    sync_parent(&source)?;
                }
                sync_parent(&destination)?;
                Ok(Some(self.capture_file(
                    &target.repository,
                    &target.root,
                    target.relative.clone(),
                )?))
            } else {
                check_cancel(cancel)?;
                recycle(&source)?;
                sync_parent(&source)?;
                Ok(None)
            }
        })();
        record["state"] = if result.is_ok() {
            "completed"
        } else {
            "failed"
        }
        .into();
        if let Err(error) = &result {
            record["error"] = error.to_string().into();
        }
        durable::write(&journal, &record, 32 * 1024 * 1024)?;
        let entry = result.map_err(|e| invalid(&format!("{e}; journal: {}", journal.display())))?;
        Ok(SourceFileResult {
            journal: journal.to_string_lossy().into(),
            entry,
        })
    }
}
