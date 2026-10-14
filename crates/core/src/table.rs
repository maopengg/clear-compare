//! Strict RFC-style delimited text parsing and explicit row/key comparison.
use crate::{Error, Result, editing::MAX_DOCUMENT_BYTES};
use clear_contracts::{Status, Summary, TableComparison, TableOptions, TableRow};
use std::collections::BTreeMap;
const MAX_CELLS: usize = 200_000;
const MAX_ROWS: usize = 30_000;
const MAX_COLUMNS: usize = 1024;

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
    let mut l = parse(left, delimiter)?;
    let mut r = parse(right, delimiter)?;
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
    let a = keyed(l, options.key_column)?;
    let mut b = keyed(r, options.key_column)?;
    let mut rows = Vec::new();
    let mut summary = Summary::default();
    for (key, left) in a {
        let right = b.remove(&key);
        let mut changed_columns = Vec::new();
        let status = match &right {
            None => {
                summary.left_only += 1;
                Status::LeftOnly
            }
            Some(right) => {
                for column in 0..left.len().max(right.len()) {
                    if !equal(left.get(column), right.get(column), tolerance) {
                        changed_columns.push(column);
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
    Ok(TableComparison {
        left_headers: lh,
        right_headers: rh,
        rows,
        summary,
    })
}
