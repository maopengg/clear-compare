use clear_contracts::workbench::*;
use clear_contracts::{Status, TextOptions};
use clear_core::workbench::Workspace;
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::AtomicBool,
};
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
fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf, Workspace, String) {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("local");
    let origin = d.path().join("origin.git");
    fs::create_dir(&root).unwrap();
    git(&root, &["init", "-b", "main"]);
    user(&root);
    fs::write(root.join("file.txt"), "initial\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "initial"]);
    git(d.path(), &["init", "--bare", origin.to_str().unwrap()]);
    git(
        &root,
        &["remote", "add", "origin", origin.to_str().unwrap()],
    );
    git(&root, &["push", "-u", "origin", "main"]);
    git(&root, &["branch", "feature"]);
    git(&root, &["push", "origin", "feature"]);
    git(
        &root,
        &["branch", "--set-upstream-to", "origin/feature", "feature"],
    );
    let mut w = Workspace::default();
    let c = w
        .scan(
            ScanSettings {
                roots: vec![root.display().to_string()],
                exclusions: vec![],
            },
            &CANCEL,
        )
        .unwrap();
    let id = c.repositories[0].id.clone();
    (d, root, origin, w, id)
}
fn request(state: &RepositoryState, name: &str, target: CompareTarget) -> ReferenceComparison {
    ReferenceComparison {
        left_reference_id: state
            .references
            .iter()
            .find(|r| r.full_name == name)
            .unwrap()
            .id
            .clone(),
        target,
    }
}
#[test]
fn reference_ids_pin_commits_and_arbitrary_targets_without_changing_checkout() {
    let (_d, root, _origin, mut w, id) = fixture();
    fs::write(root.join("file.txt"), "main changed\n").unwrap();
    git(&root, &["commit", "-am", "main change"]);
    let state = w.open(&id, &CANCEL).unwrap();
    let target = state
        .references
        .iter()
        .find(|r| r.full_name == "refs/heads/main")
        .unwrap()
        .id
        .clone();
    let req = request(
        &state,
        "refs/heads/feature",
        CompareTarget::Reference { id: target },
    );
    let snapshot = w
        .compare_references(&id, &state.revision, &req, &CANCEL)
        .unwrap();
    let row = snapshot
        .report
        .rows
        .iter()
        .find(|r| r.path == "file.txt")
        .unwrap();
    assert_eq!(row.status, Status::Changed);
    fs::write(root.join("file.txt"), "later\n").unwrap();
    git(&root, &["commit", "-am", "later change"]);
    git(&root, &["branch", "-f", "feature", "main"]);
    let text = snapshot
        .text_diff(&row.id, &TextOptions::default(), &CANCEL)
        .unwrap();
    assert!(
        text.lines
            .iter()
            .any(|l| l.tag == "delete" && l.text == "initial\n")
    );
    assert!(
        text.lines
            .iter()
            .any(|l| l.tag == "insert" && l.text == "main changed\n")
    );
    assert!(!text.lines.iter().any(|l| l.text == "later\n"));
    assert_eq!(git(&root, &["branch", "--show-current"]), "main");
    assert!(git(&root, &["status", "--porcelain"]).is_empty());
    assert!(
        w.compare_references(&id, &state.revision, &req, &CANCEL)
            .is_err()
    );
}
#[test]
fn working_tree_inclusion_exclusions_and_external_changes_are_checked() {
    let (_d, root, _origin, mut w, id) = fixture();
    fs::write(root.join("file.txt"), "disk edit\n").unwrap();
    fs::write(root.join("new.txt"), "untracked\n").unwrap();
    fs::write(root.join(".gitignore"), "ignored.txt\n").unwrap();
    fs::write(root.join("ignored.txt"), "ignored\n").unwrap();
    let state = w.open(&id, &CANCEL).unwrap();
    let mut req = request(
        &state,
        "refs/heads/main",
        CompareTarget::WorkingTree {
            include_untracked: false,
        },
    );
    let snapshot = w
        .compare_references(&id, &state.revision, &req, &CANCEL)
        .unwrap();
    assert!(!snapshot.report.rows.iter().any(|r| r.path == "new.txt"));
    req.target = CompareTarget::WorkingTree {
        include_untracked: true,
    };
    w.catalog.file_settings.insert(
        id.clone(),
        FileSettings {
            exclusions: "new.txt".into(),
            ..Default::default()
        },
    );
    let snapshot = w
        .compare_references(&id, &state.revision, &req, &CANCEL)
        .unwrap();
    assert!(
        !snapshot
            .report
            .rows
            .iter()
            .any(|r| matches!(r.path.as_str(), "new.txt" | "ignored.txt"))
    );
    w.catalog
        .file_settings
        .get_mut(&id)
        .unwrap()
        .exclusions
        .clear();
    let snapshot = w
        .compare_references(&id, &state.revision, &req, &CANCEL)
        .unwrap();
    assert_eq!(
        snapshot
            .report
            .rows
            .iter()
            .find(|r| r.path == "new.txt")
            .unwrap()
            .status,
        Status::RightOnly
    );
    let row = snapshot
        .report
        .rows
        .iter()
        .find(|r| r.path == "file.txt")
        .unwrap();
    assert!(
        snapshot
            .text_diff(&row.id, &TextOptions::default(), &CANCEL)
            .is_ok()
    );
    fs::write(root.join("file.txt"), "external\n").unwrap();
    assert!(
        snapshot
            .text_diff(&row.id, &TextOptions::default(), &CANCEL)
            .is_err()
    );
}
#[test]
fn captured_index_and_annotated_tags_use_object_content() {
    let (_d, root, _origin, mut w, id) = fixture();
    git(&root, &["tag", "-a", "release", "-m", "annotated release"]);
    fs::write(root.join("file.txt"), "indexed\n").unwrap();
    git(&root, &["add", "file.txt"]);
    fs::write(root.join("file.txt"), "disk only\n").unwrap();
    let state = w.open(&id, &CANCEL).unwrap();
    let req = request(&state, "refs/tags/release", CompareTarget::Index);
    let snapshot = w
        .compare_references(&id, &state.revision, &req, &CANCEL)
        .unwrap();
    let row = snapshot
        .report
        .rows
        .iter()
        .find(|r| r.path == "file.txt")
        .unwrap();
    git(&root, &["add", "file.txt"]);
    let text = snapshot
        .text_diff(&row.id, &TextOptions::default(), &CANCEL)
        .unwrap();
    assert!(text.lines.iter().any(|l| l.text == "indexed\n"));
    assert!(!text.lines.iter().any(|l| l.text == "disk only\n"));
}
#[test]
fn invented_selections_cancel_and_bare_working_sources_are_rejected() {
    let (_d, _root, origin, mut w, id) = fixture();
    let state = w.open(&id, &CANCEL).unwrap();
    let mut req = request(&state, "refs/heads/main", CompareTarget::Current);
    let equal = w
        .compare_references(&id, &state.revision, &req, &CANCEL)
        .unwrap();
    assert!(equal.report.rows.iter().all(|r| r.status == Status::Same));
    assert!(equal.page(0, "", true).unwrap().rows.is_empty());
    assert!(!equal.page(0, "", false).unwrap().rows.is_empty());
    req.left_reference_id = "invented".into();
    assert!(
        w.compare_references(&id, &state.revision, &req, &CANCEL)
            .is_err()
    );
    req = request(
        &state,
        "refs/heads/main",
        CompareTarget::Reference {
            id: "invented".into(),
        },
    );
    assert!(
        w.compare_references(&id, &state.revision, &req, &CANCEL)
            .is_err()
    );
    let cancel = AtomicBool::new(true);
    assert!(
        w.compare_references(&id, &state.revision, &req, &cancel)
            .is_err()
    );
    let catalog = w
        .scan(
            ScanSettings {
                roots: vec![origin.to_str().unwrap().into()],
                exclusions: vec![],
            },
            &CANCEL,
        )
        .unwrap();
    let id = catalog.repositories[0].id.clone();
    let state = w.open(&id, &CANCEL).unwrap();
    let main = state
        .references
        .iter()
        .find(|r| r.name == "main")
        .unwrap()
        .id
        .clone();
    let req = ReferenceComparison {
        left_reference_id: main.clone(),
        target: CompareTarget::Reference { id: main },
    };
    assert!(
        w.compare_references(&id, &state.revision, &req, &CANCEL)
            .is_ok()
    );
    for target in [
        CompareTarget::Index,
        CompareTarget::WorkingTree {
            include_untracked: false,
        },
    ] {
        let req = ReferenceComparison {
            target,
            ..req.clone()
        };
        assert!(
            w.compare_references(&id, &state.revision, &req, &CANCEL)
                .is_err()
        );
    }
}
