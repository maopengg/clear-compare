//! Lazy immutable commit trees and bounded blob pages. Never checkout or run filters.
use super::*;
use crate::sync::durable::Object;
use clear_contracts::TextOptions;
use std::io::{Read, Seek, SeekFrom};

#[derive(Clone)]
struct Node {
    view: RevisionEntry,
    relative: PathBuf,
    parent: Option<String>,
}
pub(super) struct CapturedTree {
    repository: String,
    commit: String,
    tree: String,
    nodes: BTreeMap<String, Node>,
    node_bytes: usize,
    directories: BTreeMap<String, Vec<String>>,
    blobs: BTreeMap<String, tempfile::NamedTempFile>,
}
fn invalid(text: &str) -> Error {
    Error::Invalid(text.into())
}
fn oid(text: &str) -> bool {
    matches!(text.len(), 40 | 64) && text.bytes().all(|b| b.is_ascii_hexdigit())
}
fn root(workspace: &Workspace, repository: &str, cancel: &AtomicBool) -> Result<PathBuf> {
    check_cancel(cancel)?;
    let repo = workspace
        .repositories
        .get(repository)
        .ok_or_else(|| invalid("Historical repository is unavailable; reopen the revision"))?;
    if Object::observe(&repo.root, Path::new(""))? != repo.root_identity
        || Object::observe(&repo.git, Path::new(""))? != repo.git_identity
    {
        return Err(invalid(
            "Historical repository identity changed; scan again",
        ));
    }
    let git = runner::text(&repo.root, &["rev-parse", "--absolute-git-dir"], cancel)?;
    if fs::canonicalize(git)? != repo.git {
        return Err(invalid("Historical repository mapping changed; scan again"));
    }
    Ok(repo.root.clone())
}
fn verify_blob(file: &mut fs::File, object: &str, size: u64, cancel: &AtomicBool) -> Result<()> {
    use sha2::Digest;
    fn digest<D: Digest>(file: &mut fs::File, size: u64, cancel: &AtomicBool) -> Result<String> {
        let mut hash = D::new();
        hash.update(format!("blob {size}\0").as_bytes());
        file.seek(SeekFrom::Start(0))?;
        let mut buffer = [0; 65536];
        loop {
            check_cancel(cancel)?;
            let length = file.read(&mut buffer)?;
            if length == 0 {
                break;
            }
            hash.update(&buffer[..length]);
        }
        Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
    }
    let actual = if object.len() == 40 {
        digest::<sha1::Sha1>(file, size, cancel)?
    } else {
        digest::<sha2::Sha256>(file, size, cancel)?
    };
    if actual != object {
        return Err(invalid(
            "Historical blob content does not match its object identity",
        ));
    }
    Ok(())
}
fn blob(
    tree: &mut CapturedTree,
    root: &Path,
    node: &Node,
    cancel: &AtomicBool,
) -> Result<fs::File> {
    if !matches!(node.view.kind.as_str(), "file" | "link") {
        return Err(invalid(
            "Select a historical file or link to view its bytes",
        ));
    }
    if !tree.blobs.contains_key(&node.view.id) {
        let size = node
            .view
            .bytes
            .parse::<u64>()
            .map_err(|_| invalid("Invalid historical blob size"))?;
        let mut captured = runner::blob(root, &node.view.object, size, cancel)?;
        verify_blob(captured.as_file_mut(), &node.view.object, size, cancel)?;
        if tree.blobs.len() >= 2 {
            tree.blobs.clear();
        }
        tree.blobs.insert(node.view.id.clone(), captured);
    }
    Ok(tree.blobs[&node.view.id].reopen()?)
}
impl Workspace {
    fn revision_cache_room(&mut self, repository: &str, session: &str, file: &str) -> Result<()> {
        let tree = self
            .revision_trees
            .get(session)
            .filter(|tree| tree.repository == repository)
            .ok_or_else(|| invalid("Historical tree expired; reopen the revision"))?;
        let node = tree
            .nodes
            .get(file)
            .ok_or_else(|| invalid("Unknown historical file"))?;
        if tree.blobs.contains_key(file) {
            return Ok(());
        }
        let size = node
            .view
            .bytes
            .parse::<u64>()
            .map_err(|_| invalid("Invalid historical blob size"))?;
        let used = self
            .revision_trees
            .values()
            .flat_map(|tree| tree.blobs.values())
            .try_fold(0u64, |total, file| {
                Ok::<_, Error>(total.saturating_add(file.as_file().metadata()?.len()))
            })?;
        if used.saturating_add(size) > 512 * 1024 * 1024 {
            for tree in self.revision_trees.values_mut() {
                tree.blobs.clear();
            }
        }
        Ok(())
    }
    pub fn revision_open(
        &mut self,
        repository: &str,
        commit: &str,
        cancel: &AtomicBool,
    ) -> Result<RevisionTree> {
        if !oid(commit) {
            return Err(invalid("Select a full commit identity from history"));
        }
        if !self.repositories.contains_key(repository) {
            self.open(repository, cancel)?;
        }
        let root = root(self, repository, cancel)?;
        let actual = runner::text(
            &root,
            &[
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("{commit}^{{commit}}"),
            ],
            cancel,
        )?;
        if actual != commit {
            return Err(invalid("Historical commit identity changed"));
        }
        let tree = runner::text(
            &root,
            &[
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("{commit}^{{tree}}"),
            ],
            cancel,
        )?;
        if !oid(&tree) {
            return Err(invalid("Malformed historical tree identity"));
        }
        if self.revision_trees.len() >= 4 {
            self.revision_trees.clear();
        }
        let id = crate::unique_id();
        self.revision_trees.insert(
            id.clone(),
            CapturedTree {
                repository: repository.into(),
                commit: commit.into(),
                tree,
                nodes: BTreeMap::new(),
                node_bytes: 0,
                directories: BTreeMap::new(),
                blobs: BTreeMap::new(),
            },
        );
        match self.revision_tree(repository, &id, None, 0, "", cancel) {
            Ok(page) => Ok(page),
            Err(error) => {
                self.revision_trees.remove(&id);
                Err(error)
            }
        }
    }
    pub fn revision_tree(
        &mut self,
        repository: &str,
        session: &str,
        directory: Option<&str>,
        offset: usize,
        filter: &str,
        cancel: &AtomicBool,
    ) -> Result<RevisionTree> {
        if filter.len() > 1024 || filter.contains('\0') {
            return Err(invalid("Invalid historical filename filter"));
        }
        let root = root(self, repository, cancel)?;
        let tree = self
            .revision_trees
            .get_mut(session)
            .filter(|t| t.repository == repository)
            .ok_or_else(|| invalid("Historical tree expired; reopen the revision"))?;
        let (object, relative, parent) = if let Some(id) = directory {
            let node = tree
                .nodes
                .get(id)
                .filter(|n| n.view.kind == "directory")
                .ok_or_else(|| invalid("Unknown historical directory"))?;
            (
                node.view.object.clone(),
                node.relative.clone(),
                node.parent.clone(),
            )
        } else {
            (tree.tree.clone(), PathBuf::new(), None)
        };
        let key = directory.unwrap_or("").to_string();
        if !tree.directories.contains_key(&key) {
            let raw = runner::run(&root, &["ls-tree", "-z", "-l", &object, "--"], cancel)?;
            if !raw.is_empty() && raw.last() != Some(&0) {
                return Err(invalid("Truncated historical tree listing"));
            }
            let mut nodes = Vec::new();
            let mut node_bytes = tree.node_bytes;
            for record in raw.split(|b| *b == 0).filter(|r| !r.is_empty()) {
                check_cancel(cancel)?;
                if tree.nodes.len() + nodes.len() >= 50_000 {
                    return Err(invalid("Historical tree exceeds 50000 captured entries"));
                }
                let tab = record
                    .iter()
                    .position(|b| *b == b'\t')
                    .ok_or_else(|| invalid("Malformed historical tree record"))?;
                let fields = std::str::from_utf8(&record[..tab])
                    .map_err(|_| invalid("Invalid historical tree header"))?
                    .split_ascii_whitespace()
                    .collect::<Vec<_>>();
                if fields.len() != 4 || !oid(fields[2]) {
                    return Err(invalid("Invalid historical tree metadata"));
                }
                let kind = match (fields[0], fields[1]) {
                    ("040000", "tree") => "directory",
                    ("100644" | "100755", "blob") => "file",
                    ("120000", "blob") => "link",
                    ("160000", "commit") => "submodule",
                    _ => return Err(invalid("Unsupported historical file mode")),
                };
                let name = &record[tab + 1..];
                if name.contains(&b'/') {
                    return Err(invalid("Historical directory returned a nested filename"));
                }
                let native = crate::git::path_from_bytes(name)?;
                let path = relative.join(native);
                if path.components().count() > 128 || path.as_os_str().len() > 16384 {
                    return Err(invalid("Historical path exceeds the depth or length limit"));
                }
                node_bytes =
                    node_bytes.saturating_add(path.as_os_str().len() * 4 + name.len() * 3 + 512);
                if node_bytes > 64 * 1024 * 1024 {
                    return Err(invalid(
                        "Historical tree exceeds the captured name size limit",
                    ));
                }
                let bytes = if matches!(kind, "directory" | "submodule") && fields[3] == "-" {
                    0
                } else {
                    fields[3]
                        .parse::<u64>()
                        .map_err(|_| invalid("Invalid historical object size"))?
                };
                nodes.push(Node {
                    view: RevisionEntry {
                        id: crate::unique_id(),
                        name: String::from_utf8_lossy(name).into(),
                        path: crate::tree::display_path(&path),
                        kind: kind.into(),
                        bytes: bytes.to_string(),
                        mode: fields[0].into(),
                        object: fields[2].into(),
                    },
                    relative: path,
                    parent: directory.map(str::to_string),
                });
            }
            nodes.sort_by(|a, b| {
                (a.view.kind != "directory", &a.view.name, &a.view.id).cmp(&(
                    b.view.kind != "directory",
                    &b.view.name,
                    &b.view.id,
                ))
            });
            tree.directories.insert(
                key.clone(),
                nodes.iter().map(|n| n.view.id.clone()).collect(),
            );
            tree.node_bytes = node_bytes;
            tree.nodes
                .extend(nodes.into_iter().map(|n| (n.view.id.clone(), n)));
        }
        let filter = filter.to_lowercase();
        let entries = tree.directories[&key]
            .iter()
            .filter_map(|id| tree.nodes.get(id))
            .filter(|n| n.view.name.to_lowercase().contains(&filter))
            .collect::<Vec<_>>();
        if offset > entries.len() {
            return Err(invalid("Invalid historical tree page"));
        }
        Ok(RevisionTree {
            id: session.into(),
            repository_id: repository.into(),
            commit: tree.commit.clone(),
            directory: directory.map(str::to_string),
            parent,
            path: crate::tree::display_path(&relative),
            entries: entries
                .iter()
                .skip(offset)
                .take(300)
                .map(|n| n.view.clone())
                .collect(),
            offset,
            total: entries.len(),
        })
    }
    pub fn revision_content(
        &mut self,
        repository: &str,
        session: &str,
        file: &str,
        mode: &str,
        offset: usize,
        cancel: &AtomicBool,
    ) -> Result<RevisionContent> {
        if !matches!(mode, "auto" | "text" | "hex") {
            return Err(invalid("Invalid historical content mode"));
        }
        let root = root(self, repository, cancel)?;
        self.revision_cache_room(repository, session, file)?;
        let tree = self
            .revision_trees
            .get_mut(session)
            .filter(|t| t.repository == repository)
            .ok_or_else(|| invalid("Historical tree expired; reopen the revision"))?;
        let node = tree
            .nodes
            .get(file)
            .cloned()
            .ok_or_else(|| invalid("Unknown historical file"))?;
        let size = node
            .view
            .bytes
            .parse::<usize>()
            .map_err(|_| invalid("Invalid historical blob size"))?;
        let mut file = blob(tree, &root, &node, cancel)?;
        let text = if mode != "hex" && size <= crate::editing::MAX_DOCUMENT_BYTES {
            let mut bytes = Vec::new();
            file.read_to_end(&mut bytes)?;
            match crate::editing::decode(&bytes) {
                Ok(value) => Some(value),
                Err(error) if mode == "text" => return Err(error),
                Err(_) => None,
            }
        } else if mode == "text" {
            return Err(invalid(
                "Historical text view exceeds 4 MiB; use paged hexadecimal view",
            ));
        } else {
            None
        };
        let (mode, total, next, text, encoding, ending, long_lines) =
            if let Some((text, encoding, _)) = text {
                let ending = crate::editing::endings(&text);
                let lines = crate::text::split_lines(&text);
                if offset > lines.len() {
                    return Err(invalid("Invalid historical text page"));
                }
                let long = lines
                    .iter()
                    .skip(offset)
                    .take(500)
                    .any(|l| l.chars().count() > 8192);
                let page = lines
                    .iter()
                    .skip(offset)
                    .take(500)
                    .map(|l| {
                        l.trim_end_matches(['\r', '\n'])
                            .chars()
                            .take(8192)
                            .collect::<String>()
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                (
                    "text",
                    lines.len(),
                    (offset.saturating_add(500) < lines.len()).then_some(offset + 500),
                    page,
                    Some(encoding),
                    Some(ending),
                    long,
                )
            } else {
                if offset > size || !offset.is_multiple_of(16) {
                    return Err(invalid("Invalid historical byte page"));
                }
                file.seek(SeekFrom::Start(offset as u64))?;
                let mut bytes = Vec::new();
                file.take(4096).read_to_end(&mut bytes)?;
                let mut text = String::new();
                for (i, chunk) in bytes.chunks(16).enumerate() {
                    use std::fmt::Write;
                    let hex = chunk
                        .iter()
                        .map(|b| format!("{b:02X} "))
                        .collect::<String>();
                    let ascii = chunk
                        .iter()
                        .map(|b| {
                            if (32..127).contains(b) {
                                *b as char
                            } else {
                                '.'
                            }
                        })
                        .collect::<String>();
                    let _ = writeln!(text, "{:08X}  {hex:48} {ascii}", offset + i * 16);
                }
                (
                    "hex",
                    size,
                    (offset.saturating_add(4096) < size).then_some(offset + 4096),
                    text,
                    None,
                    None,
                    false,
                )
            };
        check_cancel(cancel)?;
        Ok(RevisionContent {
            file_id: node.view.id,
            path: node.view.path,
            object: node.view.object,
            mode: mode.into(),
            offset,
            total,
            next_offset: next,
            text,
            encoding,
            line_ending: ending,
            long_lines,
        })
    }
    pub fn revision_local_diff(
        &mut self,
        repository: &str,
        session: &str,
        file: &str,
        options: &TextOptions,
        cancel: &AtomicBool,
    ) -> Result<RevisionLocalDiff> {
        let root = root(self, repository, cancel)?;
        self.revision_cache_room(repository, session, file)?;
        if self.repositories[repository].view.kind == "bare" {
            return Err(invalid("Bare repositories have no local working file"));
        }
        let tree = self
            .revision_trees
            .get_mut(session)
            .filter(|t| t.repository == repository)
            .ok_or_else(|| invalid("Historical tree expired; reopen the revision"))?;
        let node = tree
            .nodes
            .get(file)
            .cloned()
            .filter(|n| n.view.kind == "file")
            .ok_or_else(|| invalid("Select a regular historical file to compare locally"))?;
        if node.relative.components().any(|part| {
            part.as_os_str()
                .to_string_lossy()
                .eq_ignore_ascii_case(".git")
        }) {
            return Err(invalid("Git metadata is protected"));
        }
        if node.view.bytes.parse::<u64>().unwrap_or(u64::MAX)
            > crate::editing::MAX_DOCUMENT_BYTES as u64
        {
            return Err(invalid("Historical text comparison exceeds 4 MiB"));
        }
        let mut original = Vec::new();
        blob(tree, &root, &node, cancel)?.read_to_end(&mut original)?;
        let before = crate::editing::decode(&original)?.0;
        let path = crate::compare::checked_path(&root, &node.relative, true)?;
        let local = match fs::symlink_metadata(&path) {
            Ok(meta) if meta.is_file() => Some(crate::editing::open_file(&path)?),
            Ok(_) => return Err(invalid("Local counterpart is not a regular file")),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        let diff = crate::text::compare_strings(
            &node.view.path,
            &before,
            local.as_ref().map(|d| d.view.text.as_str()).unwrap_or(""),
            options,
        )?;
        check_cancel(cancel)?;
        if local.as_ref().is_some_and(|d| !d.location_valid()) {
            return Err(invalid(
                "Local counterpart changed during comparison; compare again",
            ));
        }
        if local.is_none() && path.exists() {
            return Err(invalid(
                "Local counterpart appeared during comparison; compare again",
            ));
        }
        let local_file_id = if local.is_some() {
            Some(self.capture_file(repository, &root, node.relative)?.id)
        } else {
            None
        };
        Ok(RevisionLocalDiff {
            diff,
            local_missing: local.is_none(),
            local_file_id,
        })
    }
}
