use clear_contracts::workbench::*;
use clear_core::workbench::Workspace;
use std::{fs, path::Path, process::Command, sync::atomic::AtomicBool};
const CANCEL: AtomicBool = AtomicBool::new(false);
fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim_end().into()
}
fn user(root: &Path) {
    git(root, &["config", "user.name", "Branch Test"]);
    git(root, &["config", "user.email", "branches@example.com"]);
    git(root, &["config", "commit.gpgsign", "false"]);
    git(root, &["config", "core.hooksPath", ""]);
    git(root, &["config", "core.autocrlf", "false"]);
}

fn fixture() -> (tempfile::TempDir, Workspace, String) {
    let d = tempfile::tempdir().unwrap();
    git(d.path(), &["init", "-b", "main"]);
    user(d.path());
    fs::write(d.path().join("file.txt"), "base\n").unwrap();
    git(d.path(), &["add", "."]);
    git(d.path(), &["commit", "-m", "base"]);
    let mut w = Workspace::default();
    let c = w
        .scan(
            ScanSettings {
                roots: vec![d.path().to_str().unwrap().into()],
                exclusions: vec![],
            },
            &CANCEL,
        )
        .unwrap();
    let id = c.repositories[0].id.clone();
    (d, w, id)
}
fn run(w: &mut Workspace, id: &str, action: Action) -> ActionResult {
    let s = w.open(id, &CANCEL).unwrap();
    let p = w.prepare(id, &s.revision, action, &CANCEL).unwrap();
    w.apply(&p.id, &CANCEL).unwrap()
}
fn save(w: &mut Workspace, id: &str, include_untracked: bool) {
    run(
        w,
        id,
        Action::StashSave {
            message: "saved fixture".into(),
            include_untracked,
        },
    );
}
fn use_stash(id: String, operation: StashOperation, restore_index: bool) -> Action {
    Action::StashUse {
        id,
        operation,
        name: None,
        restore_index,
    }
}
#[test]
fn stash_trees_show_staged_unstaged_and_untracked_content_without_checkout() {
    let (d, mut w, id) = fixture();
    assert!(w.stashes(&id, 0, &CANCEL).unwrap().entries.is_empty());
    fs::write(d.path().join("file.txt"), "staged\n").unwrap();
    git(d.path(), &["add", "file.txt"]);
    fs::write(d.path().join("file.txt"), "working\n").unwrap();
    fs::write(d.path().join("new.txt"), "new content\n").unwrap();
    save(&mut w, &id, true);
    let page = w.stashes(&id, 0, &CANCEL).unwrap();
    let e = &page.entries[0];
    assert_eq!(e.position, 0);
    assert_eq!(e.parents.len(), 3);
    assert!(e.message.contains("saved fixture"));
    for (area, expected) in [
        ("tracked", "+working"),
        ("index", "+staged"),
        ("unstaged", "-staged"),
        ("untracked", "+new content"),
    ] {
        let detail = w.stash_detail(&id, &e.id, area, &CANCEL).unwrap();
        assert_eq!(detail.area, area);
        let patch = w
            .commit_patch_page(
                &id,
                &detail.detail.id,
                &detail.detail.files[0].id,
                0,
                &CANCEL,
            )
            .unwrap();
        assert!(patch.text.contains(expected), "{area}: {}", patch.text);
    }
    assert_eq!(
        fs::read_to_string(d.path().join("file.txt")).unwrap(),
        "base\n"
    );
    assert!(!d.path().join("new.txt").exists());
    assert!(git(d.path(), &["status", "--porcelain"]).is_empty());
    assert!(w.stash_detail(&id, &e.id, "invented", &CANCEL).is_err());
}
#[test]
fn applying_keeps_record_restores_index_and_popping_removes_only_after_success() {
    let (d, mut w, id) = fixture();
    fs::write(d.path().join("file.txt"), "staged\n").unwrap();
    git(d.path(), &["add", "file.txt"]);
    fs::write(d.path().join("file.txt"), "working\n").unwrap();
    save(&mut w, &id, false);
    let page = w.stashes(&id, 0, &CANCEL).unwrap();
    let e = page.entries[0].clone();
    let applied = run(
        &mut w,
        &id,
        use_stash(e.id.clone(), StashOperation::Apply, true),
    );
    assert_eq!(git(d.path(), &["show", ":file.txt"]), "staged");
    assert_eq!(
        fs::read_to_string(d.path().join("file.txt")).unwrap(),
        "working\n"
    );
    assert_eq!(
        git(d.path(), &["rev-parse", &applied.backup_reference.unwrap()]),
        e.object
    );
    assert_eq!(w.stashes(&id, 0, &CANCEL).unwrap().entries.len(), 1);
    git(d.path(), &["reset", "--hard", "HEAD"]);
    let popped = run(&mut w, &id, use_stash(e.id, StashOperation::Pop, true));
    assert!(w.stashes(&id, 0, &CANCEL).unwrap().entries.is_empty());
    assert_eq!(
        git(d.path(), &["rev-parse", &popped.backup_reference.unwrap()]),
        e.object
    );
    assert_eq!(git(d.path(), &["show", ":file.txt"]), "staged");
}
#[test]
fn stale_stash_order_and_reviewed_state_cannot_drop_a_different_record() {
    let (d, mut w, id) = fixture();
    fs::write(d.path().join("file.txt"), "first\n").unwrap();
    save(&mut w, &id, false);
    let e = w.stashes(&id, 0, &CANCEL).unwrap().entries[0].clone();
    fs::write(d.path().join("file.txt"), "second\n").unwrap();
    save(&mut w, &id, false);
    let state = w.open(&id, &CANCEL).unwrap();
    assert!(
        w.prepare(
            &id,
            &state.revision,
            use_stash(e.id.clone(), StashOperation::Drop, false),
            &CANCEL
        )
        .is_err()
    );
    assert!(w.stash_detail(&id, &e.id, "tracked", &CANCEL).is_err());
    let page = w.stashes(&id, 0, &CANCEL).unwrap();
    let e = page.entries[1].clone();
    let p = w
        .prepare(
            &id,
            &page.state.revision,
            use_stash(e.id.clone(), StashOperation::Drop, false),
            &CANCEL,
        )
        .unwrap();
    assert!(p.dangerous);
    git(
        d.path(),
        &["stash", "store", "-m", "external store", &e.object],
    );
    assert!(w.apply(&p.id, &CANCEL).is_err());
    assert_eq!(git(d.path(), &["stash", "list"]).lines().count(), 3);
    let page = w.stashes(&id, 0, &CANCEL).unwrap();
    let e = page.entries[1].clone();
    let result = run(&mut w, &id, use_stash(e.id, StashOperation::Drop, false));
    assert_eq!(
        git(d.path(), &["rev-parse", &result.backup_reference.unwrap()]),
        e.object
    );
    assert_eq!(git(d.path(), &["stash", "list"]).lines().count(), 2);
}
#[test]
fn stash_branch_starts_at_saved_base_and_branch_collision_preserves_record() {
    let (d, mut w, id) = fixture();
    let base = git(d.path(), &["rev-parse", "HEAD"]);
    fs::write(d.path().join("file.txt"), "saved\n").unwrap();
    save(&mut w, &id, false);
    fs::write(d.path().join("later.txt"), "later\n").unwrap();
    git(d.path(), &["add", "."]);
    git(d.path(), &["commit", "-m", "later"]);
    let e = w.stashes(&id, 0, &CANCEL).unwrap().entries[0].clone();
    let p = w.open(&id, &CANCEL).unwrap();
    let collision = w
        .prepare(
            &id,
            &p.revision,
            Action::StashUse {
                id: e.id.clone(),
                operation: StashOperation::Branch,
                name: Some("main".into()),
                restore_index: false,
            },
            &CANCEL,
        )
        .unwrap();
    assert!(w.apply(&collision.id, &CANCEL).is_err());
    assert_eq!(w.stashes(&id, 0, &CANCEL).unwrap().entries.len(), 1);
    let result = run(
        &mut w,
        &id,
        Action::StashUse {
            id: e.id,
            operation: StashOperation::Branch,
            name: Some("from-stash".into()),
            restore_index: false,
        },
    );
    assert_eq!(result.state.repository.branch, "from-stash");
    assert_eq!(git(d.path(), &["rev-parse", "HEAD"]), base);
    assert_eq!(
        fs::read_to_string(d.path().join("file.txt")).unwrap(),
        "saved\n"
    );
    assert!(w.stashes(&id, 0, &CANCEL).unwrap().entries.is_empty());
    assert_eq!(
        git(d.path(), &["rev-parse", &result.backup_reference.unwrap()]),
        e.object
    );
}
#[test]
fn conflicted_pop_keeps_the_record_and_actual_conflicts() {
    let (d, mut w, id) = fixture();
    fs::write(d.path().join("file.txt"), "stash change\n").unwrap();
    save(&mut w, &id, false);
    fs::write(d.path().join("file.txt"), "current change\n").unwrap();
    git(d.path(), &["commit", "-am", "diverge"]);
    let page = w.stashes(&id, 0, &CANCEL).unwrap();
    let e = page.entries[0].clone();
    let p = w
        .prepare(
            &id,
            &page.state.revision,
            use_stash(e.id.clone(), StashOperation::Pop, false),
            &CANCEL,
        )
        .unwrap();
    assert!(w.apply(&p.id, &CANCEL).is_err());
    let state = w.open(&id, &CANCEL).unwrap();
    assert_eq!(state.repository.conflicts, 1);
    assert!(state.changes.iter().any(|f| f.conflicted));
    assert_eq!(
        w.stashes(&id, 0, &CANCEL).unwrap().entries[0].object,
        e.object
    );
    assert_eq!(
        git(d.path(), &["rev-parse", &p.backup_reference.unwrap()]),
        e.object
    );
}
#[test]
fn paged_records_remain_distinct_even_when_they_share_a_commit() {
    let (d, mut w, id) = fixture();
    fs::write(d.path().join("file.txt"), "save\n").unwrap();
    fs::write(d.path().join("untracked.txt"), "keep\n").unwrap();
    save(&mut w, &id, false);
    assert!(d.path().join("untracked.txt").exists());
    let first = w.stashes(&id, 0, &CANCEL).unwrap().entries[0].clone();
    assert!(
        w.stash_detail(&id, &first.id, "untracked", &CANCEL)
            .is_err()
    );
    let tree = git(
        d.path(),
        &["rev-parse", &format!("{}^{{tree}}", first.object)],
    );
    let other = git(
        d.path(),
        &[
            "commit-tree",
            &tree,
            "-p",
            &first.parents[0],
            "-p",
            &first.parents[1],
            "-m",
            "second stash object",
        ],
    );
    for n in 0..102 {
        let object = if n % 2 == 0 { &other } else { &first.object };
        git(
            d.path(),
            &["stash", "store", "-m", &format!("record {n}"), object],
        );
    }
    let page = w.stashes(&id, 0, &CANCEL).unwrap();
    assert_eq!(page.entries.len(), 100);
    assert!(page.has_more);
    let ids = page
        .entries
        .iter()
        .map(|e| &e.id)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(ids.len(), 100);
    let last = w.stashes(&id, 100, &CANCEL).unwrap();
    assert_eq!(last.entries.len(), 3);
    assert!(!last.has_more);
    assert_eq!(last.entries[0].position, 100);
    assert!(w.stashes(&id, 10001, &CANCEL).is_err());
    assert!(w.stash_detail(&id, "invented", "tracked", &CANCEL).is_err());
    assert!(w.stashes(&id, 0, &AtomicBool::new(true)).is_err());
}

#[test]
fn dirty_buffers_block_apply_at_review_and_execution_but_do_not_block_dropping_a_stash() {
    let (d, mut w, id) = fixture();
    fs::write(d.path().join("file.txt"), "saved\n").unwrap();
    save(&mut w, &id, false);
    let page = w.stashes(&id, 0, &CANCEL).unwrap();
    let e = page.entries[0].clone();
    let plan = w
        .prepare(
            &id,
            &page.state.revision,
            use_stash(e.id.clone(), StashOperation::Apply, false),
            &CANCEL,
        )
        .unwrap();
    let files = w.file_tree(&id, None, 0, &CANCEL).unwrap();
    let file = files.files.iter().find(|f| f.name == "file.txt").unwrap();
    let doc = w
        .source_open(&id, &file.id, None, &CANCEL)
        .unwrap()
        .document;
    w.source_dirty(&doc.id, true, 1).unwrap();
    assert!(w.apply(&plan.id, &CANCEL).is_err());
    let state = w.open(&id, &CANCEL).unwrap();
    assert!(
        w.prepare(
            &id,
            &state.revision,
            use_stash(e.id.clone(), StashOperation::Apply, false),
            &CANCEL
        )
        .is_err()
    );
    run(&mut w, &id, use_stash(e.id, StashOperation::Drop, false));
    assert!(w.has_dirty_sources());
    assert_eq!(
        fs::read_to_string(d.path().join("file.txt")).unwrap(),
        "base\n"
    );
    assert!(w.stashes(&id, 0, &CANCEL).unwrap().entries.is_empty());
}
#[test]
fn failed_stash_branch_keeps_the_created_branch_and_untracked_collision() {
    let (d, mut w, id) = fixture();
    fs::write(d.path().join("new.txt"), "saved file\n").unwrap();
    save(&mut w, &id, true);
    fs::write(d.path().join("new.txt"), "local file\n").unwrap();
    let page = w.stashes(&id, 0, &CANCEL).unwrap();
    let e = page.entries[0].clone();
    let p = w
        .prepare(
            &id,
            &page.state.revision,
            Action::StashUse {
                id: e.id,
                operation: StashOperation::Branch,
                name: Some("attempted".into()),
                restore_index: false,
            },
            &CANCEL,
        )
        .unwrap();
    assert!(w.apply(&p.id, &CANCEL).is_err());
    assert_eq!(git(d.path(), &["branch", "--show-current"]), "attempted");
    assert_eq!(
        fs::read_to_string(d.path().join("new.txt")).unwrap(),
        "local file\n"
    );
    assert_eq!(
        w.stashes(&id, 0, &CANCEL).unwrap().entries[0].object,
        e.object
    );
    assert_eq!(
        git(d.path(), &["rev-parse", &p.backup_reference.unwrap()]),
        e.object
    );
}
#[test]
fn stash_internal_literal_paths_are_preserved_without_global_literal_pathspec_mode() {
    let (d, mut w, id) = fixture();
    let name = if cfg!(windows) {
        "[literal].txt"
    } else {
        ":(glob)*.txt"
    };
    fs::write(d.path().join(name), "literal content\n").unwrap();
    save(&mut w, &id, true);
    assert!(!d.path().join(name).exists());
    let e = w.stashes(&id, 0, &CANCEL).unwrap().entries[0].clone();
    run(&mut w, &id, use_stash(e.id, StashOperation::Apply, false));
    assert_eq!(
        fs::read_to_string(d.path().join(name)).unwrap(),
        "literal content\n"
    );
    assert_eq!(
        fs::read_to_string(d.path().join("file.txt")).unwrap(),
        "base\n"
    );
}

#[test]
fn captured_records_are_repository_scoped_and_bare_repositories_can_inspect_only() {
    let (d, mut w, id) = fixture();
    fs::write(d.path().join("file.txt"), "saved\n").unwrap();
    save(&mut w, &id, false);
    let object = git(d.path(), &["rev-parse", "refs/stash"]);
    let (other, _other_workspace, other_id) = fixture();
    w.scan(
        ScanSettings {
            roots: vec![
                d.path().to_str().unwrap().into(),
                other.path().to_str().unwrap().into(),
            ],
            exclusions: vec![],
        },
        &CANCEL,
    )
    .unwrap();
    let entry = w.stashes(&id, 0, &CANCEL).unwrap().entries[0].clone();
    assert!(
        w.stash_detail(&other_id, &entry.id, "tracked", &CANCEL)
            .is_err()
    );
    let bare = tempfile::tempdir().unwrap();
    git(bare.path(), &["init", "--bare"]);
    git(
        bare.path(),
        &["fetch", d.path().to_str().unwrap(), "refs/stash"],
    );
    git(
        bare.path(),
        &["update-ref", "--create-reflog", "refs/stash", &object],
    );
    let catalog = w
        .scan(
            ScanSettings {
                roots: vec![bare.path().to_str().unwrap().into()],
                exclusions: vec![],
            },
            &CANCEL,
        )
        .unwrap();
    let id = &catalog.repositories[0].id;
    let page = w.stashes(id, 0, &CANCEL).unwrap();
    assert_eq!(page.entries[0].object, object);
    assert!(
        !w.stash_detail(id, &page.entries[0].id, "tracked", &CANCEL)
            .unwrap()
            .detail
            .files
            .is_empty()
    );
    assert!(
        w.prepare(
            id,
            &page.state.revision,
            use_stash(page.entries[0].id.clone(), StashOperation::Drop, false),
            &CANCEL
        )
        .is_err()
    );
}
