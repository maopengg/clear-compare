//! Baseline-aware multi-root reconciliation. Native writes reuse the reviewed sync engine.
mod baseline;
mod execute;
mod planner;
mod recovery;
use crate::{
    Error, Result,
    compare::{Snapshot, check_cancel},
    native_path,
    sync::{self, durable},
    unique_id,
};
pub use baseline::{capture, inspect};
use clear_contracts::*;
pub use execute::{RunResult, execute};
pub use planner::{Plan, prepare};
pub use recovery::{RecoveryPlan, execute_recovery, preview_recovery};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, atomic::AtomicBool},
};
const MAX_BASELINE: u64 = 64 * 1024 * 1024;
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Node {
    Directory,
    File { digest: String },
}
type Inventory = BTreeMap<PathBuf, Node>;
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Stored {
    path: NativePath,
    node: Node,
}
#[derive(PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Baseline {
    version: u32,
    id: String,
    revision: u64,
    last_run: Option<String>,
    left: NativePath,
    right: NativePath,
    left_root: durable::Object,
    right_root: durable::Object,
    exclusions: String,
    entries: Vec<Stored>,
}
fn invalid(message: impl Into<String>) -> Error {
    Error::Invalid(message.into())
}
fn store(entries: &Inventory) -> Vec<Stored> {
    entries
        .iter()
        .map(|(p, n)| Stored {
            path: native_path::encode(p),
            node: n.clone(),
        })
        .collect()
}
fn inventory(entries: &[Stored]) -> Result<Inventory> {
    if entries.len() > crate::compare::MAX_ENTRIES {
        return Err(invalid("Baseline exceeds 200000 entries"));
    }
    let mut out = BTreeMap::new();
    for entry in entries {
        let path = native_path::decode(&entry.path)?;
        if path.as_os_str().is_empty()
            || path
                .components()
                .any(|c| !matches!(c, std::path::Component::Normal(_)))
            || out.insert(path, entry.node.clone()).is_some()
        {
            return Err(invalid("Duplicate or unsafe baseline path"));
        }
        if let Node::File { digest } = &entry.node
            && (digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()))
        {
            return Err(invalid("Invalid baseline digest"));
        }
    }
    for path in out.keys() {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
            && out.get(parent) != Some(&Node::Directory)
        {
            return Err(invalid("Incomplete baseline hierarchy"));
        }
    }
    Ok(out)
}
fn node(entry: &Entry) -> Result<Node> {
    match (&entry.kind, &entry.error, &entry.digest) {
        (Kind::Directory, None, _) => Ok(Node::Directory),
        (Kind::File, None, Some(hash)) => Ok(Node::File {
            digest: hash.clone(),
        }),
        _ => Err(invalid(
            "Bidirectional sync requires readable files/directories and SHA-256 verification; links and errors cannot be reconciled",
        )),
    }
}
fn side(snapshot: &Snapshot, left: bool) -> Result<Inventory> {
    snapshot
        .report
        .rows
        .iter()
        .filter_map(|row| {
            let entry = if left { &row.left } else { &row.right };
            entry.as_ref().map(|e| {
                Ok((
                    snapshot
                        .paths
                        .get(&row.id)
                        .ok_or_else(|| invalid("Missing native bidirectional path"))?
                        .clone(),
                    node(e)?,
                ))
            })
        })
        .collect()
}
fn scan(root: &Path, rules: &crate::rules::Rules, cancel: &AtomicBool) -> Result<Inventory> {
    crate::compare::scan(root, rules, true, cancel)?
        .into_iter()
        .map(|(p, e)| Ok((p, node(&e)?)))
        .collect()
}
fn policy(snapshot: &Snapshot) -> Result<String> {
    snapshot
        .report
        .policy
        .as_ref()
        .filter(|p| matches!(p.verification, VerificationBasis::Sha256))
        .map(|p| p.exclusions.clone())
        .ok_or_else(|| invalid("Bidirectional sync requires content verification"))
}
fn roots(baseline: &Baseline, snapshot: &Snapshot) -> Result<()> {
    if baseline.left_root.identity.volume == baseline.right_root.identity.volume
        && baseline.left_root.identity.index == baseline.right_root.identity.index
    {
        return Err(invalid(
            "Bidirectional roots alias the same native directory",
        ));
    }
    if baseline.version != 1
        || baseline.id.is_empty()
        || baseline.left != native_path::encode(&snapshot.left)
        || baseline.right != native_path::encode(&snapshot.right)
        || baseline.exclusions != policy(snapshot)?
    {
        return Err(invalid(
            "Baseline belongs to different roots, direction or exclusion rules",
        ));
    }
    for (root, expected) in [
        (&snapshot.left, &baseline.left_root),
        (&snapshot.right, &baseline.right_root),
    ] {
        if fs::canonicalize(root)? != *root
            || durable::Object::observe(root, Path::new(""))?.as_ref() != Some(expected)
        {
            return Err(invalid("Baseline root identity/permissions changed"));
        }
    }
    Ok(())
}

fn write_guard<T>(
    base: &Baseline,
    cancel: &AtomicBool,
    work: impl FnOnce() -> Result<T>,
) -> Result<T> {
    let roots = [
        (native_path::decode(&base.left)?, base.left_root.clone()),
        (native_path::decode(&base.right)?, base.right_root.clone()),
    ];
    crate::task::with_mutation_guard(
        cancel,
        move || {
            for (root, pin) in &roots {
                if fs::canonicalize(root)? != *root
                    || durable::Object::observe(root, Path::new(""))?.as_ref() != Some(pin)
                {
                    return Err(invalid(
                        "Bidirectional root identity changed before mutation",
                    ));
                }
            }
            Ok(())
        },
        work,
    )
}
