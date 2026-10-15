use clear_contracts::TableOptions;
use clear_core::table::{compare, parse};
#[test]
fn parser_retains_quoted_newlines_commas_escapes_and_empty_cells() {
    assert_eq!(
        parse("a,b\r\n\"x,y\",\"two\nlines\"\r\n\"a\"\"b\",\r\n", ',').unwrap(),
        vec![vec!["a", "b"], vec!["x,y", "two\nlines"], vec!["a\"b", ""]]
    );
    assert_eq!(parse("a,", ',').unwrap(), vec![vec!["a", ""]]);
    assert!(parse("\"open", ',').is_err());
    assert!(parse("a\"b", ',').is_err());
    assert!(parse("\"a\"junk", ',').is_err());
}
#[test]
fn keyed_numeric_comparison_detects_missing_rows_and_duplicate_keys() {
    let options = TableOptions {
        left_source: clear_contracts::TableSource::Delimited,
        right_source: clear_contracts::TableSource::Delimited,
        delimiter: ",".into(),
        headers: true,
        key_column: Some(0),
        numeric_tolerance: Some("0.01".into()),
    };
    let result = compare(
        "id,value\na,1.000\nb,2\n",
        "id,value\nb,2\na,1.005\nc,3\n",
        &options,
    )
    .unwrap();
    assert_eq!(result.summary.same, 2);
    assert_eq!(result.summary.right_only, 1);
    assert!(compare("id,value\na,1\na,2\n", "", &options).is_err());
    let strict = TableOptions {
        numeric_tolerance: None,
        ..options.clone()
    };
    assert_eq!(
        compare("id,value\na,1.000\n", "id,value\na,1.005\n", &strict)
            .unwrap()
            .summary
            .changed,
        1
    );
    assert!(
        compare(
            "",
            "",
            &TableOptions {
                numeric_tolerance: Some("NaN".into()),
                ..options
            }
        )
        .is_err()
    );
}
