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
        align_columns_by_name: false,
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

#[test]
fn name_alignment_remaps_values_and_keys_without_mutating_original_rows() {
    let options = TableOptions {
        align_columns_by_name: true,
        key_column: Some(0),
        numeric_tolerance: Some("0.01".into()),
        ..Default::default()
    };
    let left = "id,value\na,1.0\nb,2\n";
    let right = "value,id\n2,b\n1.005,a\n";
    let result = compare(left, right, &options).unwrap();
    assert_eq!(result.summary.same, 2);
    assert!(!result.headers_changed);
    assert_eq!(result.column_map[0].name, "id");
    assert_eq!(result.source_column(0, false), Some(1));
    assert_eq!(result.source_column(1, false), Some(0));
    assert_eq!(result.columns, 2);
    assert_eq!(result.rows[0].right.as_ref().unwrap(), &["1.005", "a"]);
    let positional = compare(left, right, &TableOptions::default()).unwrap();
    assert!(positional.headers_changed);
    assert_eq!(positional.summary.changed, 2);
}
#[test]
fn absent_columns_and_short_rows_remain_distinct_from_explicit_empty_cells() {
    let options = TableOptions {
        align_columns_by_name: true,
        ..Default::default()
    };
    let result = compare("id,v\na,\n", "v,id,extra\n,a,\n", &options).unwrap();
    assert!(result.headers_changed);
    assert_eq!(result.columns, 3);
    assert_eq!(result.source_column(2, true), None);
    assert_eq!(result.rows[0].changed_columns, [2]);
    let short = compare("id,v\na\n", "v,id\n,a\n", &options).unwrap();
    assert_eq!(short.rows[0].changed_columns, [1]);
    let headers_only = compare("id,v\n", "v,id\n", &options).unwrap();
    assert!(!headers_only.headers_changed);
    assert!(headers_only.rows.is_empty());
    let extra = compare("id,v\n", "v,id,extra\n", &options).unwrap();
    assert!(extra.headers_changed);
    let exact = compare("id,Value\na,1\n", "id,value\na,1\n", &options).unwrap();
    assert_eq!(exact.columns, 3);
    assert_eq!(exact.rows[0].changed_columns, [1, 2]);
}
#[test]
fn ambiguous_or_unmapped_data_and_invalid_logical_keys_are_rejected() {
    let options = TableOptions {
        align_columns_by_name: true,
        ..Default::default()
    };
    for left in ["id,id\na,b\n", "id, \na,b\n", "id\na,b\n"] {
        assert!(compare(left, "id\na\n", &options).is_err());
    }
    assert!(
        compare(
            "a\nx\n",
            "a\nx\n",
            &TableOptions {
                headers: false,
                ..options.clone()
            }
        )
        .is_err()
    );
    let key = TableOptions {
        key_column: Some(1),
        ..options.clone()
    };
    assert!(compare("id,v\na,x\n", "id\na\n", &key).is_err());
    assert!(
        compare(
            "id\na\n",
            "id\na\n",
            &TableOptions {
                key_column: Some(9),
                ..options.clone()
            }
        )
        .is_err()
    );
    assert!(
        compare(
            "id,v\na,x\na,y\n",
            "v,id\nx,a\n",
            &TableOptions {
                key_column: Some(0),
                ..options.clone()
            }
        )
        .is_err()
    );
    let right_only = compare(
        "",
        "id,v\na,1\n",
        &TableOptions {
            key_column: Some(0),
            ..options
        },
    )
    .unwrap();
    assert_eq!(right_only.summary.right_only, 1);
    assert!(right_only.headers_changed);
}
#[test]
fn aligned_schema_has_a_global_column_limit() {
    let options = TableOptions {
        align_columns_by_name: true,
        ..Default::default()
    };
    let left = (0..1024)
        .map(|i| format!("col{i}"))
        .collect::<Vec<_>>()
        .join(",");
    assert_eq!(compare(&left, &left, &options).unwrap().columns, 1024);
    assert!(compare(&left, "extra", &options).is_err());
}
