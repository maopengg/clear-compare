//! Frozen native source bytes for repeatable table format/rule changes.
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
    left: Option<Vec<u8>>,
    right: Option<Vec<u8>>,
}
fn read(path: &Path, expected: Option<&str>, cancel: &AtomicBool) -> Result<Vec<u8>> {
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
    Ok(bytes)
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
fn xlsx_path(path: &Path) -> bool {
    path.extension().and_then(|s| s.to_str()).is_some_and(|e| {
        ["xlsx", "xlsm", "xltx", "xltm"]
            .iter()
            .any(|x| e.eq_ignore_ascii_case(x))
    })
}
impl Document {
    fn new(
        path: String,
        left: Option<Vec<u8>>,
        right: Option<Vec<u8>>,
        hints: [[bool; 3]; 2],
        cancel: &AtomicBool,
    ) -> Result<Self> {
        let xlsx =
            |data: &Option<Vec<u8>>| data.as_ref().is_some_and(|b| b.starts_with(b"PK\x03\x04"));
        let info = TableDocumentInfo {
            left_xlsx: hints[0][2] || xlsx(&left),
            right_xlsx: hints[1][2] || xlsx(&right),
            left_sheets: Vec::new(),
            right_sheets: Vec::new(),
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
                [html_path(left), tsv_path(left), xlsx_path(left)],
                [html_path(right), tsv_path(right), xlsx_path(right)],
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
        let load = |root: &Path, entry: Option<&Entry>| -> Result<Option<Vec<u8>>> {
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
            [[html_path(path), tsv_path(path), xlsx_path(path)]; 2],
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
        for (input, enabled, workbook, catalog, sheets, error) in [
            (
                &self.left,
                left_html,
                info.left_xlsx,
                &mut info.left_tables,
                &mut info.left_sheets,
                &mut info.left_error,
            ),
            (
                &self.right,
                right_html,
                info.right_xlsx,
                &mut info.right_tables,
                &mut info.right_sheets,
                &mut info.right_error,
            ),
        ] {
            catalog.clear();
            sheets.clear();
            *error = None;
            let Some(bytes) = input else {
                continue;
            };
            let inspected = if workbook && !enabled {
                super::xlsx::Document::parse(bytes, cancel).map(|doc| *sheets = doc.sheets)
            } else if enabled {
                editing::decode(bytes)
                    .and_then(|(text, _, _)| super::html::Document::parse(&text, cancel))
                    .map(|doc| *catalog = doc.tables)
            } else {
                Ok(())
            };
            match inspected {
                Ok(()) => {}
                Err(Error::Cancelled) => return Err(Error::Cancelled),
                Err(e) => *error = Some(e.to_string()),
            }
        }
        Ok(info)
    }
    pub fn compare(&self, options: &TableOptions, cancel: &AtomicBool) -> Result<TableComparison> {
        let mut options = options.clone();
        if matches!(options.left_source, TableSource::Auto) {
            options.left_source = if self.info.left_xlsx {
                TableSource::Xlsx { sheet: 0 }
            } else if self.info.left_html {
                TableSource::Html { table: 0 }
            } else if self.info.left_tsv || options.delimiter == "\t" {
                TableSource::Tsv
            } else {
                TableSource::Csv
            };
        }
        if matches!(options.right_source, TableSource::Auto) {
            options.right_source = if self.info.right_xlsx {
                TableSource::Xlsx { sheet: 0 }
            } else if self.info.right_html {
                TableSource::Html { table: 0 }
            } else if self.info.right_tsv || options.delimiter == "\t" {
                TableSource::Tsv
            } else {
                TableSource::Csv
            };
        }
        let (delimiter, tolerance) = super::validated_options(&options)?;
        let mut warnings = Vec::new();
        let mut input = |bytes: Option<&[u8]>,
                         source: &TableSource,
                         side: &str|
         -> Result<super::ParsedInput> {
            let Some(bytes) = bytes else {
                return Ok((Vec::new(), Vec::new()));
            };
            if let TableSource::Xlsx { sheet } = source {
                let (rows, notes) =
                    super::xlsx::Document::parse(bytes, cancel)?.selected(*sheet, cancel)?;
                warnings.extend(notes.into_iter().map(|note| format!("{side}: {note}")));
                Ok((rows, Vec::new()))
            } else {
                let (text, _, _) = editing::decode(bytes)?;
                if matches!(source, TableSource::Html { .. }) {
                    warnings.push("Static HTML text extraction; scripts/styles/templates, rendered CSS, images and form values are not evaluated. Spanned values repeat in covered slots.".into());
                }
                super::input(Some(&text), source, delimiter, cancel)
            }
        };
        let (left, left_tables) = input(self.left.as_deref(), &options.left_source, "Left")?;
        let (right, right_tables) = input(self.right.as_deref(), &options.right_source, "Right")?;
        super::compare_rows(
            left,
            right,
            (left_tables, right_tables, warnings),
            &options,
            tolerance,
            cancel,
        )
    }
}
