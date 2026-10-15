//! Strict RFC-style delimited text parsing and explicit row/key comparison.
pub mod document;
pub mod html;
use crate::{Error, Result, editing::MAX_DOCUMENT_BYTES};
use clear_contracts::{Status, Summary, TableComparison, TableOptions, TableRow};
use std::collections::BTreeMap;
pub(crate) const MAX_CELLS: usize = 200_000;
pub(crate) const MAX_ROWS: usize = 30_000;
pub(crate) const MAX_COLUMNS: usize = 1024;

pub fn parse(text: &str, delimiter: char) -> Result<Vec<Vec<String>>> {
    if text.len() > MAX_DOCUMENT_BYTES {
        return Err(Error::Invalid("Table input exceeds 4 MiB".into()));
    }
    if !matches!(delimiter, ',' | '\t') {
        return Err(Error::Invalid("Use comma or tab delimiter".into()));
    }
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let mut chars = text.chars().peekable();
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut cell = String::new();
    // 0 start, 1 unquoted, 2 quoted, 3 closed quoted.
    let mut state = 0;
    let mut cells = 0;
    let mut ended_row = false;
    while let Some(c) = chars.next() {
        ended_row = false;
        if state == 2 {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cell.push('"');
                } else {
                    state = 3;
                }
            } else {
                cell.push(c);
            }
            continue;
        }
        if c == delimiter || matches!(c, '\r' | '\n') {
            row.push(std::mem::take(&mut cell));
            cells += 1;
            state = 0;
            if row.len() > MAX_COLUMNS || cells > MAX_CELLS {
                return Err(Error::Invalid("Table cell/column limit exceeded".into()));
            }
            if c != delimiter {
                if c == '\r' && chars.peek() == Some(&'\n') {
                    chars.next();
                }
                rows.push(std::mem::take(&mut row));
                ended_row = true;
                if rows.len() > MAX_ROWS {
                    return Err(Error::Invalid("Table row limit exceeded".into()));
                }
            }
        } else if c == '"' && state == 0 {
            state = 2;
        } else if c == '"' || state == 3 {
            return Err(Error::Invalid("Malformed table quoting".into()));
        } else {
            cell.push(c);
            state = 1;
        }
    }
    if state == 2 {
        return Err(Error::Invalid("Unclosed quoted table cell".into()));
    }
    if !ended_row {
        row.push(cell);
        cells += 1;
        if row.len() > MAX_COLUMNS || cells > MAX_CELLS {
            return Err(Error::Invalid("Table cell/column limit exceeded".into()));
        }
        rows.push(row);
    }
    if rows.len() > MAX_ROWS {
        return Err(Error::Invalid("Table row limit exceeded".into()));
    }
    Ok(rows)
}
fn keyed(rows: Vec<Vec<String>>, key: Option<usize>) -> Result<BTreeMap<String, Vec<String>>> {
    let mut map = BTreeMap::new();
    for (index, row) in rows.into_iter().enumerate() {
        let label = match key {
            None => format!("{index:08}"),
            Some(column) => row
                .get(column)
                .ok_or_else(|| Error::Invalid("Key column is missing from a row".into()))?
                .clone(),
        };
        if map.insert(label.clone(), row).is_some() {
            return Err(Error::Invalid(format!("Duplicate table key: {label}")));
        }
    }
    Ok(map)
}
fn equal(left: Option<&String>, right: Option<&String>, tolerance: Option<f64>) -> bool {
    if left == right {
        return true;
    }
    let (Some(left), Some(right), Some(tolerance)) = (left, right, tolerance) else {
        return false;
    };
    match (left.trim().parse::<f64>(), right.trim().parse::<f64>()) {
        (Ok(a), Ok(b)) if a.is_finite() && b.is_finite() => (a - b).abs() <= tolerance,
        _ => false,
    }
}
pub fn compare(left: &str, right: &str, options: &TableOptions) -> Result<TableComparison> {
    compare_optional(
        Some(left),
        Some(right),
        options,
        &std::sync::atomic::AtomicBool::new(false),
    )
}
pub fn compare_optional(
    left: Option<&str>,
    right: Option<&str>,
    options: &TableOptions,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<TableComparison> {
    let delimiter = match options.delimiter.as_str() {
        "," => ',',
        "\t" => '\t',
        _ => return Err(Error::Invalid("Use comma or tab delimiter".into())),
    };
    let tolerance = options
        .numeric_tolerance
        .as_ref()
        .map(|v| {
            v.parse::<f64>()
                .map_err(|_| Error::Invalid("Invalid numeric tolerance".into()))
        })
        .transpose()?;
    if tolerance.is_some_and(|n| !n.is_finite() || n < 0.) {
        return Err(Error::Invalid(
            "Numeric tolerance must be finite and nonnegative".into(),
        ));
    }
    let (l, left_tables) = input(left, &options.left_source, delimiter, cancel)?;
    let (r, right_tables) = input(right, &options.right_source, delimiter, cancel)?;
    let warnings = if matches!(
        options.left_source,
        clear_contracts::TableSource::Html { .. }
    ) || matches!(
        options.right_source,
        clear_contracts::TableSource::Html { .. }
    ) {
        vec!["Static HTML text only. ASCII whitespace is collapsed, NBSP is retained; scripts/styles/templates and nested-table text are omitted. Spanned values repeat in covered grid slots. CSS, images and form values are not evaluated.".into()]
    } else {
        Vec::new()
    };
    compare_rows(
        l,
        r,
        (left_tables, right_tables, warnings),
        options,
        tolerance,
        cancel,
    )
}
fn named_columns(
    left: &[String],
    right: &[String],
    rows: [&[Vec<String>]; 2],
) -> Result<Vec<clear_contracts::TableColumn>> {
    let index = |headers: &[String], values: &[Vec<String>]| -> Result<BTreeMap<String, usize>> {
        let mut map = BTreeMap::new();
        for (i, name) in headers.iter().enumerate() {
            if name.trim().is_empty() || map.insert(name.clone(), i).is_some() {
                return Err(Error::Invalid(
                    "Name alignment requires unique, nonblank header names on each side".into(),
                ));
            }
        }
        if values.iter().any(|r| r.len() > headers.len()) {
            return Err(Error::Invalid(
                "A data row has more columns than its header; name alignment would omit data"
                    .into(),
            ));
        }
        Ok(map)
    };
    let a = index(left, rows[0])?;
    let b = index(right, rows[1])?;
    let mut columns = Vec::new();
    for name in left
        .iter()
        .chain(right.iter().filter(|n| !a.contains_key(*n)))
    {
        columns.push(clear_contracts::TableColumn {
            name: name.clone(),
            left: a.get(name).copied(),
            right: b.get(name).copied(),
        });
    }
    if columns.len() > MAX_COLUMNS {
        return Err(Error::Invalid(
            "Aligned table exceeds 1024 logical columns".into(),
        ));
    }
    Ok(columns)
}
fn mapped_key(
    key: Option<usize>,
    columns: &[clear_contracts::TableColumn],
    left: bool,
    has_rows: bool,
) -> Result<Option<usize>> {
    let Some(key) = key else {
        return Ok(None);
    };
    let column = columns
        .get(key)
        .ok_or_else(|| Error::Invalid("Logical key column is outside the aligned schema".into()))?;
    let index = if left { column.left } else { column.right };
    if index.is_none() && has_rows {
        return Err(Error::Invalid(
            "The selected logical key column is absent on a nonempty side".into(),
        ));
    }
    Ok(index)
}

fn compare_rows(
    mut l: Vec<Vec<String>>,
    mut r: Vec<Vec<String>>,
    metadata: (
        Vec<clear_contracts::HtmlTableInfo>,
        Vec<clear_contracts::HtmlTableInfo>,
        Vec<String>,
    ),
    options: &TableOptions,
    tolerance: Option<f64>,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<TableComparison> {
    let (left_tables, right_tables, warnings) = metadata;
    let lh = if options.headers && !l.is_empty() {
        l.remove(0)
    } else {
        Vec::new()
    };
    let rh = if options.headers && !r.is_empty() {
        r.remove(0)
    } else {
        Vec::new()
    };
    let column_map = if options.align_columns_by_name {
        if !options.headers {
            return Err(Error::Invalid(
                "Name alignment requires a header row".into(),
            ));
        }
        named_columns(&lh, &rh, [&l, &r])?
    } else {
        Vec::new()
    };
    let headers_changed = if options.align_columns_by_name {
        column_map
            .iter()
            .any(|c| c.left.is_none() || c.right.is_none())
    } else {
        lh != rh
    };
    let left_key = if options.align_columns_by_name {
        mapped_key(options.key_column, &column_map, true, !l.is_empty())?
    } else {
        options.key_column
    };
    let right_key = if options.align_columns_by_name {
        mapped_key(options.key_column, &column_map, false, !r.is_empty())?
    } else {
        options.key_column
    };
    let a = keyed(l, left_key)?;
    let mut b = keyed(r, right_key)?;
    let mut rows = Vec::new();
    let mut summary = Summary::default();
    for (key, left) in a {
        crate::compare::check_cancel(cancel)?;
        let right = b.remove(&key);
        let mut changed_columns = Vec::new();
        let status = match &right {
            None => {
                summary.left_only += 1;
                Status::LeftOnly
            }
            Some(right) => {
                if options.align_columns_by_name {
                    for (column, mapping) in column_map.iter().enumerate() {
                        let a = mapping.left.and_then(|i| left.get(i));
                        let b = mapping.right.and_then(|i| right.get(i));
                        if !equal(a, b, tolerance) {
                            changed_columns.push(column);
                        }
                    }
                } else {
                    for column in 0..left.len().max(right.len()) {
                        if !equal(left.get(column), right.get(column), tolerance) {
                            changed_columns.push(column);
                        }
                    }
                }
                if changed_columns.is_empty() {
                    summary.same += 1;
                    Status::Same
                } else {
                    summary.changed += 1;
                    Status::Changed
                }
            }
        };
        rows.push(TableRow {
            key,
            left: Some(left),
            right,
            changed_columns,
            status,
        });
    }
    for (key, right) in b {
        summary.right_only += 1;
        rows.push(TableRow {
            key,
            left: None,
            right: Some(right),
            changed_columns: Vec::new(),
            status: Status::RightOnly,
        });
    }
    rows.sort_by(|a, b| a.key.cmp(&b.key));
    let columns = if options.align_columns_by_name {
        column_map.len()
    } else {
        rows.iter()
            .map(|r| {
                r.left
                    .as_ref()
                    .map(Vec::len)
                    .unwrap_or_default()
                    .max(r.right.as_ref().map(Vec::len).unwrap_or_default())
            })
            .max()
            .unwrap_or_default()
            .max(lh.len())
            .max(rh.len())
    };
    Ok(TableComparison {
        column_map,
        headers_changed,
        columns,
        options: Some(options.clone()),
        left_tables,
        right_tables,
        warnings,
        left_headers: lh,
        right_headers: rh,
        rows,
        summary,
    })
}

type ParsedInput = (Vec<Vec<String>>, Vec<clear_contracts::HtmlTableInfo>);
fn input(
    text: Option<&str>,
    source: &clear_contracts::TableSource,
    delimiter: char,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<ParsedInput> {
    crate::compare::check_cancel(cancel)?;
    let Some(text) = text else {
        return Ok((Vec::new(), Vec::new()));
    };
    match source {
        clear_contracts::TableSource::Html { table } => {
            let doc = html::Document::parse(text, cancel)?;
            Ok((doc.selected(*table, cancel)?, doc.tables))
        }
        clear_contracts::TableSource::Csv => Ok((parse(text, ',')?, Vec::new())),
        clear_contracts::TableSource::Tsv => Ok((parse(text, '\t')?, Vec::new())),
        _ => Ok((parse(text, delimiter)?, Vec::new())),
    }
}
