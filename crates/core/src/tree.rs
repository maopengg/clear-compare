//! Snapshot-wide filtering, directory navigation and descendant status aggregation.
use crate::{Error, Result, compare::Snapshot};
use clear_contracts::{
    Breadcrumb, FolderPage, FolderQuery, FolderSort, Kind, Page, Row, Status, Summary,
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
pub fn display_path(path: &Path) -> String {
    #[cfg(windows)]
    {
        path.to_string_lossy().replace('\\', "/")
    }
    #[cfg(not(windows))]
    {
        path.to_string_lossy().into()
    }
}
pub fn is_directory(row: &Row) -> bool {
    row.left.as_ref().is_some_and(|e| e.kind == Kind::Directory)
        || row
            .right
            .as_ref()
            .is_some_and(|e| e.kind == Kind::Directory)
}
fn add(summary: &mut Summary, status: &Status) {
    match status {
        Status::Same => summary.same += 1,
        Status::Changed => summary.changed += 1,
        Status::LeftOnly => summary.left_only += 1,
        Status::RightOnly => summary.right_only += 1,
        Status::MetadataMatch => summary.unverified += 1,
        _ => summary.errors += 1,
    }
}
pub fn aggregate(rows: &mut [Row], paths: &BTreeMap<String, PathBuf>) {
    let mut summaries = BTreeMap::<PathBuf, (Summary, bool, u128, u128)>::new();
    for row in rows.iter() {
        let Some(path) = paths.get(&row.id) else {
            continue;
        };
        let count =
            !is_directory(row) || matches!(row.status, Status::Error | Status::TypeConflict);
        let different = matches!(
            row.status,
            Status::Changed
                | Status::LeftOnly
                | Status::RightOnly
                | Status::Error
                | Status::TypeConflict
        );
        let file_bytes = |entry: &Option<clear_contracts::Entry>| {
            entry
                .as_ref()
                .filter(|e| e.kind == Kind::File)
                .and_then(|e| e.bytes.parse::<u128>().ok())
                .unwrap_or_default()
        };
        let left_bytes = file_bytes(&row.left);
        let right_bytes = file_bytes(&row.right);
        for parent in path.ancestors().skip(1) {
            if parent.as_os_str().is_empty() {
                break;
            }
            let item = summaries.entry(parent.to_path_buf()).or_default();
            if count {
                add(&mut item.0, &row.status);
            }
            item.1 |= different;
            item.2 += left_bytes;
            item.3 += right_bytes;
        }
    }
    for row in rows.iter_mut().filter(|r| is_directory(r)) {
        let Some(path) = paths.get(&row.id) else {
            continue;
        };
        let (summary, different, left_bytes, right_bytes) =
            summaries.remove(path).unwrap_or_default();
        if let Some(e) = row.left.as_mut().filter(|e| e.kind == Kind::Directory) {
            e.bytes = left_bytes.to_string();
        }
        if let Some(e) = row.right.as_mut().filter(|e| e.kind == Kind::Directory) {
            e.bytes = right_bytes.to_string();
        }
        if row.status == Status::Same {
            row.status = if summary.errors > 0 {
                Status::Error
            } else if different {
                Status::Changed
            } else if summary.unverified > 0 {
                Status::MetadataMatch
            } else {
                Status::Same
            };
        }
        row.directory_summary = Some(summary);
    }
}
fn rank(status: &Status) -> u8 {
    match status {
        Status::Error => 0,
        Status::TypeConflict => 1,
        Status::Changed => 2,
        Status::LeftOnly => 3,
        Status::RightOnly => 4,
        Status::MetadataMatch => 5,
        Status::Same => 6,
    }
}
fn size(row: &Row, left: bool) -> u128 {
    (if left { &row.left } else { &row.right })
        .as_ref()
        .and_then(|e| e.bytes.parse().ok())
        .unwrap_or_default()
}
pub fn query(snapshot: &Snapshot, request: &FolderQuery, offset: usize) -> Result<FolderPage> {
    if request.search.len() > 4096 {
        return Err(Error::Invalid("Search exceeds 4096 bytes".into()));
    }
    let parent = match &request.parent_id {
        None => Path::new(""),
        Some(id) => {
            let row = snapshot
                .report
                .rows
                .iter()
                .find(|r| &r.id == id)
                .ok_or_else(|| Error::Invalid("Unknown directory".into()))?;
            if !is_directory(row) {
                return Err(Error::Invalid("Choose a directory entry".into()));
            }
            snapshot
                .paths
                .get(id)
                .ok_or_else(|| Error::Invalid("Unknown directory path".into()))?
                .as_path()
        }
    };
    let search = request.search.to_lowercase();
    let mut rows = snapshot
        .report
        .rows
        .iter()
        .filter(|row| {
            let Some(path) = snapshot.paths.get(&row.id) else {
                return false;
            };
            let in_scope = if request.recursive {
                path != parent && path.starts_with(parent)
            } else {
                path.parent() == Some(parent)
            };
            in_scope
                && (!request.differences_only || row.status != Status::Same)
                && (search.is_empty() || row.path.to_lowercase().contains(&search))
        })
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| {
        let dirs = is_directory(b).cmp(&is_directory(a));
        if !dirs.is_eq() {
            return dirs;
        }
        let order = match request.sort {
            FolderSort::Path => a.path.cmp(&b.path),
            FolderSort::Status => rank(&a.status).cmp(&rank(&b.status)),
            FolderSort::LeftSize => size(a, true).cmp(&size(b, true)),
            FolderSort::RightSize => size(a, false).cmp(&size(b, false)),
        };
        let order = order
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.id.cmp(&b.id));
        if request.descending {
            order.reverse()
        } else {
            order
        }
    });
    let total = rows.len();
    let page = Page {
        total,
        rows: rows.into_iter().skip(offset).take(500).cloned().collect(),
    };
    let mut breadcrumbs = parent
        .ancestors()
        .filter(|p| !p.as_os_str().is_empty())
        .filter_map(|p| {
            snapshot.directory_index.get(p).map(|id| Breadcrumb {
                id: id.clone(),
                label: p
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            })
        })
        .collect::<Vec<_>>();
    breadcrumbs.reverse();
    Ok(FolderPage {
        page,
        breadcrumbs,
        outline: BTreeMap::new(),
    })
}

/// Preorder of the visible outline. Filtering precedes collapse and paging;
/// matching descendants retain their ancestors. Native paths never leave core.
pub fn outline(
    snapshot: &Snapshot,
    request: &FolderQuery,
    expanded: &[String],
    expand_all: bool,
    offset: usize,
) -> Result<FolderPage> {
    use clear_contracts::OutlineNode;
    use std::collections::BTreeSet;
    if expanded.len() > crate::compare::MAX_ENTRIES {
        return Err(Error::Invalid("Too many expanded directories".into()));
    }
    let rows = &snapshot.report.rows;
    let ids: BTreeMap<_, _> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| (r.id.as_str(), i))
        .collect();
    let mut opened = BTreeSet::new();
    for id in expanded {
        let i = ids
            .get(id.as_str())
            .ok_or_else(|| Error::Invalid("Unknown expanded directory".into()))?;
        if !is_directory(&rows[*i]) {
            return Err(Error::Invalid("Expanded entry is not a directory".into()));
        }
        opened.insert(*i);
    }
    let search = request.search.to_lowercase();
    let matches: BTreeSet<_> = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| {
            (!request.differences_only || row.status != Status::Same)
                && (search.is_empty() || row.path.to_lowercase().contains(&search))
        })
        .map(|(i, _)| i)
        .collect();
    let mut included = matches.clone();
    for i in &matches {
        if let Some(path) = snapshot.paths.get(&rows[*i].id) {
            for parent in path
                .ancestors()
                .skip(1)
                .filter(|p| !p.as_os_str().is_empty())
            {
                if let Some(id) = snapshot.directory_index.get(parent) {
                    included.insert(ids[id.as_str()]);
                }
            }
        }
    }
    let mut children: BTreeMap<&Path, Vec<usize>> = BTreeMap::new();
    for i in &included {
        if let Some(path) = snapshot.paths.get(&rows[*i].id) {
            children
                .entry(path.parent().unwrap_or(Path::new("")))
                .or_default()
                .push(*i);
        }
    }
    for siblings in children.values_mut() {
        siblings.sort_by(|a, b| {
            let (a, b) = (&rows[*a], &rows[*b]);
            let dirs = is_directory(b).cmp(&is_directory(a));
            if !dirs.is_eq() {
                return dirs;
            }
            let order = match request.sort {
                FolderSort::Path => a.path.cmp(&b.path),
                FolderSort::Status => rank(&a.status).cmp(&rank(&b.status)),
                FolderSort::LeftSize => size(a, true).cmp(&size(b, true)),
                FolderSort::RightSize => size(a, false).cmp(&size(b, false)),
            }
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.id.cmp(&b.id));
            if request.descending {
                order.reverse()
            } else {
                order
            }
        });
    }
    let mut stack = children
        .get(Path::new(""))
        .into_iter()
        .flatten()
        .rev()
        .map(|i| (*i, 0usize))
        .collect::<Vec<_>>();
    let mut total = 0usize;
    let mut visible = Vec::new();
    let mut nodes = BTreeMap::new();
    while let Some((i, depth)) = stack.pop() {
        let row = &rows[i];
        let path = &snapshot.paths[&row.id];
        let nested = children.get(path.as_path());
        let has_children = nested.is_some_and(|c| !c.is_empty());
        let expanded = has_children && (!search.is_empty() || (expand_all != opened.contains(&i)));
        if total >= offset && visible.len() < 500 {
            nodes.insert(
                row.id.clone(),
                OutlineNode {
                    name: path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    depth,
                    expanded,
                    has_children,
                    context_only: !matches.contains(&i),
                },
            );
            visible.push(row.clone());
        }
        total += 1;
        if expanded && let Some(children) = nested {
            stack.extend(children.iter().rev().map(|child| (*child, depth + 1)));
        }
    }
    Ok(FolderPage {
        page: Page {
            total,
            rows: visible,
        },
        breadcrumbs: Vec::new(),
        outline: nodes,
    })
}

#[cfg(test)]
mod load_tests {
    use super::*;
    use clear_contracts::{Comparison, Entry};
    #[test]
    fn large_directory_query_keeps_small_folder_replies_bounded() {
        // Synthetic captured metadata: no real directory scanning or file I/O.
        let mut rows = Vec::new();
        let mut paths = BTreeMap::new();
        let mut directory_index = BTreeMap::new();
        for (i, name) in [
            "project",
            "project/.venv",
            "project/.venv/bin",
            "project/.venv/lib",
        ]
        .iter()
        .enumerate()
        {
            let id = format!("d{i}");
            let path = PathBuf::from(name);
            directory_index.insert(path.clone(), id.clone());
            paths.insert(id.clone(), path);
            rows.push(Row {
                id,
                path: (*name).into(),
                directory_summary: None,
                status: Status::LeftOnly,
                left: Some(Entry {
                    kind: Kind::Directory,
                    bytes: "0".into(),
                    modified_ms: None,
                    digest: None,
                    error: None,
                }),
                right: None,
            });
        }
        for i in 0..105_128 {
            let name = if i < 128 {
                format!("project/.venv/bin/tool-{i:06}")
            } else {
                format!("project/.venv/lib/package-{i:06}.py")
            };
            let id = format!("f{i}");
            paths.insert(id.clone(), PathBuf::from(&name));
            rows.push(Row {
                id,
                path: name,
                directory_summary: None,
                status: Status::LeftOnly,
                left: Some(Entry {
                    kind: Kind::File,
                    bytes: "128".into(),
                    modified_ms: None,
                    digest: None,
                    error: None,
                }),
                right: None,
            });
        }
        let snapshot = Snapshot {
            restart: None,
            report: Comparison {
                policy: None,
                session: "load-test".into(),
                left: "/sample/left".into(),
                right: "/sample/right".into(),
                summary: Summary::default(),
                rows,
                warnings: Vec::new(),
            },
            left: PathBuf::from("/sample/left"),
            right: PathBuf::from("/sample/right"),
            paths,
            directory_index,
            rules: crate::rules::Rules::parse("").unwrap(),
        };
        let mut request = FolderQuery {
            parent_id: None,
            recursive: true,
            search: String::new(),
            differences_only: true,
            sort: FolderSort::Path,
            descending: false,
        };
        let started = std::time::Instant::now();
        let root = query(&snapshot, &request, 0).unwrap();
        eprintln!("105132-entry root query: {:?}", started.elapsed());
        assert_eq!(root.page.total, 105_132);
        assert_eq!(root.page.rows.len(), 500);
        request.parent_id = Some("d2".into());
        request.recursive = false;
        let started = std::time::Instant::now();
        let bin = query(&snapshot, &request, 0).unwrap();
        eprintln!(
            "128-entry bin query in 105132-entry snapshot: {:?}",
            started.elapsed()
        );
        assert_eq!(bin.page.total, 128);
        assert_eq!(bin.page.rows.len(), 128);
        assert_eq!(
            bin.breadcrumbs
                .iter()
                .map(|b| b.label.as_str())
                .collect::<Vec<_>>(),
            ["project", ".venv", "bin"]
        );
        assert!(
            bin.page
                .rows
                .iter()
                .all(|r| r.path.starts_with("project/.venv/bin/"))
        );
    }
}
