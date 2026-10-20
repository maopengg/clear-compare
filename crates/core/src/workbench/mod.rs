//! Persistent multi-repository workspace. Discovery and inspection never fetch or mutate Git.
mod actions;
mod branches;
mod comparison;
mod conflicts;
mod creation;
mod file_history;
mod file_ops;
mod files;
mod history;
mod organization;
pub mod rebase_editor;
mod registration;
mod remotes;
mod replace;
mod revision;
mod runner;
mod stashes;
mod status;
mod submodules;
mod working;
mod worktrees;
use crate::{
    Error, Result,
    compare::{canonical_root, check_cancel},
};
use clear_contracts::workbench::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::atomic::AtomicBool,
};
use walkdir::WalkDir;
#[derive(Clone)]
pub struct Captured {
    pub root: PathBuf,
    pub git: PathBuf,
    pub common: PathBuf,
    pub view: Repository,
    pub files: BTreeMap<String, Vec<PathBuf>>,
    root_identity: Option<crate::sync::durable::Object>,
    git_identity: Option<crate::sync::durable::Object>,
}
#[derive(Default)]
pub struct Workspace {
    pub catalog: Catalog,
    pub repositories: BTreeMap<String, Captured>,
    ignored: BTreeSet<String>,
    manual: BTreeSet<String>,
    registration: Option<registration::Prepared>,
    creation: Option<creation::Prepared>,
    pending: Option<actions::Prepared>,
    history_details: BTreeMap<String, history::CapturedDetail>,
    file_caps: BTreeMap<String, files::FileCapability>,
    source_documents: BTreeMap<String, files::Document>,
    file_plan: Option<file_ops::Prepared>,
    replace_plan: Option<replace::Prepared>,
    revision_trees: BTreeMap<String, revision::CapturedTree>,
    file_histories: BTreeMap<String, file_history::CapturedHistory>,
    working_diff: Option<working::CapturedDiff>,
    conflict_documents: BTreeMap<String, conflicts::Guard>,
    stash_entries: BTreeMap<String, (String, StashEntry)>,
    worktree_entries: BTreeMap<String, (String, worktrees::Entry)>,
    worktree_destinations: BTreeMap<String, worktrees::Destination>,
    submodule_entries: BTreeMap<String, (String, submodules::Entry)>,
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    #[serde(default)]
    organization: RepositoryOrganization,
    #[serde(default)]
    manual: BTreeSet<String>,
    #[serde(default)]
    reference_settings: BTreeMap<String, ReferenceSettings>,
    #[serde(default)]
    file_settings: BTreeMap<String, FileSettings>,
    version: u32,
    settings: ScanSettings,
    entries: Vec<Repository>,
    #[serde(default)]
    ignored: BTreeSet<String>,
}
pub(crate) fn id(value: &[u8]) -> String {
    format!("{:x}", Sha256::digest(value))
}
fn path_id(path: &Path) -> String {
    id(format!("{:?}", crate::native_path::encode(path)).as_bytes())
}
// Missing descendants still inherit an existing ancestor's canonical spelling
// (notably /var -> /private/var on macOS).
fn scan_location(input: &str) -> PathBuf {
    let mut cursor = PathBuf::from(input);
    let mut tail = Vec::new();
    loop {
        if let Ok(mut resolved) = fs::canonicalize(&cursor) {
            for part in tail.iter().rev() {
                resolved.push(part);
            }
            return resolved;
        }
        let Some(name) = cursor.file_name().map(|s| s.to_os_string()) else {
            return PathBuf::from(input);
        };
        tail.push(name);
        if !cursor.pop() {
            return PathBuf::from(input);
        }
    }
}
pub fn remote_key(url: &str) -> Option<String> {
    let raw = url.trim().trim_end_matches('/').trim_end_matches(".git");
    if raw.is_empty() {
        return None;
    }
    if let Some((scheme, rest)) = raw.split_once("://") {
        if !["https", "http", "ssh", "git"].contains(&scheme) {
            return (runner::redact(raw) == raw).then(|| raw.to_owned());
        }
        let rest = rest
            .split(['?', '#'])
            .next()
            .unwrap_or(rest)
            .trim_end_matches('/')
            .trim_end_matches(".git");
        let (authority, path) = rest.split_once('/')?;
        let host = authority
            .rsplit_once('@')
            .map(|(_, r)| r)
            .unwrap_or(authority);
        let port = match scheme {
            "https" => ":443",
            "http" => ":80",
            "ssh" => ":22",
            _ => ":9418",
        };
        let host = host.strip_suffix(port).unwrap_or(host);
        return Some(format!(
            "{}/{}",
            host.to_lowercase(),
            path.trim_start_matches('/')
        ));
    }
    if !raw.starts_with('/')
        && let Some((host, path)) = raw.split_once(':')
    {
        let host = host.rsplit_once('@').map(|(_, h)| h).unwrap_or(host);
        if !host.contains('/') && !path.is_empty() {
            return Some(format!(
                "{}/{}",
                host.to_lowercase(),
                path.trim_start_matches('/')
            ));
        }
    }
    Some(format!("local:{raw}"))
}
fn validate(settings: &ScanSettings) -> Result<()> {
    if settings.roots.len() > 64
        || settings.exclusions.len() > 256
        || settings
            .roots
            .iter()
            .any(|p| p.len() > 8192 || p.contains('\0'))
        || settings
            .exclusions
            .iter()
            .any(|e| e.is_empty() || e.len() > 512 || e.contains(['\0', '/', '\\']))
    {
        return Err(Error::Invalid("Invalid workspace scan settings".into()));
    }
    Ok(())
}
impl Workspace {
    pub fn load(base: &Path) -> Result<Self> {
        let file = base.join("git-workspace.json");
        if !file.exists() {
            return Ok(Self::default());
        }
        let meta = fs::symlink_metadata(&file)?;
        if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 4 * 1024 * 1024 {
            return Err(Error::Invalid("Invalid Git workspace settings file".into()));
        }
        let stored: Stored = serde_json::from_slice(&fs::read(file)?)?;
        if stored.version != 1 || stored.entries.len() > 4096 {
            return Err(Error::Invalid("Unsupported Git workspace settings".into()));
        }
        validate(&stored.settings)?;
        registration::validate_manual(&stored.manual)?;
        organization::validate(&stored.organization)?;
        let mut catalog = Catalog {
            organization: stored.organization,
            reference_settings: stored.reference_settings,
            settings: stored.settings,
            file_settings: stored.file_settings,
            ..Catalog::default()
        };
        for mut entry in stored.entries {
            entry.available = false;
            entry.error = None;
            catalog.repositories.push(entry);
        }
        Ok(Self {
            catalog,
            repositories: BTreeMap::new(),
            ignored: stored.ignored,
            manual: stored.manual,
            registration: None,
            creation: None,
            pending: None,
            history_details: BTreeMap::new(),
            file_caps: BTreeMap::new(),
            source_documents: BTreeMap::new(),
            file_plan: None,
            replace_plan: None,
            revision_trees: BTreeMap::new(),
            file_histories: BTreeMap::new(),
            working_diff: None,
            conflict_documents: BTreeMap::new(),
            stash_entries: BTreeMap::new(),
            worktree_entries: BTreeMap::new(),
            worktree_destinations: BTreeMap::new(),
            submodule_entries: BTreeMap::new(),
        })
    }
    pub fn save(&self, base: &Path) -> Result<()> {
        fs::create_dir_all(base)?;
        let file = base.join("git-workspace.json");
        if file.exists() && !fs::symlink_metadata(&file)?.is_file() {
            return Err(Error::Invalid(
                "Workspace settings path is not a regular file".into(),
            ));
        }
        if fs::symlink_metadata(&file).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(Error::Invalid(
                "Workspace settings must not be a symbolic link".into(),
            ));
        }
        let stored = Stored {
            organization: self.catalog.organization.clone(),
            manual: self.manual.clone(),
            reference_settings: self.catalog.reference_settings.clone(),
            file_settings: self.catalog.file_settings.clone(),
            version: 1,
            settings: self.catalog.settings.clone(),
            entries: self.catalog.repositories.clone(),
            ignored: self.ignored.clone(),
        };
        let bytes = serde_json::to_vec_pretty(&stored)?;
        if bytes.len() > 4 * 1024 * 1024 {
            return Err(Error::Invalid("Workspace settings exceed 4 MiB".into()));
        }
        let mut tmp = tempfile::NamedTempFile::new_in(base)?;
        tmp.write_all(&bytes)?;
        tmp.as_file().sync_all()?;
        tmp.persist(file).map_err(|e| e.error)?;
        #[cfg(unix)]
        std::fs::File::open(base)?.sync_all()?;
        Ok(())
    }
    pub fn scan(&mut self, settings: ScanSettings, cancel: &AtomicBool) -> Result<Catalog> {
        validate(&settings)?;
        let mut next = Workspace::default();
        next.catalog.settings = settings.clone();
        next.catalog.organization = self.catalog.organization.clone();
        next.catalog.file_settings = self.catalog.file_settings.clone();
        next.catalog.reference_settings = self.catalog.reference_settings.clone();
        next.file_caps = self.file_caps.clone();
        next.source_documents = self.source_documents.clone();
        next.conflict_documents = self.conflict_documents.clone();
        next.ignored = self.ignored.clone();
        next.manual = self.manual.clone();
        let mut seen = BTreeSet::new();
        let mut visited = 0;
        for (input, manual) in settings
            .roots
            .iter()
            .map(|p| (p, false))
            .chain(self.manual.iter().map(|p| (p, true)))
        {
            check_cancel(cancel)?;
            let root = match canonical_root(input) {
                Ok(r) => r,
                Err(e) => {
                    next.catalog.warnings.push(format!("{input}: {e}"));
                    continue;
                }
            };
            let walker = WalkDir::new(&root)
                .follow_links(false)
                .max_depth(if manual { 0 } else { 64 })
                .into_iter()
                .filter_entry(|e| {
                    e.depth() == 0
                        || e.file_name() != ".git"
                            && !settings
                                .exclusions
                                .iter()
                                .any(|s| e.file_name() == s.as_str())
                });
            for item in walker {
                check_cancel(cancel)?;
                let item = match item {
                    Ok(e) => e,
                    Err(e) => {
                        if next.catalog.warnings.len() < 128 {
                            next.catalog.warnings.push(e.to_string());
                        }
                        continue;
                    }
                };
                if !item.file_type().is_dir() {
                    continue;
                }
                visited += 1;
                if item.depth() == 64 {
                    next.catalog.truncated = true;
                }
                if visited > 250_000 || next.repositories.len() >= 4096 {
                    next.catalog.truncated = true;
                    break;
                }
                let dir = item.path();
                let marker = dir.join(".git");
                let ordinary = fs::symlink_metadata(&marker)
                    .is_ok_and(|m| !m.file_type().is_symlink() && (m.is_file() || m.is_dir()));
                let bare = dir.join("HEAD").is_file()
                    && dir.join("objects").is_dir()
                    && dir.join("refs").is_dir();
                if !ordinary && !bare {
                    continue;
                }
                let canonical = fs::canonicalize(dir)?;
                if next.ignored.contains(&path_id(&canonical)) || !seen.insert(canonical.clone()) {
                    continue;
                }
                match inspect(&canonical, cancel) {
                    Ok(mut repo) => {
                        if let Some(old) = self
                            .catalog
                            .repositories
                            .iter()
                            .find(|r| r.id == repo.view.id)
                        {
                            repo.view.favorite = old.favorite;
                            repo.view.last_opened = old.last_opened.clone();
                        }
                        if let Err(e) = status::summary(&mut repo, cancel) {
                            repo.view.error = Some(e.to_string());
                        }
                        next.repositories.insert(repo.view.id.clone(), repo);
                    }
                    Err(e) => {
                        if matches!(e, Error::Cancelled) {
                            return Err(e);
                        }
                        if next.catalog.warnings.len() < 128 {
                            next.catalog
                                .warnings
                                .push(format!("{}: {e}", dir.display()));
                        }
                    }
                }
            }
        }
        next.catalog.scanned_directories = visited;
        next.catalog.repositories = next.repositories.values().map(|r| r.view.clone()).collect();
        next.catalog
            .repositories
            .sort_by(|a, b| a.name.cmp(&b.name).then(a.path.cmp(&b.path)));
        let covered_roots: Vec<_> = settings.roots.iter().map(|r| scan_location(r)).collect();
        for old in &self.catalog.repositories {
            if !next.repositories.contains_key(&old.id)
                && (self.manual.contains(&old.path)
                    || covered_roots
                        .iter()
                        .any(|r| Path::new(&old.path).starts_with(r)))
            {
                let mut old = old.clone();
                old.available = false;
                old.error = Some("Repository missing or unavailable during scan".into());
                next.catalog.repositories.push(old);
            }
        }
        self.retain_source_roots(&mut next);
        *self = next;
        Ok(self.catalog.clone())
    }
    pub fn open(&mut self, repository_id: &str, cancel: &AtomicBool) -> Result<RepositoryState> {
        if !self.repositories.contains_key(repository_id) {
            let old = self
                .catalog
                .repositories
                .iter()
                .find(|r| r.id == repository_id)
                .ok_or_else(|| Error::Invalid("Unknown repository".into()))?;
            let mut captured = inspect(&canonical_root(&old.path)?, cancel)?;
            if captured.view.id != repository_id {
                return Err(Error::Invalid(
                    "Repository identity changed; scan again".into(),
                ));
            }
            captured.view.favorite = old.favorite;
            self.repositories.insert(repository_id.into(), captured);
        }
        let repo = self
            .repositories
            .get_mut(repository_id)
            .ok_or_else(|| Error::Invalid("Repository unavailable".into()))?;
        let current_git = git_directory(&repo.root, &["rev-parse", "--absolute-git-dir"], cancel)?;
        let current_common = git_directory(
            &repo.root,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            cancel,
        )?;
        if current_git != repo.git
            || current_common != repo.common
            || (repo.view.kind != "bare"
                && git_directory(&repo.root, &["rev-parse", "--show-toplevel"], cancel)?
                    != repo.root)
        {
            return Err(Error::Invalid(
                "Git repository mapping changed; scan again before continuing".into(),
            ));
        }
        let mut result = status::state(repo, cancel)?;
        result.repository.last_opened = format!(
            "{:020}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );
        repo.view = result.repository.clone();
        if let Some(old) = self
            .catalog
            .repositories
            .iter_mut()
            .find(|r| r.id == repository_id)
        {
            *old = repo.view.clone();
        }
        self.record_reference_use(repository_id, &mut result);
        Ok(result)
    }
    pub fn favorite(&mut self, repository_id: &str, value: bool) -> Result<Catalog> {
        let entry = self
            .catalog
            .repositories
            .iter_mut()
            .find(|r| r.id == repository_id)
            .ok_or_else(|| Error::Invalid("Unknown repository".into()))?;
        entry.favorite = value;
        if let Some(repo) = self.repositories.get_mut(repository_id) {
            repo.view.favorite = value;
        }
        Ok(self.catalog.clone())
    }
    pub fn remove(&mut self, repository_id: &str) -> Result<Catalog> {
        if self.open_sources_in(repository_id) {
            return Err(Error::Invalid(
                "Close this repository's source tabs before removing it".into(),
            ));
        }
        if !self
            .catalog
            .repositories
            .iter()
            .any(|r| r.id == repository_id)
        {
            return Err(Error::Invalid("Unknown repository".into()));
        }
        self.catalog.repositories.retain(|r| r.id != repository_id);
        self.manual
            .retain(|path| path_id(Path::new(path)) != repository_id);
        self.repositories.remove(repository_id);
        self.ignored.insert(repository_id.to_owned());
        Ok(self.catalog.clone())
    }
}
// Git for Windows prints drive/slash paths while canonical_root uses verbatim
// paths. Compare actual directories in one representation, not their spelling.
fn git_directory(root: &Path, args: &[&str], cancel: &AtomicBool) -> Result<PathBuf> {
    let path = PathBuf::from(runner::text(root, args, cancel)?);
    #[cfg(windows)]
    let path = fs::canonicalize(path)?;
    Ok(path)
}
fn inspect(root: &Path, cancel: &AtomicBool) -> Result<Captured> {
    let git = git_directory(root, &["rev-parse", "--absolute-git-dir"], cancel)?;
    let common = git_directory(
        root,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        cancel,
    )?;
    let bare = runner::text(root, &["rev-parse", "--is-bare-repository"], cancel)? == "true";
    if !bare && git_directory(root, &["rev-parse", "--show-toplevel"], cancel)? != root {
        return Err(Error::Invalid("Expected repository root".into()));
    }
    let kind = if bare {
        "bare"
    } else if git != common {
        "worktree"
    } else if git.components().any(|c| c.as_os_str() == "modules") {
        "submodule"
    } else {
        "repository"
    };
    let remotes = status::remotes(root, cancel)?;
    let match_key = remotes
        .iter()
        .find(|r| r.name == "origin")
        .or(remotes.first())
        .and_then(|r| r.match_key.clone());
    let view = Repository {
        id: path_id(root),
        name: root
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into(),
        path: root.to_string_lossy().into(),
        kind: kind.into(),
        common_id: path_id(&common),
        match_key,
        branch: String::new(),
        head: None,
        changed: 0,
        conflicts: 0,
        ahead: 0,
        behind: 0,
        favorite: false,
        last_opened: String::new(),
        available: true,
        error: None,
    };
    let root_identity = crate::sync::durable::Object::observe(root, Path::new(""))?;
    let git_identity = crate::sync::durable::Object::observe(&git, Path::new(""))?;
    Ok(Captured {
        files: BTreeMap::new(),
        root_identity,
        git_identity,
        root: root.into(),
        git,
        common,
        view,
    })
}
