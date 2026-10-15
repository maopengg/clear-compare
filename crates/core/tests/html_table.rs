use clear_contracts::{CompareRequest, TableOptions, TableSource};
use clear_core::table::{self, html::Document};
use std::{fs, sync::atomic::AtomicBool};
fn cancel() -> AtomicBool {
    AtomicBool::new(false)
}
fn selected(text: &str, index: usize) -> Vec<Vec<String>> {
    Document::parse(text, &cancel())
        .unwrap()
        .selected(index, &cancel())
        .unwrap()
}
#[test]
fn html_entities_optional_tags_and_static_text_have_defined_semantics() {
    let html = "<TABLE><TR><TH>id<TH>value<TR><TD>a<TD><div>A</div>B<br>C &amp; D&nbsp;E<script>fetch('never')</script><style>evil</style><span hidden>hidden</span></TABLE>";
    assert_eq!(
        selected(html, 0),
        vec![vec!["id", "value"], vec!["a", "A B C & D\u{a0}E"]]
    );
    let d=Document::parse("<template><table><tr><td>inert</table></template><table hidden><tr><td>hidden</table><table id='outer'><caption>Outer</caption><tr><td>before<table id='inner'><tr><td>INNER</table>after</table>",&cancel()).unwrap();
    assert_eq!(d.tables.len(), 2);
    assert_eq!(d.tables[0].element_id, "outer");
    assert_eq!(d.tables[0].caption, "Outer");
    assert_eq!(
        d.selected(0, &cancel()).unwrap(),
        vec![vec!["before after"]]
    );
    assert_eq!(d.selected(1, &cancel()).unwrap(), vec![vec!["INNER"]]);
}
#[test]
fn row_groups_and_merged_cells_expand_without_crossing_groups() {
    let html = "<table><tfoot><tr><td>F<td>x<td>y</tfoot><tbody><tr><td rowspan=0>A<td>1<td>2<tr><td colspan=2>B</tbody><thead><tr><th colspan=2>K<th>V</thead></table>";
    assert_eq!(
        selected(html, 0),
        vec![
            vec!["K", "K", "V"],
            vec!["A", "1", "2"],
            vec!["A", "B", "B"],
            vec!["F", "x", "y"]
        ]
    );
    assert_eq!(
        selected(
            "<table><tbody><tr><td rowspan=3>A<td>1</tbody><tbody><tr><td>Z</tbody></table>",
            0
        ),
        vec![vec!["A", "1"], vec!["A", ""], vec!["A", ""], vec!["Z", ""]]
    );
    assert_eq!(
        selected("<table><tr><td colspan=0>one<td>two</table>", 0),
        vec![vec!["one", "two"]]
    );
}
#[test]
fn malformed_overlapping_or_oversized_models_are_not_compared_as_equal() {
    for html in [
        "<table><tr><td>x<td rowspan=2>y<tr><td colspan=2>overlap</table>",
        "<table><tr><td colspan=bad>x</table>",
        "<table><tr><td rowspan=65534>x</table>",
        "<table><tr><td colspan=1000 rowspan=1000>x</table>",
    ] {
        let d = Document::parse(html, &cancel()).unwrap();
        assert!(d.selected(0, &cancel()).is_err(), "accepted {html}");
    }
    let html = format!("<table><tr><td colspan=1000>{}</table>", "x".repeat(10000));
    assert!(
        Document::parse(&html, &cancel())
            .unwrap()
            .selected(0, &cancel())
            .is_err()
    );
    assert!(Document::parse(&"x".repeat(1024 * 1024 + 1), &cancel()).is_err());
    assert!(Document::parse("<table></table>", &AtomicBool::new(true)).is_err());
    assert!(
        Document::parse(
            &format!(
                "{}<table></table>{}",
                "<div>".repeat(300),
                "</div>".repeat(300)
            ),
            &cancel()
        )
        .is_err()
    );
    assert!(
        Document::parse("plain text", &cancel())
            .unwrap()
            .selected(0, &cancel())
            .is_err()
    );
    assert!(Document::parse(&"<table></table>".repeat(65), &cancel()).is_err());
}
#[test]
fn html_to_csv_reuses_key_alignment_and_numeric_tolerance() {
    let html = "<table><tr><th>id<th>value<tr><td>b<td>2<tr><td>a<td>1.005</table>";
    let csv = "id,value\na,1.000\nb,2\n";
    let options = TableOptions {
        left_source: TableSource::Html { table: 0 },
        right_source: TableSource::Delimited,
        key_column: Some(0),
        numeric_tolerance: Some("0.01".into()),
        ..Default::default()
    };
    let r = table::compare(html, csv, &options).unwrap();
    assert_eq!(r.summary.same, 2);
    assert_eq!(r.columns, 2);
    assert_eq!(r.left_tables.len(), 1);
    assert!(!r.warnings.is_empty());
    let second = "<table><tr><td>wrong</table><table><tr><th>id<th>value<tr><td>a<td>1<tr><td>b<td>2</table>";
    let options = TableOptions {
        left_source: TableSource::Html { table: 1 },
        ..options
    };
    assert_eq!(
        table::compare(second, csv, &options).unwrap().summary.same,
        2
    );
}
#[test]
fn frozen_documents_reuse_original_text_and_detect_changed_open_sources() {
    let t = tempfile::tempdir().unwrap();
    let l = t.path().join("left");
    let r = t.path().join("right");
    fs::create_dir(&l).unwrap();
    fs::create_dir(&r).unwrap();
    let html = "<table><tr><th>id<th>value<tr><td>a<td>1</table>";
    fs::write(l.join("data.html"), html).unwrap();
    fs::write(r.join("data.html"), "id,value\na,1\n").unwrap();
    let request = CompareRequest {
        left: l.display().to_string(),
        right: r.display().to_string(),
        content: true,
        exclusions: String::new(),
    };
    let snapshot = clear_core::compare::compare(&request, &cancel()).unwrap();
    let id = &snapshot.report.rows[0].id;
    let doc = table::document::Document::open(&snapshot, id, &cancel()).unwrap();
    assert_eq!(doc.info.left_tables.len(), 1);
    let options = TableOptions {
        right_source: TableSource::Delimited,
        key_column: Some(0),
        ..Default::default()
    };
    assert_eq!(doc.compare(&options, &cancel()).unwrap().summary.same, 1);
    fs::write(l.join("data.html"), html.replace(">1<", ">9<")).unwrap();
    assert_eq!(doc.compare(&options, &cancel()).unwrap().summary.same, 1);
    assert!(table::document::Document::open(&snapshot, id, &cancel()).is_err());
    let fresh = clear_core::compare::compare(&request, &cancel()).unwrap();
    let doc2 =
        table::document::Document::open(&fresh, &fresh.report.rows[0].id, &cancel()).unwrap();
    assert_eq!(
        doc2.compare(&options, &cancel()).unwrap().summary.changed,
        1
    );
}
#[test]
fn missing_side_is_empty_without_misreading_an_empty_html_file() {
    let t = tempfile::tempdir().unwrap();
    let l = t.path().join("left");
    let r = t.path().join("right");
    fs::create_dir(&l).unwrap();
    fs::create_dir(&r).unwrap();
    fs::write(l.join("a.html"), "<table><tr><th>x<tr><td>data</table>").unwrap();
    let s = clear_core::compare::compare(
        &CompareRequest {
            left: l.display().to_string(),
            right: r.display().to_string(),
            exclusions: String::new(),
            content: true,
        },
        &cancel(),
    )
    .unwrap();
    let doc = table::document::Document::open(&s, &s.report.rows[0].id, &cancel()).unwrap();
    assert!(!doc.info.right_available);
    assert_eq!(
        doc.compare(&Default::default(), &cancel())
            .unwrap()
            .summary
            .left_only,
        1
    );
    let html = TableOptions {
        left_source: TableSource::Html { table: 0 },
        right_source: TableSource::Html { table: 0 },
        ..Default::default()
    };
    assert!(table::compare("", "", &html).is_err());
}

#[test]
fn catalog_survives_a_bad_first_grid_and_foreign_scripts_stay_out_of_cell_text() {
    let t = tempfile::tempdir().unwrap();
    let file = t.path().join("a.html");
    fs::write(&file,"<table><tr><td colspan=bad>invalid</table><table><tr><th>x<tr><td><svg><script>evil()</script></svg>safe</table>").unwrap();
    let doc = table::document::Document::from_paths(&file, &file, &cancel()).unwrap();
    assert_eq!(doc.info.left_tables.len(), 2);
    assert!(doc.compare(&Default::default(), &cancel()).is_err());
    let options = TableOptions {
        left_source: TableSource::Html { table: 1 },
        right_source: TableSource::Html { table: 1 },
        ..Default::default()
    };
    let result = doc.compare(&options, &cancel()).unwrap();
    assert_eq!(
        result.rows[0].left.as_ref().unwrap(),
        &vec!["safe".to_string()]
    );
}

#[test]
fn csv_tsv_extension_detection_and_explicit_formats_work_independently() {
    let t = tempfile::tempdir().unwrap();
    let l = t.path().join("a.csv");
    let r = t.path().join("b.tsv");
    fs::write(&l, "id,value\na,1\n").unwrap();
    fs::write(&r, "id\tvalue\na\t1\n").unwrap();
    let doc = table::document::Document::from_paths(&l, &r, &cancel()).unwrap();
    assert!(doc.info.right_tsv);
    assert_eq!(
        doc.compare(&Default::default(), &cancel())
            .unwrap()
            .summary
            .same,
        1
    );
    let options = TableOptions {
        left_source: TableSource::Csv,
        right_source: TableSource::Tsv,
        ..Default::default()
    };
    assert_eq!(
        table::compare("id,value\na,1\n", "id\tvalue\na\t1\n", &options)
            .unwrap()
            .summary
            .same,
        1
    );
}

#[test]
fn node_allocation_budget_rejects_dense_markup_before_table_comparison() {
    assert!(Document::parse(&"<br>".repeat(100001), &cancel()).is_err());
}
