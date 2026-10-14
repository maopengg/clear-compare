use clear_core::merge::three_way;
#[test]
fn independent_adjacent_edits_and_insertions_merge_without_loss() {
    let result = three_way("a\nb\nc\n", "A\nb\nc\n", "a\nB\nc\n").unwrap();
    assert_eq!(result.conflicts, 0);
    assert_eq!(result.text, "A\nB\nc\n");
    let result = three_way("a\nb\nc\n", "intro\na\nb\nc\n", "a\nb\nc\noutro\n").unwrap();
    assert_eq!(result.conflicts, 0);
    assert_eq!(result.text, "intro\na\nb\nc\noutro\n");
}
#[test]
fn competing_replacements_and_same_position_insertions_conflict() {
    let result = three_way("a\nb\n", "a\nleft\n", "a\nright\n").unwrap();
    assert_eq!(result.conflicts, 1);
    assert!(
        result
            .text
            .contains("<<<<<<< LEFT\nleft\n||||||| BASE\nb\n=======\nright\n>>>>>>> RIGHT\n")
    );
    let result = three_way("a\n", "one\na\n", "two\na\n").unwrap();
    assert_eq!(result.conflicts, 1);
    assert!(result.text.ends_with("a\n"));
}
#[test]
fn deletion_and_identical_edits_are_resolved() {
    assert_eq!(
        three_way("a\nb\nc\n", "a\nc\n", "a\nb\nC\n").unwrap().text,
        "a\nC\n"
    );
    assert_eq!(three_way("a", "same", "same").unwrap().text, "same");
    assert_eq!(three_way("a", "a", "right").unwrap().conflicts, 0);
}
