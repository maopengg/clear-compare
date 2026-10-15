use clear_contracts::{CompareRequest, TableOptions, TableSource};
use clear_core::table::{document, xlsx};
use std::{fs, sync::atomic::AtomicBool};
fn cancel() -> AtomicBool {
    AtomicBool::new(false)
}
#[path = "support/xlsx_fixture.rs"]
mod fixture;
use fixture::{DATA, STRINGS, package, workbook};
#[test]
fn worksheets_cached_formulas_key_and_header_alignment_work_across_formats() {
    let t = tempfile::tempdir().unwrap();
    let left = t.path().join("a.xlsx");
    let right = t.path().join("b.csv");
    fs::write(&left, workbook(DATA, STRINGS)).unwrap();
    fs::write(&right, "value,id\n1.000,a\n2,b\n").unwrap();
    let doc = document::Document::from_paths(&left, &right, &cancel()).unwrap();
    assert!(doc.info.left_xlsx);
    assert_eq!(doc.info.left_sheets.len(), 2);
    assert!(doc.info.left_sheets[1].hidden);
    let report = doc
        .compare(
            &TableOptions {
                align_columns_by_name: true,
                key_column: Some(0),
                numeric_tolerance: Some("0.01".into()),
                ..Default::default()
            },
            &cancel(),
        )
        .unwrap();
    assert_eq!(report.summary.same, 2);
    assert!(
        report
            .warnings
            .iter()
            .any(|s| s.contains("1 formula cells"))
    );
    fs::write(&left, b"external replacement").unwrap();
    assert_eq!(
        doc.compare(
            &TableOptions {
                left_source: TableSource::Xlsx { sheet: 1 },
                headers: false,
                ..Default::default()
            },
            &cancel()
        )
        .unwrap()
        .rows[0]
            .left
            .as_ref()
            .unwrap()[0],
        "hidden value"
    );
    assert!(
        doc.compare(
            &TableOptions {
                left_source: TableSource::Xlsx { sheet: 2 },
                ..Default::default()
            },
            &cancel()
        )
        .is_err()
    );
}
#[test]
fn sparse_leading_cells_shared_strings_booleans_and_blank_cells_have_explicit_grid_semantics() {
    let input = workbook(
        r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="2"><c r="B2" t="s"><v>2</v></c><c r="C2" t="b"><v>1</v></c><c r="D2" t="inlineStr"><is><t></t></is></c></row><row r="3"><c r="A3" t="s"><v>3</v></c></row></sheetData></worksheet>"#,
        STRINGS,
    );
    let mut doc = xlsx::Document::parse(&input, &cancel()).unwrap();
    let (rows, _) = doc.selected(0, &cancel()).unwrap();
    assert_eq!(
        rows,
        vec![
            vec!["", "", "", ""],
            vec!["", "rich & text", "TRUE", ""],
            vec!["雪", "", "", ""]
        ]
    );
}
#[test]
fn uncached_formulas_cell_errors_duplicates_and_sparse_allocation_attacks_are_errors() {
    for cell in [
        r#"<c r="A1"><f>1+1</f></c>"#,
        r#"<c r="A1" t="e"><v>#DIV/0!</v></c>"#,
        r#"<c r="A1"><v>1</v></c><c r="A1"><v>2</v></c>"#,
        r#"<c r="XFD1048576"><v>1</v></c>"#,
        r#"<c r="A1"><v>NaN</v></c>"#,
    ] {
        let sheet = format!(
            r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1">{cell}</row></sheetData></worksheet>"#
        );
        let input = workbook(&sheet, STRINGS);
        let mut doc = xlsx::Document::parse(&input, &cancel()).unwrap();
        assert!(doc.selected(0, &cancel()).is_err(), "accepted {cell}");
    }
}
#[test]
fn package_preflight_bounds_library_allocation_hints_dtd_and_expansion() {
    for shared in [
        r#"<sst uniqueCount="18446744073709551615"></sst>"#,
        r#"<!DOCTYPE sst [<!ENTITY x "expanded">]><sst></sst>"#,
    ] {
        assert!(xlsx::Document::parse(&workbook(DATA, shared), &cancel()).is_err());
    }
    let shared_formula = DATA.replace(
        "<f>1+1</f>",
        r#"<f t="shared" si="18446744073709551615" ref="B2:B3">1+1</f>"#,
    );
    assert!(xlsx::Document::parse(&workbook(&shared_formula, STRINGS), &cancel()).is_err());
    let huge = format!("<unused>{}</unused>", "x".repeat(8 * 1024 * 1024));
    assert!(
        xlsx::Document::parse(&package(DATA, STRINGS, &[("unused.xml", &huge)]), &cancel())
            .is_err()
    );
    assert!(xlsx::Document::parse(&workbook(DATA, STRINGS), &AtomicBool::new(true)).is_err());
}
#[test]
fn content_verified_folder_open_pins_workbook_bytes_and_missing_side_stays_missing() {
    let t = tempfile::tempdir().unwrap();
    for d in ["l", "r"] {
        fs::create_dir(t.path().join(d)).unwrap();
    }
    let left = t.path().join("l/a.xlsx");
    fs::write(&left, workbook(DATA, STRINGS)).unwrap();
    let s = clear_core::compare::compare(
        &CompareRequest {
            left: t.path().join("l").display().to_string(),
            right: t.path().join("r").display().to_string(),
            exclusions: String::new(),
            content: true,
        },
        &cancel(),
    )
    .unwrap();
    let row = &s.report.rows[0];
    let doc = document::Document::open(&s, &row.id, &cancel()).unwrap();
    assert!(!doc.info.right_available);
    assert_eq!(
        doc.compare(&TableOptions::default(), &cancel())
            .unwrap()
            .summary
            .left_only,
        2
    );
    fs::write(&left, workbook(&DATA.replace("1.005", "9"), STRINGS)).unwrap();
    assert!(document::Document::open(&s, &row.id, &cancel()).is_err());
}

#[test]
fn shared_formulas_use_cached_values_without_expanding_their_range() {
    let data = workbook(
        r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1"><f t="shared" si="0" ref="A1:XFD1048576">1+1</f><v>2</v></c></row><row r="2"><c r="A2"><f t="shared" si="0"/><v>3</v></c></row></sheetData></worksheet>"#,
        STRINGS,
    );
    let (rows, warnings) = xlsx::Document::parse(&data, &cancel())
        .unwrap()
        .selected(0, &cancel())
        .unwrap();
    assert_eq!(rows, vec![vec!["2"], vec!["3"]]);
    assert!(warnings[0].contains("2 formula cells"));
}
#[test]
fn workbook_date_systems_normalize_to_the_same_local_calendar_value() {
    let styles = r#"<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><cellXfs count="2"><xf numFmtId="0"/><xf numFmtId="14"/></cellXfs></styleSheet>"#;
    let workbook1904 = r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><workbookPr date1904="1"/><sheets><sheet name="Date" sheetId="1" r:id="rId1"/></sheets></workbook>"#;
    let mut values = Vec::new();
    for serial in [1462, 0] {
        let sheet = format!(
            r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="1"><c r="A1" s="1"><v>{serial}</v></c></row></sheetData></worksheet>"#
        );
        let mut extra = vec![("xl/styles.xml", styles)];
        if serial == 0 {
            extra.push(("xl/workbook.xml", workbook1904));
        }
        let data = package(&sheet, STRINGS, &extra);
        values.push(
            xlsx::Document::parse(&data, &cancel())
                .unwrap()
                .selected(0, &cancel())
                .unwrap()
                .0[0][0]
                .clone(),
        );
    }
    assert_eq!(
        values,
        ["1904-01-01T00:00:00.000", "1904-01-01T00:00:00.000"]
    );
}

#[test]
fn worksheet_relationships_cannot_bypass_xml_preflight_through_binary_part_names() {
    let relationships = r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/payload.bin"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/></Relationships>"#;
    let payload = DATA.replace(
        "<f>1+1</f>",
        r#"<f t="shared" si="18446744073709551615">1+1</f>"#,
    );
    let bytes = package(
        DATA,
        STRINGS,
        &[
            ("xl/_rels/workbook.xml.rels", relationships),
            ("xl/worksheets/payload.bin", &payload),
        ],
    );
    let error = xlsx::Document::parse(&bytes, &cancel())
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("internal XML parts"));
}
