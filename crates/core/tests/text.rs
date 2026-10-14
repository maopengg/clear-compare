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
