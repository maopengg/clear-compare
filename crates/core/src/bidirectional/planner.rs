use super::*;
struct Record {
    id: String,
    path: PathBuf,
    choice: Option<Side>,
}
pub struct Plan {
    pub view: BidirectionalPreview,
    pub(super) snapshot: Arc<Snapshot>,
    pub(super) baseline_path: PathBuf,
    pub(super) hash: String,
    pub(super) baseline: Baseline,
    pub(super) desired: Inventory,
    pub(super) operations: Vec<SyncPlan>,
    records: Vec<Record>,
    left: Inventory,
    right: Inventory,
    base: Inventory,
    left_hash: BTreeMap<PathBuf, String>,
    right_hash: BTreeMap<PathBuf, String>,
    base_hash: BTreeMap<PathBuf, String>,
    protected: [BTreeSet<PathBuf>; 2],
    pub(super) backup: PathBuf,
    pub(super) run_root: PathBuf,
    rows: Vec<BidirectionalRow>,
}
fn value(node: Option<&Node>) -> Option<BidirectionalValue> {
    node.map(|n| match n {
        Node::Directory => BidirectionalValue {
            kind: "directory".into(),
            digest: None,
        },
        Node::File { digest } => BidirectionalValue {
            kind: "file".into(),
            digest: Some(digest.clone()),
        },
    })
}
fn fingerprints(map: &Inventory) -> Result<BTreeMap<PathBuf, String>> {
    use sha2::{Digest, Sha256};
    let mut children: BTreeMap<PathBuf, Vec<PathBuf>> = BTreeMap::new();
    for path in map.keys() {
        children
            .entry(path.parent().unwrap_or(Path::new("")).to_owned())
            .or_default()
            .push(path.clone());
    }
    let mut sorted = map.keys().collect::<Vec<_>>();
    sorted.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
    let mut hashes = BTreeMap::<PathBuf, String>::new();
    for path in sorted {
        let mut hash = Sha256::new();
        hash.update(serde_json::to_vec(&map[path])?);
        for child in children.get(path).into_iter().flatten() {
            hash.update(serde_json::to_vec(&native_path::encode(Path::new(
                child.file_name().unwrap(),
            )))?);
            hash.update(hashes[child].as_bytes());
        }
        hashes.insert(path.clone(), format!("{:x}", hash.finalize()));
    }
    Ok(hashes)
}
fn protected(root: &Path, map: &Inventory, cancel: &AtomicBool) -> Result<BTreeSet<PathBuf>> {
    let mut out = BTreeSet::new();
    let mut visited = 0;
    for (path, node) in map {
        if *node != Node::Directory {
            continue;
        }
        for child in fs::read_dir(crate::compare::checked_path(root, path, false)?)? {
            check_cancel(cancel)?;
            visited += 1;
            if visited > crate::compare::MAX_ENTRIES {
                return Err(invalid("Excluded-child preflight exceeds 200000 entries"));
            }
            let child = path.join(child?.file_name());
            if !map.contains_key(&child) {
                for ancestor in path.ancestors().filter(|p| !p.as_os_str().is_empty()) {
                    out.insert(ancestor.to_owned());
                }
            }
        }
    }
    Ok(out)
}
pub fn prepare(
    snapshot: Arc<Snapshot>,
    path: &Path,
    backup: &Path,
    cancel: &AtomicBool,
) -> Result<Plan> {
    let path = baseline::canonical(path, false)?;
    baseline::outside(&path, &snapshot)?;
    let _lock = baseline::lock(&path, false)?;
    let (base, hash) = baseline::read(&path)?;
    roots(&base, &snapshot)?;
    sync::revalidate_scope(&snapshot, cancel)?;
    let backup = fs::canonicalize(backup)?;
    if !backup.is_dir() || backup.to_str().is_none() {
        return Err(invalid(
            "Bidirectional backup base must be an existing Unicode directory",
        ));
    }
    crate::compare::reject_overlap(&backup, &snapshot.left)?;
    crate::compare::reject_overlap(&backup, &snapshot.right)?;
    if path.starts_with(backup.join(".bidirectional")) {
        return Err(invalid(
            "Baseline cannot be inside bidirectional run history",
        ));
    }
    let left = side(&snapshot, true)?;
    let right = side(&snapshot, false)?;
    let original = inventory(&base.entries)?;
    let paths = left
        .keys()
        .chain(right.keys())
        .chain(original.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    let ids = snapshot
        .paths
        .iter()
        .map(|(id, p)| (p.clone(), id.clone()))
        .collect::<BTreeMap<_, _>>();
    let records = paths
        .into_iter()
        .map(|path| Record {
            id: ids.get(&path).cloned().unwrap_or_else(unique_id),
            path,
            choice: None,
        })
        .collect();
    let id = unique_id();
    let run = backup.join(".bidirectional").join(&id);
    let view = BidirectionalPreview {
        can_prefer_left: false,
        can_prefer_right: false,
        id,
        baseline: baseline::info(&path, &base),
        backup_root: run.to_string_lossy().into(),
        rows: Vec::new(),
        total: 0,
        conflicts: 0,
        blocked: 0,
        actions: 0,
    };
    let protected = [
        protected(&snapshot.left, &left, cancel)?,
        protected(&snapshot.right, &right, cancel)?,
    ];
    let mut plan = Plan {
        view,
        snapshot,
        baseline_path: path,
        hash,
        baseline: base,
        desired: BTreeMap::new(),
        operations: Vec::new(),
        records,
        left_hash: fingerprints(&left)?,
        right_hash: fingerprints(&right)?,
        base_hash: fingerprints(&original)?,
        left,
        right,
        base: original,
        protected,
        backup,
        run_root: run,
        rows: Vec::new(),
    };
    plan.refresh(cancel)?;
    Ok(plan)
}
impl Plan {
    pub fn session(&self) -> &str {
        &self.snapshot.report.session
    }
    pub fn affected_paths(&self) -> Result<Vec<PathBuf>> {
        self.operations
            .iter()
            .try_fold(Vec::new(), |mut out, plan| {
                out.extend(sync::affected_paths(&self.snapshot, plan)?);
                Ok(out)
            })
    }
    pub fn page(&self, offset: usize) -> Vec<BidirectionalRow> {
        self.rows.iter().skip(offset).take(500).cloned().collect()
    }
    pub fn resolve(&mut self, id: &str, winner: Side, cancel: &AtomicBool) -> Result<()> {
        let row = self
            .rows
            .iter()
            .find(|r| r.id == id)
            .ok_or_else(|| invalid("Unknown conflict"))?;
        if (winner == Side::Left && !row.can_left)
            || (winner == Side::Right && !row.can_right)
            || !matches!(row.state.as_str(), "conflict" | "resolved")
        {
            return Err(invalid(
                "This branch cannot use that winner; type conflicts require external resolution",
            ));
        }
        let index = self
            .records
            .iter()
            .position(|r| r.id == id)
            .ok_or_else(|| invalid("Conflict expired"))?;
        let previous = self.records[index].choice.clone();
        self.records[index].choice = Some(winner);
        if let Err(e) = self.refresh(cancel) {
            self.records[index].choice = previous;
            return Err(e);
        }
        Ok(())
    }
    pub fn prefer(&mut self, winner: Side, cancel: &AtomicBool) -> Result<()> {
        let ids = self
            .rows
            .iter()
            .filter(|r| r.state == "conflict")
            .map(|r| {
                if winner == Side::Left && !r.can_left || winner == Side::Right && !r.can_right {
                    Err(invalid("One or more conflicts cannot use that winner"))
                } else {
                    Ok(r.id.clone())
                }
            })
            .collect::<Result<BTreeSet<_>>>()?;
        let previous = self
            .records
            .iter()
            .map(|r| r.choice.clone())
            .collect::<Vec<_>>();
        for record in &mut self.records {
            if ids.contains(&record.id) {
                record.choice = Some(winner.clone());
            }
        }
        if let Err(e) = self.refresh(cancel) {
            for (record, old) in self.records.iter_mut().zip(previous) {
                record.choice = old;
            }
            return Err(e);
        }
        Ok(())
    }
    fn refresh(&mut self, cancel: &AtomicBool) -> Result<()> {
        let mut inherited = BTreeMap::<PathBuf, Option<Side>>::new();
        let mut actions: [Vec<(String, FileOperation)>; 2] = [Vec::new(), Vec::new()];
        let mut rows = Vec::new();
        let mut desired = BTreeMap::new();
        let mut conflicts = 0;
        let mut blocked = 0;
        for record in &self.records {
            check_cancel(cancel)?;
            let path = &record.path;
            let left = self.left.get(path);
            let right = self.right.get(path);
            let base = self.base.get(path);
            let subtree = left == Some(&Node::Directory)
                || right == Some(&Node::Directory)
                || base == Some(&Node::Directory);
            let ancestor = path.ancestors().skip(1).find_map(|p| inherited.get(p));
            if ancestor == Some(&None) {
                rows.push(BidirectionalRow {
                    id: record.id.clone(),
                    baseline: value(base),
                    left: value(left),
                    right: value(right),
                    path: crate::tree::display_path(path),
                    state: "parent_conflict".into(),
                    action: "review_parent_branch".into(),
                    winner: None,
                    can_left: false,
                    can_right: false,
                    subtree,
                });
                continue;
            }
            let mut winner = ancestor.cloned().flatten();
            let mut state = "automatic";
            let mut can_left = true;
            let mut can_right = true;
            if winner.is_none() {
                if left == right {
                    if let Some(node) = left {
                        desired.insert(path.clone(), node.clone());
                    }
                    rows.push(BidirectionalRow {
                        id: record.id.clone(),
                        baseline: value(base),
                        left: value(left),
                        right: value(right),
                        path: crate::tree::display_path(path),
                        state: "common".into(),
                        action: "keep_common".into(),
                        winner: None,
                        can_left: false,
                        can_right: false,
                        subtree,
                    });
                    continue;
                }
                let same_left = if subtree {
                    self.left_hash.get(path) == self.base_hash.get(path)
                } else {
                    left == base
                };
                let same_right = if subtree {
                    self.right_hash.get(path) == self.base_hash.get(path)
                } else {
                    right == base
                };
                winner = if same_left {
                    Some(Side::Right)
                } else if same_right {
                    Some(Side::Left)
                } else {
                    None
                };
                if left.is_some()
                    && right.is_some()
                    && std::mem::discriminant(left.unwrap())
                        != std::mem::discriminant(right.unwrap())
                {
                    state = "type_conflict";
                    blocked += 1;
                    can_left = false;
                    can_right = false;
                    winner = None;
                }
                if left.is_none()
                    && right == Some(&Node::Directory)
                    && self.protected[1].contains(path)
                {
                    can_left = false;
                    if winner == Some(Side::Left) {
                        winner = None;
                    }
                }
                if right.is_none()
                    && left == Some(&Node::Directory)
                    && self.protected[0].contains(path)
                {
                    can_right = false;
                    if winner == Some(Side::Right) {
                        winner = None;
                    }
                }
                if state != "type_conflict" && (winner.is_none() || record.choice.is_some()) {
                    winner = record.choice.clone().filter(|s| {
                        if *s == Side::Left {
                            can_left
                        } else {
                            can_right
                        }
                    });
                    if winner.is_none() {
                        state = "conflict";
                        conflicts += 1;
                    } else {
                        state = "resolved";
                    }
                }
            }
            let mut action = "review_conflict";
            if let Some(winner) = winner.clone() {
                let (source, target, index) = if winner == Side::Left {
                    (left, right, 1)
                } else {
                    (right, left, 0)
                };
                if source != target {
                    let operation = match (source, target) {
                        (Some(Node::File { .. }), None | Some(Node::File { .. })) => {
                            FileOperation::Copy
                        }
                        (Some(Node::Directory), None) => FileOperation::CreateDirectory,
                        (None, Some(Node::File { .. })) => FileOperation::Delete,
                        (None, Some(Node::Directory)) => FileOperation::RemoveDirectory,
                        _ => return Err(invalid("Unsupported bidirectional type replacement")),
                    };
                    action = match operation {
                        FileOperation::Copy => "copy",
                        FileOperation::Delete => "delete",
                        FileOperation::CreateDirectory => "create_directory",
                        FileOperation::RemoveDirectory => "remove_directory",
                        _ => unreachable!(),
                    };
                    actions[index].push((record.id.clone(), operation));
                } else {
                    action = "keep_common";
                }
                if let Some(node) = source {
                    desired.insert(path.clone(), node.clone());
                }
                if subtree && (left != Some(&Node::Directory) || right != Some(&Node::Directory)) {
                    inherited.insert(path.clone(), Some(winner));
                }
            } else if subtree {
                inherited.insert(path.clone(), None);
            }
            rows.push(BidirectionalRow {
                id: record.id.clone(),
                baseline: value(base),
                left: value(left),
                right: value(right),
                path: crate::tree::display_path(path),
                state: state.into(),
                action: action.into(),
                winner,
                can_left,
                can_right,
                subtree,
            });
        }
        let mut operations = Vec::new();
        if conflicts == 0 && blocked == 0 {
            inventory(&store(&desired))?;
            for (index, mut items) in actions.into_iter().enumerate() {
                if items.is_empty() {
                    continue;
                }
                items.sort_by(|a, b| {
                    let rank = |x: &(String, FileOperation)| {
                        let path = &self.snapshot.paths[&x.0];
                        match x.1 {
                            FileOperation::CreateDirectory => (0, path.components().count()),
                            FileOperation::Copy => (1, 0),
                            FileOperation::Delete => (2, 0),
                            FileOperation::RemoveDirectory => {
                                (3, usize::MAX - path.components().count())
                            }
                            _ => (4, 0),
                        }
                    };
                    rank(a)
                        .cmp(&rank(b))
                        .then_with(|| self.snapshot.paths[&a.0].cmp(&self.snapshot.paths[&b.0]))
                });
                operations.push(sync::plan_reconcile(
                    &self.snapshot,
                    &items,
                    &self.backup,
                    if index == 0 { Side::Left } else { Side::Right },
                    cancel,
                )?);
            }
        }
        let rank = |r: &BidirectionalRow| match r.state.as_str() {
            "type_conflict" => 0,
            "conflict" => 1,
            "resolved" => 2,
            "automatic" => 3,
            "parent_conflict" => 4,
            _ => 5,
        };
        rows.sort_by(|a, b| rank(a).cmp(&rank(b)).then(a.path.cmp(&b.path)));
        self.view.can_prefer_left = conflicts > 0
            && blocked == 0
            && rows
                .iter()
                .filter(|r| r.state == "conflict")
                .all(|r| r.can_left);
        self.view.can_prefer_right = conflicts > 0
            && blocked == 0
            && rows
                .iter()
                .filter(|r| r.state == "conflict")
                .all(|r| r.can_right);
        self.operations = operations;
        self.desired = desired;
        self.view.id = unique_id();
        self.view.conflicts = conflicts;
        self.view.blocked = blocked;
        self.view.actions = rows
            .iter()
            .filter(|r| {
                matches!(
                    r.action.as_str(),
                    "copy" | "delete" | "create_directory" | "remove_directory"
                )
            })
            .count();
        self.view.total = rows.len();
        self.view.rows = rows.iter().take(500).cloned().collect();
        self.rows = rows;
        Ok(())
    }
}
