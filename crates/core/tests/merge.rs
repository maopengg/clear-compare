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
    let token = result.conflict_token.as_ref().unwrap();
    assert!(result.text.contains(&format!("<<<<<<< LEFT [cc:{token}] [block:0]\nleft\n||||||| BASE [cc:{token}] [block:0]\nb\n======= [cc:{token}] [block:0]\nright\n>>>>>>> RIGHT [cc:{token}] [block:0]\n")));
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

#[test]
fn generated_conflicts_resolve_with_original_final_newline_and_multi_group_identity() {
    use clear_contracts::merge_edit::{Choice, blocks, resolve};
    let r = three_way("base", "left", "right").unwrap();
    assert_eq!(r.conflict_versions.len(), 1);
    let token = r.conflict_token.as_ref().unwrap();
    assert_eq!(
        resolve(&r.text, token, &r.conflict_versions, 0, Choice::Right).unwrap(),
        "right"
    );
    let r = three_way("a\nkeep\nb\n", "A\nkeep\nB\n", "X\nkeep\nY\n").unwrap();
    assert_eq!(r.conflicts, 2);
    let token = r.conflict_token.as_ref().unwrap();
    let text = resolve(&r.text, token, &r.conflict_versions, 1, Choice::Left).unwrap();
    assert_eq!(blocks(&text, token).unwrap()[0].id, 0);
    let text = resolve(&text, token, &r.conflict_versions, 0, Choice::Right).unwrap();
    assert_eq!(text, "X\nkeep\nB\n");
}
#[test]
fn oversized_generated_merge_results_are_rejected_before_editor_staging() {
    let size = 3 * 1024 * 1024;
    assert!(three_way(&"a".repeat(size), &"b".repeat(size), &"c".repeat(size)).is_err());
}
