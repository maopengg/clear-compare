use clear_contracts::workbench::ScanSettings;
use clear_core::workbench::{Workspace, remote_key};
use std::{fs, path::Path, process::Command, sync::atomic::AtomicBool};
fn git(root: &Path, args: &[&str]) -> String {
    let result = Command::new("git")
        .current_dir(root)
        .args([
            "-c",
            "user.name=Workspace Test",
            "-c",
            "user.email=workspace@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    String::from_utf8_lossy(&result.stdout).trim().into()
}
fn init(root: &Path) {
    fs::create_dir_all(root).unwrap();
    git(root, &["init", "-b", "main"]);
    fs::write(root.join("tracked.txt"), "initial\n").unwrap();
    git(root, &["add", "tracked.txt"]);
    git(root, &["commit", "-m", "initial"]);
}
#[test]
fn remote_identity_matches_transports_and_never_includes_credentials() {
    assert_eq!(
        remote_key("git@GitHub.com:owner/project.git"),
        remote_key("https://github.com/owner/project/")
    );
    assert_eq!(
        remote_key("ssh://git@example.com:22/team/project.git"),
        remote_key("https://user:secret@example.com/team/project.git")
    );
    assert_ne!(
        remote_key("https://example.com/a/project"),
        remote_key("https://example.com/b/project")
    );
}
#[test]
fn discovers_nested_bare_submodule_and_worktree_without_merging_local_copies() {
    let base = tempfile::tempdir().unwrap();
    let root = base.path();
    let a = root.join("a");
    let b = root.join("b");
    init(&a);
    init(&b);
    git(
        &a,
        &["remote", "add", "origin", "git@example.com:team/shared.git"],
    );
    git(
        &b,
        &[
            "remote",
            "add",
            "origin",
            "https://example.com/team/shared.git",
        ],
    );
    let nested = a.join("nested");
    init(&nested);
    fs::create_dir(root.join("bare.git")).unwrap();
    git(&root.join("bare.git"), &["init", "--bare"]);
    let work = root.join("parallel");
    git(
        &a,
        &["worktree", "add", "-b", "parallel", work.to_str().unwrap()],
    );
    git(
        &a,
        &[
            "-c",
            "protocol.file.allow=always",
            "submodule",
            "add",
            b.to_str().unwrap(),
            "modules/child",
        ],
    );
    let mut workspace = Workspace::default();
    let catalog = workspace
        .scan(
            ScanSettings {
                roots: vec![root.display().to_string(), a.display().to_string()],
                exclusions: vec![],
            },
            &AtomicBool::new(false),
        )
        .unwrap();
    assert_eq!(catalog.repositories.len(), 6, "{:?}", catalog.repositories);
    assert_eq!(
        catalog
            .repositories
            .iter()
            .filter(|r| r.kind == "bare")
            .count(),
        1
    );
    assert_eq!(
        catalog
            .repositories
            .iter()
            .filter(|r| r.kind == "worktree")
            .count(),
        1
    );
    assert_eq!(
        catalog
            .repositories
            .iter()
            .filter(|r| r.kind == "submodule")
            .count(),
        1
    );
    let a_view = catalog
        .repositories
        .iter()
        .find(|r| r.path == fs::canonicalize(&a).unwrap().display().to_string())
        .unwrap();
    let b_view = catalog
        .repositories
        .iter()
        .find(|r| r.path == fs::canonicalize(&b).unwrap().display().to_string())
        .unwrap();
    assert_eq!(a_view.match_key, b_view.match_key);
    assert_ne!(a_view.id, b_view.id);
    let work_view = catalog
        .repositories
        .iter()
        .find(|r| r.kind == "worktree")
        .unwrap();
    assert_eq!(work_view.common_id, a_view.common_id);
    let stored = root.join("settings");
    workspace.favorite(&a_view.id, true).unwrap();
    workspace.save(&stored).unwrap();
    let mut reopened = Workspace::load(&stored).unwrap();
    assert!(
        reopened
            .catalog
            .repositories
            .iter()
            .find(|r| r.id == a_view.id)
            .unwrap()
            .favorite
    );
    assert!(reopened.open(&a_view.id, &AtomicBool::new(false)).is_ok());
}
#[test]
fn reads_changes_references_and_remotes_without_modifying_index_or_files() {
    let base = tempfile::tempdir().unwrap();
    let repo = base.path().join("project");
    init(&repo);
    fs::write(repo.join("tracked.txt"), "modified\n").unwrap();
    fs::write(repo.join("staged file.txt"), "staged\n").unwrap();
    git(&repo, &["add", "staged file.txt"]);
    fs::write(repo.join("untracked file.txt"), "fresh").unwrap();
    git(
        &repo,
        &[
            "remote",
            "add",
            "origin",
            "https://user:secret@example.com/team/project.git",
        ],
    );
    git(&repo, &["branch", "feature/test"]);
    git(&repo, &["tag", "v1"]);
    let index = fs::read(repo.join(".git/index")).unwrap();
    let before = git(&repo, &["status", "--porcelain=v2", "-z"]);
    let mut workspace = Workspace::default();
    let catalog = workspace
        .scan(
            ScanSettings {
                roots: vec![base.path().display().to_string()],
                exclusions: vec![],
            },
            &AtomicBool::new(false),
        )
        .unwrap();
    let repository_id = catalog.repositories[0].id.clone();
    let state = workspace
        .open(&repository_id, &AtomicBool::new(false))
        .unwrap();
    assert_eq!(state.changes.len(), 3);
    assert!(
        state
            .changes
            .iter()
            .any(|c| c.untracked && c.path == "untracked file.txt")
    );
    assert!(
        state
            .changes
            .iter()
            .any(|c| c.index == "A" && c.path == "staged file.txt")
    );
    assert!(state.references.iter().any(|r| r.name == "feature/test"));
    assert!(
        state
            .references
            .iter()
            .any(|r| r.kind == "tag" && r.name == "v1")
    );
    assert!(!serde_json::to_string(&state).unwrap().contains("secret"));
    assert_eq!(index, fs::read(repo.join(".git/index")).unwrap());
    assert_eq!(before, git(&repo, &["status", "--porcelain=v2", "-z"]));
    assert!(
        workspace
            .open("foreign-id", &AtomicBool::new(false))
            .is_err()
    );
}
#[test]
fn scan_exclusions_errors_and_cancellation_preserve_the_previous_catalog() {
    let base = tempfile::tempdir().unwrap();
    init(&base.path().join("keep"));
    init(&base.path().join("skip/hidden"));
    let settings = ScanSettings {
        roots: vec![base.path().display().to_string()],
        exclusions: vec!["skip".into()],
    };
    let mut workspace = Workspace::default();
    let catalog = workspace
        .scan(settings.clone(), &AtomicBool::new(false))
        .unwrap();
    assert_eq!(catalog.repositories.len(), 1);
    assert!(workspace.scan(settings, &AtomicBool::new(true)).is_err());
    assert_eq!(workspace.catalog.repositories.len(), 1);
    assert!(
        workspace
            .scan(
                ScanSettings {
                    roots: vec![base.path().display().to_string()],
                    exclusions: vec!["../escape".into()]
                },
                &AtomicBool::new(false)
            )
            .is_err()
    );
    assert_eq!(workspace.catalog.repositories.len(), 1);
}

#[test]
fn removing_a_repository_survives_rescans_and_restart_without_deleting_its_files() {
    let base = tempfile::tempdir().unwrap();
    let repo = base.path().join("repo");
    init(&repo);
    let settings = ScanSettings {
        roots: vec![base.path().display().to_string()],
        exclusions: vec![],
    };
    let mut w = Workspace::default();
    let catalog = w.scan(settings.clone(), &AtomicBool::new(false)).unwrap();
    let id = catalog.repositories[0].id.clone();
    w.remove(&id).unwrap();
    assert!(repo.join(".git").exists());
    assert!(
        w.scan(settings.clone(), &AtomicBool::new(false))
            .unwrap()
            .repositories
            .is_empty()
    );
    let storage = base.path().join("settings");
    w.save(&storage).unwrap();
    let mut w = Workspace::load(&storage).unwrap();
    assert!(
        w.scan(settings, &AtomicBool::new(false))
            .unwrap()
            .repositories
            .is_empty()
    );
}
