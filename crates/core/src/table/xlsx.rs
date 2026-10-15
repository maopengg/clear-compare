//! Bounded OOXML cached-value extraction. Never allocate a library dense Range.
use crate::{Error, Result, compare::check_cancel};
use calamine::{DataRef, Reader, SheetType, SheetVisible, Xlsx};
use clear_contracts::WorksheetInfo;
use quick_xml::events::Event;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Cursor, Read},
    sync::atomic::AtomicBool,
};
const MAX_PART: u64 = 8 * 1024 * 1024;
const MAX_PACKAGE: u64 = 32 * 1024 * 1024;
const MAX_TEXT: usize = 8 * 1024 * 1024;
fn invalid(message: impl std::fmt::Display) -> Error {
    Error::Invalid(format!("Excel workbook: {message}"))
}
fn preflight(bytes: &[u8], cancel: &AtomicBool) -> Result<()> {
    check_cancel(cancel)?;
    if bytes.len() > crate::editing::MAX_DOCUMENT_BYTES {
        return Err(invalid("source exceeds 4 MiB"));
    }
    // Bound central-directory allocation hints before either ZIP implementation is constructed.
    crate::archive::preflight_limit(&mut Cursor::new(bytes), cancel, 512)?;
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(invalid)?;
    if zip.len() > 512 {
        return Err(invalid("package exceeds 512 parts"));
    }
    let mut names = BTreeSet::new();
    let mut total = 0;
    let mut elements = 0usize;
    for i in 0..zip.len() {
        check_cancel(cancel)?;
        let mut part = zip.by_index(i).map_err(invalid)?;
        let name = part.name().map_err(invalid)?.to_ascii_lowercase();
        if name.starts_with('/')
            || name.contains('\\')
            || name.split('/').any(|c| c == ".." || c == ".")
            || !names.insert(name.clone())
        {
            return Err(invalid("duplicate or unsafe package name"));
        }
        if part.is_dir() {
            continue;
        }
        total += part.size();
        if part.size() > MAX_PART || total > MAX_PACKAGE || part.encrypted() {
            return Err(invalid(
                "encrypted or oversized package part (8 MiB/part, 32 MiB total)",
            ));
        }
        let mut data = Vec::new();
        (&mut part).take(MAX_PART + 1).read_to_end(&mut data)?;
        if data.len() as u64 != part.size() || data.len() as u64 > MAX_PART {
            return Err(invalid("invalid expanded part size"));
        }
        let xml = name.ends_with(".xml")
            || name.ends_with(".rels")
            || data.iter().copied().find(|c| !c.is_ascii_whitespace()) == Some(b'<');
        if !xml {
            continue;
        }
        std::str::from_utf8(&data).map_err(|_| invalid("only UTF-8 XML parts are supported"))?;
        let mut reader = quick_xml::Reader::from_reader(data.as_slice());
        let mut depth = 0usize;
        loop {
            check_cancel(cancel)?;
            let event = reader.read_event().map_err(invalid)?;
            let empty = matches!(event, Event::Empty(_));
            match event {
                Event::DocType(_) => return Err(invalid("DTD declarations are unsupported")),
                Event::Start(ref e) | Event::Empty(ref e) => {
                    elements += 1;
                    if elements > 1_000_000 || depth >= 64 {
                        return Err(invalid("XML element/depth limit exceeded"));
                    }
                    if e.local_name().as_ref() == b"Relationship" {
                        let attribute = |key| -> Result<Option<String>> {
                            e.try_get_attribute(key)
                                .map_err(invalid)?
                                .map(|a| {
                                    a.decoded_and_normalized_value(
                                        quick_xml::XmlVersion::Implicit1_0,
                                        reader.decoder(),
                                    )
                                    .map(|v| v.into_owned())
                                    .map_err(invalid)
                                })
                                .transpose()
                        };
                        if attribute("Type")?.is_some_and(|t| t.ends_with("/worksheet"))
                            && (attribute("Target")?
                                .is_none_or(|p| !p.to_ascii_lowercase().ends_with(".xml"))
                                || attribute("TargetMode")?.is_some_and(|m| m == "External"))
                        {
                            return Err(invalid("worksheet targets must be internal XML parts"));
                        }
                    }
                    // Library shared-string reserve and shared-formula indices are untrusted hints.
                    for attribute in e.attributes() {
                        let attribute = attribute.map_err(invalid)?;
                        if (e.local_name().as_ref() == b"sst"
                            && attribute.key.local_name().as_ref() == b"uniqueCount")
                            || (e.local_name().as_ref() == b"f"
                                && attribute.key.local_name().as_ref() == b"si")
                        {
                            let value = attribute
                                .decoded_and_normalized_value(
                                    quick_xml::XmlVersion::Implicit1_0,
                                    reader.decoder(),
                                )
                                .map_err(invalid)?;
                            if value.parse::<usize>().map_err(invalid)? > super::MAX_CELLS {
                                return Err(invalid(
                                    "shared string/formula allocation hint exceeds limit",
                                ));
                            }
                        }
                    }
                    if !empty {
                        depth += 1;
                    }
                }
                Event::End(_) => {
                    depth = depth
                        .checked_sub(1)
                        .ok_or_else(|| invalid("invalid XML nesting"))?;
                }
                Event::Eof => {
                    if depth != 0 {
                        return Err(invalid("unclosed XML element"));
                    }
                    break;
                }
                _ => {}
            }
        }
    }
    Ok(())
}
pub struct Document<'a> {
    workbook: Xlsx<Cursor<&'a [u8]>>,
    pub sheets: Vec<WorksheetInfo>,
}
impl<'a> Document<'a> {
    pub fn parse(bytes: &'a [u8], cancel: &AtomicBool) -> Result<Self> {
        preflight(bytes, cancel)?;
        let workbook = Xlsx::new(Cursor::new(bytes)).map_err(invalid)?;
        check_cancel(cancel)?;
        if workbook.sheets_metadata().len() > 64 {
            return Err(invalid("more than 64 sheets"));
        }
        let mut names = BTreeSet::new();
        let mut sheets = Vec::new();
        for sheet in workbook.sheets_metadata() {
            if sheet.typ != SheetType::WorkSheet {
                continue;
            }
            if !names.insert(sheet.name.clone()) {
                return Err(invalid("duplicate worksheet name"));
            }
            sheets.push(WorksheetInfo {
                index: sheets.len(),
                name: sheet.name.clone(),
                hidden: sheet.visible != SheetVisible::Visible,
            });
        }
        Ok(Self { workbook, sheets })
    }
    pub fn selected(
        &mut self,
        index: usize,
        cancel: &AtomicBool,
    ) -> Result<(Vec<Vec<String>>, Vec<String>)> {
        let sheet = self
            .sheets
            .get(index)
            .ok_or_else(|| invalid("worksheet index is out of range"))?
            .name
            .clone();
        let mut reader = self
            .workbook
            .worksheet_cells_reader(&sheet)
            .map_err(invalid)?;
        let mut cells = BTreeMap::new();
        let mut positions = BTreeSet::new();
        let mut height = 0;
        let mut width = 0;
        let mut bytes = 0usize;
        let mut formulas = 0usize;
        while let Some(record) = reader.next_cell_with_formula_metadata().map_err(invalid)? {
            check_cancel(cancel)?;
            let (row, column) = (record.pos.0 as usize, record.pos.1 as usize);
            if row >= super::MAX_ROWS
                || column >= super::MAX_COLUMNS
                || positions.len() >= super::MAX_CELLS
                || !positions.insert((row, column))
            {
                return Err(invalid("duplicate cell or row/column/cell limit exceeded"));
            }
            if record.formula.is_some() {
                formulas += 1;
                if matches!(record.value, DataRef::Empty) {
                    return Err(invalid(format!(
                        "formula at row {}, column {} has no cached result; recalculate and save the workbook",
                        row + 1,
                        column + 1
                    )));
                }
            }
            let value = match record.value {
                DataRef::Empty => continue,
                DataRef::String(v) => v,
                DataRef::SharedString(v) => v.to_owned(),
                DataRef::Bool(v) => if v { "TRUE" } else { "FALSE" }.into(),
                DataRef::Int(v) => v.to_string(),
                DataRef::Float(v) if v.is_finite() => v.to_string(),
                DataRef::DateTime(v)
                    if v.as_f64().is_finite() && (0.0..=2_958_465.0).contains(&v.as_f64()) =>
                {
                    if v.is_datetime() {
                        let (y, m, d, h, n, s, ms) = v.to_ymd_hms_milli();
                        format!("{y:04}-{m:02}-{d:02}T{h:02}:{n:02}:{s:02}.{ms:03}")
                    } else {
                        format!("duration-days:{}", v.as_f64())
                    }
                }
                DataRef::DateTimeIso(v) | DataRef::DurationIso(v) => v,
                DataRef::Error(e) => {
                    return Err(invalid(format!(
                        "cell error at row {}, column {}: {e}",
                        row + 1,
                        column + 1
                    )));
                }
                _ => return Err(invalid("non-finite or out-of-range numeric/date value")),
            };
            bytes = bytes.saturating_add(value.len());
            height = height.max(row + 1);
            width = width.max(column + 1);
            if bytes > MAX_TEXT || height.saturating_mul(width) > super::MAX_CELLS {
                return Err(invalid(
                    "expanded worksheet exceeds 200000 cells or 8 MiB text",
                ));
            }
            cells.insert((row, column), value);
        }
        let mut rows = vec![vec![String::new(); width]; height];
        for ((row, column), value) in cells {
            rows[row][column] = value;
        }
        let warnings = vec![format!(
            "Excel worksheet {sheet}: cached values only ({formulas} formula cells); formulas are not recalculated. Number display formats, styles, merged-cell expansion, filters and hidden row/column state are ignored. Dates use ISO local date/time, durations use days; booleans use TRUE/FALSE. Cell types are compared as extracted strings."
        )];
        Ok((rows, warnings))
    }
}
