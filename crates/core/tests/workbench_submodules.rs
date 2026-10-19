use clear_contracts::workbench::*;
use clear_core::workbench::Workspace;
use std::{fs, path::Path, process::Command, sync::atomic::AtomicBool};
static CANCEL: AtomicBool = AtomicBool::new(false);
fn git(root: &Path, args: &[&str]) -> String {
    let r = Command::new("git")
        .current_dir(root)
        .args(["-c", "protocol.file.allow=always"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        r.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&r.stderr)
    );
    String::from_utf8_lossy(&r.stdout).trim().into()
}
fn init(root: &Path) {
    fs::create_dir_all(root).unwrap();
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "user.name", "Module Test"]);
    git(root, &["config", "user.email", "modules@example.com"]);
    git(root, &["config", "commit.gpgsign", "false"]);
    fs::write(root.join("tracked.txt"), "initial\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "initial"]);
}
fn fixture() -> (tempfile::TempDir, Workspace, String) {
    let t = tempfile::tempdir().unwrap();
    let (leaf, middle, top) = (
        t.path().join("leaf"),
        t.path().join("middle"),
        t.path().join("top"),
    );
    init(&leaf);
    init(&middle);
    git(
        &middle,
        &["submodule", "add", leaf.to_str().unwrap(), "nested/leaf"],
    );
    git(&middle, &["commit", "-am", "add nested"]);
    init(&top);
    git(
        &top,
        &[
            "submodule",
            "add",
            middle.to_str().unwrap(),
            "libs/module with space",
        ],
    );
    git(&top, &["commit", "-am", "add module"]);
    git(&top, &["submodule", "deinit", "-f", "--all"]);
    let mut w = Workspace::default();
    let catalog = w
        .scan(
            ScanSettings {
                roots: vec![top.display().to_string()],
                exclusions: vec![],
            },
            &CANCEL,
        )
        .unwrap();
    let id = catalog.repositories[0].id.clone();
    (t, w, id)
}
fn plan(w: &mut Workspace, id: &str, a: Action) -> ActionPlan {
    let s = w.open(id, &CANCEL).unwrap();
    w.prepare(id, &s.revision, a, &CANCEL).unwrap()
}
fn apply(w: &mut Workspace, id: &str, a: Action) -> ActionResult {
    let p = plan(w, id, a);
    assert!(p.dangerous);
    w.apply(&p.id, &CANCEL).unwrap()
}
#[test]
fn registration_is_separate_from_checkout_and_recursive_update_opens_nested_repositories() {
    let (t, mut w, id) = fixture();
    let list = w.submodules(&id, &CANCEL).unwrap();
    assert_eq!(list.entries.len(), 1);
    assert_eq!(list.entries[0].status, "uninitialized");
    assert!(!list.entries[0].registered);
    let selected = list.entries[0].id.clone();
    let result = apply(
        &mut w,
        &id,
        Action::InitializeSubmodules { id: Some(selected) },
    );
    assert!(result.recovery_path.is_some());
    assert!(
        !t.path()
            .join("top/libs/module with space/tracked.txt")
            .exists()
    );
    let list = w.submodules(&id, &CANCEL).unwrap();
    assert!(list.entries[0].registered);
    apply(
        &mut w,
        &id,
        Action::UpdateSubmodules {
            id: Some(list.entries[0].id.clone()),
            recursive: true,
        },
    );
    let list = w.submodules(&id, &CANCEL).unwrap();
    assert_eq!(list.entries.len(), 2);
    assert!(
        list.entries
            .iter()
            .all(|e| e.initialized && e.status == "current")
    );
    assert_eq!(list.entries[1].depth, 1);
    assert!(Path::new(&list.entries[1].path).ends_with("nested/leaf"));
    let opened = w.open_submodule(&id, &list.entries[1].id, &CANCEL).unwrap();
    assert_eq!(
        fs::read_to_string(Path::new(&opened.state.repository.path).join("tracked.txt")).unwrap(),
        "initial\n"
    );
    assert!(
        opened
            .catalog
            .repositories
            .iter()
            .any(|r| r.id == opened.state.repository.id)
    );
}
#[test]
fn nonrecursive_update_stops_at_the_selected_module_and_nested_action_uses_its_parent() {
    let (t, mut w, id) = fixture();
    apply(
        &mut w,
        &id,
        Action::UpdateSubmodules {
            id: None,
            recursive: false,
        },
    );
    let list = w.submodules(&id, &CANCEL).unwrap();
    assert!(list.entries[0].initialized);
    assert!(!list.entries[1].initialized);
    let nested = list.entries[1].id.clone();
    let p = plan(
        &mut w,
        &id,
        Action::UpdateSubmodules {
            id: Some(nested),
            recursive: false,
        },
    );
    assert!(Path::new(p.working_directory.as_ref().unwrap()).ends_with("libs/module with space"));
    w.apply(&p.id, &CANCEL).unwrap();
    assert!(
        t.path()
            .join("top/libs/module with space/nested/leaf/tracked.txt")
            .exists()
    );
}
#[test]
fn dirty_submodule_and_unsaved_buffers_are_preserved_and_late_changes_invalidate_review() {
    let (t, mut w, id) = fixture();
    apply(
        &mut w,
        &id,
        Action::UpdateSubmodules {
            id: None,
            recursive: true,
        },
    );
    let root = t.path().join("top/libs/module with space");
    fs::write(root.join("tracked.txt"), "local edit\n").unwrap();
    let list = w.submodules(&id, &CANCEL).unwrap();
    assert_eq!(list.entries[0].status, "dirty");
    let s = w.open(&id, &CANCEL).unwrap();
    assert!(
        w.prepare(
            &id,
            &s.revision,
            Action::UpdateSubmodules {
                id: None,
                recursive: true
            },
            &CANCEL
        )
        .is_err()
    );
    assert_eq!(
        fs::read_to_string(root.join("tracked.txt")).unwrap(),
        "local edit\n"
    );
    git(&root, &["restore", "tracked.txt"]);
    let list = w.submodules(&id, &CANCEL).unwrap();
    let opened = w.open_submodule(&id, &list.entries[0].id, &CANCEL).unwrap();
    let child = opened.state.repository.id;
    let file = w
        .file_tree(&child, None, 0, &CANCEL)
        .unwrap()
        .files
        .into_iter()
        .find(|f| f.name == "tracked.txt")
        .unwrap();
    let doc = w.source_open(&child, &file.id, None, &CANCEL).unwrap();
    w.source_dirty(&doc.document.id, true, 1).unwrap();
    let s = w.open(&id, &CANCEL).unwrap();
    assert!(
        w.prepare(
            &id,
            &s.revision,
            Action::UpdateSubmodules {
                id: None,
                recursive: true
            },
            &CANCEL
        )
        .is_err()
    );
    w.source_close(&doc.document.id, true).unwrap();
    let p = plan(
        &mut w,
        &id,
        Action::UpdateSubmodules {
            id: None,
            recursive: true,
        },
    );
    fs::write(root.join("new.txt"), "arrived later").unwrap();
    assert!(w.apply(&p.id, &CANCEL).is_err());
    assert!(root.join("new.txt").exists());
}
#[test]
fn stale_selections_bad_paths_symlinks_cancellation_and_foreign_ids_do_not_write() {
    let (t, mut w, id) = fixture();
    let list = w.submodules(&id, &CANCEL).unwrap();
    assert!(w.open_submodule(&id, &list.entries[0].id, &CANCEL).is_err());
    let s = w.open(&id, &CANCEL).unwrap();
    assert!(
        w.prepare(
            &id,
            &s.revision,
            Action::InitializeSubmodules {
                id: Some("forged".into())
            },
            &CANCEL
        )
        .is_err()
    );
    assert!(w.submodules(&id, &AtomicBool::new(true)).is_err());
    assert!(
        w.open_submodule("foreign", &list.entries[0].id, &CANCEL)
            .is_err()
    );
    let p = plan(
        &mut w,
        &id,
        Action::InitializeSubmodules {
            id: Some(list.entries[0].id.clone()),
        },
    );
    fs::write(
        t.path().join("top/.gitmodules"),
        "[submodule \"bad\"]\npath = ../outside\nurl = https://example.com/module.git\n",
    )
    .unwrap();
    assert!(w.apply(&p.id, &CANCEL).is_err());
    assert!(w.submodules(&id, &CANCEL).is_err());
    assert!(!t.path().join("outside").exists());
}
#[test]
fn failed_clone_records_failure_and_keeps_registration_without_retry() {
    let (t, mut w, id) = fixture();
    git(
        &t.path().join("top"),
        &[
            "config",
            "submodule.libs/module with space.url",
            "/nonexistent/example/repository.git",
        ],
    );
    fs::remove_dir_all(t.path().join("top/.git/modules")).unwrap();
    let p = plan(
        &mut w,
        &id,
        Action::UpdateSubmodules {
            id: None,
            recursive: false,
        },
    );
    let error = w.apply(&p.id, &CANCEL).unwrap_err().to_string();
    assert!(error.contains("Submodule recovery:"));
    let journal = w.repositories[&id]
        .common
        .join("clear-compare-submodule-operations")
        .join(&p.id);
    assert_eq!(
        fs::read_to_string(journal.join("result.json")).unwrap(),
        "{\"success\":false}"
    );
    assert!(journal.join("before.json").exists());
    assert!(w.apply(&p.id, &CANCEL).is_err());
}

#[test]
fn cached_detached_heads_are_protected_even_when_checkout_was_deinitialized() {
    let (t, mut w, id) = fixture();
    apply(
        &mut w,
        &id,
        Action::UpdateSubmodules {
            id: None,
            recursive: true,
        },
    );
    let leaf = t.path().join("top/libs/module with space/nested/leaf");
    git(&leaf, &["config", "user.name", "Cached Test"]);
    git(&leaf, &["config", "user.email", "cached@example.com"]);
    let tree = git(&leaf, &["rev-parse", "HEAD^{tree}"]);
    let old = git(&leaf, &["rev-parse", "HEAD"]);
    let unique = git(
        &leaf,
        &["commit-tree", &tree, "-p", &old, "-m", "cached-only commit"],
    );
    git(&leaf, &["update-ref", "HEAD", &unique]);
    let admin = git(&leaf, &["rev-parse", "--absolute-git-dir"]);
    git(
        &t.path().join("top"),
        &["submodule", "deinit", "-f", "--all"],
    );
    let list = w.submodules(&id, &CANCEL).unwrap();
    assert_eq!(list.entries.len(), 1);
    assert!(!list.entries[0].initialized);
    let p = plan(
        &mut w,
        &id,
        Action::UpdateSubmodules {
            id: None,
            recursive: true,
        },
    );
    let result = w.apply(&p.id, &CANCEL).unwrap();
    let refs = git(
        Path::new(&admin),
        &[
            "for-each-ref",
            "--format=%(objectname)",
            "refs/clear-compare/submodules",
        ],
    );
    assert!(refs.lines().any(|s| s == unique));
    assert_ne!(git(&leaf, &["rev-parse", "HEAD"]), unique);
    let before: serde_json::Value = serde_json::from_slice(
        &fs::read(Path::new(result.recovery_path.as_ref().unwrap()).join("before.json")).unwrap(),
    )
    .unwrap();
    assert!(
        before["references"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["head"] == unique)
    );
}
#[test]
fn displayed_urls_are_redacted_but_private_configuration_backup_is_exact() {
    let (t, mut w, id) = fixture();
    let top = t.path().join("top");
    let secret =
        "https://alice:example-secret@example.com/team/module.git?token=private-query#fragment";
    git(
        &top,
        &[
            "config",
            "--file",
            ".gitmodules",
            "submodule.libs/module with space.url",
            secret,
        ],
    );
    git(
        &top,
        &["config", "submodule.libs/module with space.url", secret],
    );
    let list = w.submodules(&id, &CANCEL).unwrap();
    let json = serde_json::to_string(&list).unwrap();
    assert!(!json.contains("example-secret") && !json.contains("private-query"));
    let p = plan(&mut w, &id, Action::InitializeSubmodules { id: None });
    assert!(
        !serde_json::to_string(&p)
            .unwrap()
            .contains("example-secret")
    );
    let original = fs::read(top.join(".git/config")).unwrap();
    let result = w.apply(&p.id, &CANCEL).unwrap();
    let root = Path::new(result.recovery_path.as_ref().unwrap());
    let before: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("before.json")).unwrap()).unwrap();
    assert!(
        before["configs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|c| fs::read(root.join(c["file"].as_str().unwrap())).unwrap() == original)
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(root).unwrap().permissions().mode() & 0o777,
            0o700
        );
        for c in before["configs"].as_array().unwrap() {
            assert_eq!(
                fs::metadata(root.join(c["file"].as_str().unwrap()))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
}
#[cfg(unix)]
#[test]
fn literal_paths_are_not_globs_and_explicit_checkout_does_not_run_custom_update_commands() {
    let (t, mut w, id) = fixture();
    let top = t.path().join("top");
    let middle = t.path().join("middle");
    let target = git(&middle, &["rev-parse", "HEAD"]);
    git(
        &top,
        &[
            "--literal-pathspecs",
            "update-index",
            "--add",
            "--cacheinfo",
            &format!("160000,{target},:(glob)*"),
        ],
    );
    git(
        &top,
        &[
            "config",
            "--file",
            ".gitmodules",
            "submodule.literal.path",
            ":(glob)*",
        ],
    );
    git(
        &top,
        &[
            "config",
            "--file",
            ".gitmodules",
            "submodule.literal.url",
            middle.to_str().unwrap(),
        ],
    );
    let marker = t.path().join("custom-update-ran");
    git(
        &top,
        &[
            "config",
            "submodule.literal.update",
            &format!("!touch {}", marker.display()),
        ],
    );
    let list = w.submodules(&id, &CANCEL).unwrap();
    let e = list.entries.iter().find(|e| e.name == "literal").unwrap();
    apply(
        &mut w,
        &id,
        Action::UpdateSubmodules {
            id: Some(e.id.clone()),
            recursive: false,
        },
    );
    assert!(top.join(":(glob)*/tracked.txt").exists());
    assert!(!top.join("libs/module with space/tracked.txt").exists());
    assert!(!marker.exists());
}
#[test]
fn configuration_lock_failures_keep_the_existing_lock_and_original_configuration() {
    let (t, mut w, id) = fixture();
    let top = t.path().join("top");
    let original = fs::read(top.join(".git/config")).unwrap();
    let p = plan(&mut w, &id, Action::InitializeSubmodules { id: None });
    fs::write(top.join(".git/config.lock"), "another process").unwrap();
    let error = w.apply(&p.id, &CANCEL).unwrap_err().to_string();
    assert!(error.contains("Submodule recovery:"));
    assert_eq!(fs::read(top.join(".git/config")).unwrap(), original);
    assert_eq!(
        fs::read_to_string(top.join(".git/config.lock")).unwrap(),
        "another process"
    );
    let root = w.repositories[&id]
        .common
        .join("clear-compare-submodule-operations")
        .join(&p.id);
    assert_eq!(
        fs::read_to_string(root.join("result.json")).unwrap(),
        "{\"success\":false}"
    );
}

#[cfg(unix)]
#[test]
fn symlinked_submodule_directories_fail_closed_without_following_or_writing() {
    use std::os::unix::fs::symlink;
    let (t, mut w, id) = fixture();
    let state = w.open(&id, &CANCEL).unwrap();
    let target = t.path().join("top/libs/module with space");
    fs::remove_dir(&target).unwrap();
    symlink(t.path().join("leaf"), &target).unwrap();
    assert!(
        w.submodules(&id, &CANCEL)
            .unwrap_err()
            .to_string()
            .contains("symbolic link")
    );
    assert!(
        w.prepare(
            &id,
            &state.revision,
            Action::UpdateSubmodules {
                id: None,
                recursive: true
            },
            &CANCEL
        )
        .is_err()
    );
    assert_eq!(
        fs::read_to_string(t.path().join("leaf/tracked.txt")).unwrap(),
        "initial\n"
    );
}
