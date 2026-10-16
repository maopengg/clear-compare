use clear_contracts::TextOptions;
use clear_core::text::{compare_strings, split_lines};

#[test]
fn normalization_retains_original_text_and_line_numbers() {
    let options = TextOptions {
        ignore_case: true,
        ignore_whitespace: true,
        ignore_line_endings: true,
    };
    let d = compare_strings(
        "sample",
        "Hello world\r\n中文\r\n",
        "HELLO  WORLD\n中文\n",
        &options,
    )
    .unwrap();
    assert_eq!(d.lines.len(), 2);
    assert!(d.lines.iter().all(|l| l.tag == "equal"));
    assert_eq!(d.lines[0].text, "Hello world\r\n");
    assert_eq!(d.lines[0].right_text.as_deref(), Some("HELLO  WORLD\n"));
    assert_eq!(d.lines[1].old_line, Some(2));
    let strict = compare_strings(
        "sample",
        "Hello world\r\n",
        "HELLO  WORLD\n",
        &TextOptions::default(),
    )
    .unwrap();
    assert_eq!(strict.lines.iter().filter(|l| l.tag != "equal").count(), 2);
}

#[test]
fn final_newline_and_cr_only_lines_remain_significant() {
    assert_eq!(
        split_lines("a\rb\r\nc\nd"),
        vec!["a\r", "b\r\n", "c\n", "d"]
    );
    assert!(
        compare_strings(
            "sample",
            "a\n",
            "a",
            &TextOptions {
                ignore_line_endings: true,
                ..Default::default()
            }
        )
        .unwrap()
        .lines
        .iter()
        .any(|l| l.tag != "equal")
    );
    assert!(
        compare_strings("sample", "", "", &TextOptions::default())
            .unwrap()
            .lines
            .is_empty()
    );
}

fn marked(line: &clear_contracts::DiffLine) -> String {
    line.changes
        .iter()
        .map(|r| {
            assert!(r.start <= r.end && r.end <= line.text.len());
            assert!(line.text.is_char_boundary(r.start) && line.text.is_char_boundary(r.end));
            &line.text[r.start..r.end]
        })
        .collect()
}
#[test]
fn intraline_unicode_and_rules_preserve_original_byte_ranges() {
    let r = compare_strings(
        "x",
        "\u{4e2d}\u{1f642}value\n",
        "\u{4e2d}\u{1f643}value\n",
        &TextOptions::default(),
    )
    .unwrap();
    assert_eq!(marked(&r.lines[0]), "\u{1f642}");
    assert_eq!(marked(&r.lines[1]), "\u{1f643}");
    let options = TextOptions {
        ignore_case: true,
        ignore_whitespace: true,
        ignore_line_endings: true,
    };
    let r = compare_strings("x", "Key = value_old\r\n", "KEY= value_new\n", &options).unwrap();
    assert_eq!(marked(&r.lines[0]), "old");
    assert_eq!(marked(&r.lines[1]), "new");
    assert!(!clear_contracts::text_view::significant_ending(
        Some(&r.lines[0]),
        Some(&r.lines[1])
    ));
    assert_eq!(r.lines[0].text, "Key = value_old\r\n");
    let r = compare_strings(
        "x",
        "\u{039f}\u{03a3} x\n",
        "\u{03bf}\u{03c2} y\n",
        &TextOptions {
            ignore_whitespace: false,
            ..options.clone()
        },
    )
    .unwrap();
    assert_eq!(marked(&r.lines[0]), "x");
    assert_eq!(marked(&r.lines[1]), "y");
    let r = compare_strings("x", "\u{0130} x\n", "i\u{0307} y\n", &options).unwrap();
    assert_eq!(marked(&r.lines[0]), "x");
    assert_eq!(marked(&r.lines[1]), "y");
}
#[test]
fn eol_only_changes_and_budget_fallbacks_remain_explicit() {
    let r = compare_strings("x", "a\r\n", "a\n", &TextOptions::default()).unwrap();
    assert_eq!(marked(&r.lines[0]), "\r");
    assert!(clear_contracts::text_view::significant_ending(
        Some(&r.lines[0]),
        Some(&r.lines[1])
    ));
    assert!(marked(&r.lines[1]).is_empty());
    let a = format!("{}a\n", "x".repeat(20000));
    let b = format!("{}b\n", "x".repeat(20000));
    let r = compare_strings("x", &a, &b, &TextOptions::default()).unwrap();
    assert!(r.intraline_limited);
    assert!(r.lines.iter().all(|l| l.intraline_limited));
    assert_eq!(marked(&r.lines[0]), a);
    assert_eq!(marked(&r.lines[1]), b);
}

#[test]
fn annotations_and_alignment_never_alter_source_reconstruction() {
    let samples = [
        "",
        "a",
        "a\n",
        "A \r\n\u{4e2d}\r",
        "\u{1f642}\nnext\n",
        "line\nline\nend",
    ];
    for left in samples {
        for right in samples {
            for ignore in [false, true] {
                let options = TextOptions {
                    ignore_case: ignore,
                    ignore_whitespace: ignore,
                    ignore_line_endings: ignore,
                };
                let r = compare_strings("x", left, right, &options).unwrap();
                let restored_left = r
                    .lines
                    .iter()
                    .filter(|l| l.old_line.is_some())
                    .map(|l| l.text.as_str())
                    .collect::<String>();
                let restored_right = r
                    .lines
                    .iter()
                    .filter(|l| l.new_line.is_some())
                    .map(|l| l.right_text.as_deref().unwrap_or(&l.text))
                    .collect::<String>();
                assert_eq!(restored_left, left);
                assert_eq!(restored_right, right);
                for l in &r.lines {
                    let _ = marked(l);
                }
                let rows = clear_contracts::text_view::align(&r.lines);
                let aligned_left = rows
                    .iter()
                    .filter_map(|row| row.left)
                    .map(|n| r.lines[n].text.as_str())
                    .collect::<String>();
                let aligned_right = rows
                    .iter()
                    .filter_map(|row| row.right)
                    .map(|n| r.lines[n].right_text.as_deref().unwrap_or(&r.lines[n].text))
                    .collect::<String>();
                assert_eq!(aligned_left, left);
                assert_eq!(aligned_right, right);
            }
        }
    }
}

#[test]
fn missing_side_created_after_comparison_is_not_treated_as_empty() {
    let left = tempfile::tempdir().unwrap();
    let right = tempfile::tempdir().unwrap();
    std::fs::write(left.path().join("sample.txt"), "source\n").unwrap();
    let snapshot = clear_core::compare::compare(
        &clear_contracts::CompareRequest {
            left: left.path().to_string_lossy().into_owned(),
            right: right.path().to_string_lossy().into_owned(),
            exclusions: String::new(),
            content: false,
        },
        &std::sync::atomic::AtomicBool::new(false),
    )
    .unwrap();
    let id = &snapshot.report.rows[0].id;
    assert_eq!(clear_core::text::load_pair(&snapshot, id).unwrap().2, "");
    std::fs::write(right.path().join("sample.txt"), "appeared\n").unwrap();
    assert!(clear_core::text::load_pair(&snapshot, id).is_err());
    assert_eq!(
        std::fs::read_to_string(right.path().join("sample.txt")).unwrap(),
        "appeared\n"
    );
}
