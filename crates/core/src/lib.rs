//! Comparison and synchronization domain. Shared by the desktop app and CLI.
pub mod archive;
pub mod bidirectional;
pub mod binary;
pub mod compare;
pub mod editing;
pub mod folder_merge;
pub mod git;
pub mod merge;
pub mod native_path;
mod no_replace;
pub mod picture;
pub mod recovery;
pub mod report;
pub mod rules;
pub mod sessions;
pub mod sync;
pub mod table;
pub mod task;
pub mod text;
pub mod tree;
pub mod tree_snapshot;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(String),
    #[error("Operation cancelled")]
    Cancelled,
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn unique_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        "{now:x}-{:x}-{:x}",
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    )
}
