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
    Ok(FolderPage { page, breadcrumbs })
}
