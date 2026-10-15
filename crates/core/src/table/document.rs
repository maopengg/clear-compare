//! Frozen native source text for repeatable table format/rule changes.
use crate::{
    Error, Result,
    compare::{Snapshot, check_cancel, checked_path},
    editing, unique_id,
};
use clear_contracts::{Entry, Kind, TableComparison, TableDocumentInfo, TableOptions, TableSource};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::Read,
    path::Path,
    sync::atomic::AtomicBool,
};
pub struct Document {
    pub info: TableDocumentInfo,
    left: Option<String>,
    right: Option<String>,
}
fn read(path: &Path, expected: Option<&str>, cancel: &AtomicBool) -> Result<String> {
    check_cancel(cancel)?;
    if !fs::symlink_metadata(path)?.file_type().is_file() {
        return Err(Error::Invalid(
            "Choose regular table files; links are not followed".into(),
        ));
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take((editing::MAX_DOCUMENT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > editing::MAX_DOCUMENT_BYTES {
        return Err(Error::Invalid("Table source exceeds 4 MiB".into()));
    }
    check_cancel(cancel)?;
    if expected.is_some_and(|hash| hash != format!("{:x}", Sha256::digest(&bytes))) {
        return Err(Error::Invalid(
            "Table source changed since folder comparison. Compare folders again.".into(),
        ));
    }
    Ok(editing::decode(&bytes)?.0)
}
fn html_path(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("html") || e.eq_ignore_ascii_case("htm"))
}
fn tsv_path(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("tsv"))
}
impl Document {
    fn new(
        path: String,
        left: Option<String>,
        right: Option<String>,
        hints: [[bool; 2]; 2],
        cancel: &AtomicBool,
    ) -> Result<Self> {
        let info = TableDocumentInfo {
            id: unique_id(),
            path,
            left_html: hints[0][0],
            right_html: hints[1][0],
            left_tsv: hints[0][1],
            right_tsv: hints[1][1],
            left_available: left.is_some(),
            right_available: right.is_some(),
            left_tables: Vec::new(),
            right_tables: Vec::new(),
            left_error: None,
            right_error: None,
        };
        let mut doc = Self { info, left, right };
        doc.info = doc.inspect(hints[0][0], hints[1][0], cancel)?;
        Ok(doc)
    }
    pub fn from_paths(left: &Path, right: &Path, cancel: &AtomicBool) -> Result<Self> {
        Self::new(
            format!("{} ↔ {}", left.display(), right.display()),
            Some(read(left, None, cancel)?),
            Some(read(right, None, cancel)?),
            [
                [html_path(left), tsv_path(left)],
                [html_path(right), tsv_path(right)],
            ],
            cancel,
        )
    }
    pub fn open(snapshot: &Snapshot, id: &str, cancel: &AtomicBool) -> Result<Self> {
        let path = snapshot
            .paths
            .get(id)
            .ok_or_else(|| Error::Invalid("Unknown table entry".into()))?;
        let row = snapshot
            .report
            .rows
            .iter()
            .find(|r| r.id == id)
            .ok_or_else(|| Error::Invalid("Unknown table row".into()))?;
        let load = |root: &Path, entry: Option<&Entry>| -> Result<Option<String>> {
            let Some(e) = entry else {
                return Ok(None);
            };
            if e.kind != Kind::File || e.error.is_some() {
                return Err(Error::Invalid("Select readable regular table files".into()));
            }
            Ok(Some(read(
                &checked_path(root, path, false)?,
                e.digest.as_deref(),
                cancel,
            )?))
        };
        Self::new(
            row.path.clone(),
            load(&snapshot.left, row.left.as_ref())?,
            load(&snapshot.right, row.right.as_ref())?,
            [[html_path(path), tsv_path(path)]; 2],
            cancel,
        )
    }
    pub fn inspect(
        &self,
        left_html: bool,
        right_html: bool,
        cancel: &AtomicBool,
    ) -> Result<TableDocumentInfo> {
        let mut info = self.info.clone();
        for (input, enabled, catalog, error) in [
            (
                &self.left,
                left_html,
                &mut info.left_tables,
                &mut info.left_error,
            ),
            (
                &self.right,
                right_html,
                &mut info.right_tables,
                &mut info.right_error,
            ),
        ] {
            catalog.clear();
            *error = None;
            if let (Some(text), true) = (input, enabled) {
                match super::html::Document::parse(text, cancel) {
                    Ok(doc) => *catalog = doc.tables,
                    Err(Error::Cancelled) => return Err(Error::Cancelled),
                    Err(e) => *error = Some(e.to_string()),
                }
            }
        }
        Ok(info)
    }
    pub fn compare(&self, options: &TableOptions, cancel: &AtomicBool) -> Result<TableComparison> {
        let mut options = options.clone();
        if matches!(options.left_source, TableSource::Auto) {
            options.left_source = if self.info.left_html {
                TableSource::Html { table: 0 }
            } else if self.info.left_tsv || options.delimiter == "\t" {
                TableSource::Tsv
            } else {
                TableSource::Csv
            };
        }
        if matches!(options.right_source, TableSource::Auto) {
            options.right_source = if self.info.right_html {
                TableSource::Html { table: 0 }
            } else if self.info.right_tsv || options.delimiter == "\t" {
                TableSource::Tsv
            } else {
                TableSource::Csv
            };
        }
        super::compare_optional(
            self.left.as_deref(),
            self.right.as_deref(),
            &options,
            cancel,
        )
    }
}
