use clear_contracts::workbench::*;
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
fn run(w: &mut Workspace, id: &str, action: Action) -> ActionResult {
    let state = w.open(id, &CANCEL).unwrap();
    let plan = w.prepare(id, &state.revision, action, &CANCEL).unwrap();
    w.apply(&plan.id, &CANCEL).unwrap()
}
#[test]
fn explicit_remote_checkout_tracks_without_overwriting_and_new_branch_can_switch() {
    let (_d, root, _origin, mut w, id) = fixture();
    let result = run(
        &mut w,
        &id,
        Action::CheckoutRemote {
            reference: "refs/remotes/origin/feature".into(),
            name: "remote-copy".into(),
        },
    );
    assert_eq!(result.state.repository.branch, "remote-copy");
    assert_eq!(
        git(&root, &["config", "branch.remote-copy.remote"]),
        "origin"
    );
    assert_eq!(
        git(&root, &["config", "branch.remote-copy.merge"]),
        "refs/heads/feature"
    );
    let state = w.open(&id, &CANCEL).unwrap();
    assert!(
        w.prepare(
            &id,
            &state.revision,
            Action::CheckoutRemote {
                reference: "origin/feature".into(),
                name: "main".into()
            },
            &CANCEL
        )
        .is_err()
    );
    assert!(
        w.prepare(
            &id,
            &state.revision,
            Action::Switch {
                reference: "origin/feature".into(),
                detach: false
            },
            &CANCEL
        )
        .is_err()
    );
    let result = run(
        &mut w,
        &id,
        Action::CreateAndSwitch {
            name: "new-local".into(),
            start: Some("refs/heads/main".into()),
        },
    );
    assert_eq!(result.state.repository.branch, "new-local");
    assert_eq!(
        result.state.reference_settings.recent[0],
        "refs/heads/new-local"
    );
}
#[test]
fn favorite_and_recent_references_persist_migrate_on_rename_and_rollback() {
    let (d, root, _origin, mut w, id) = fixture();
    let base = d.path().join("settings");
    run(
        &mut w,
        &id,
        Action::Switch {
            reference: "feature".into(),
            detach: false,
        },
    );
    run(
        &mut w,
        &id,
        Action::Switch {
            reference: "main".into(),
            detach: false,
        },
    );
    let state = w.open(&id, &CANCEL).unwrap();
    let reference = state
        .references
        .iter()
        .find(|r| r.full_name == "refs/heads/feature")
        .unwrap();
    let prefs = w
        .favorite_reference(&id, &reference.id, true, &base, &CANCEL)
        .unwrap();
    assert!(prefs.recent.contains(&"refs/heads/feature".into()));
    let blocked = root.join("occupied");
    fs::write(&blocked, "occupied").unwrap();
    assert!(
        w.favorite_reference(&id, &reference.id, false, &blocked, &CANCEL)
            .is_err()
    );
    assert_eq!(w.catalog.reference_settings[&id], prefs);
    run(
        &mut w,
        &id,
        Action::RenameBranch {
            name: "feature".into(),
            new_name: "renamed".into(),
        },
    );
    w.save(&base).unwrap();
    let mut loaded = Workspace::load(&base).unwrap();
    let state = loaded.open(&id, &CANCEL).unwrap();
    assert_eq!(
        state.reference_settings.favorites,
        vec!["refs/heads/renamed"]
    );
    assert!(
        state
            .reference_settings
            .recent
            .contains(&"refs/heads/renamed".into())
    );
    assert!(
        !state
            .reference_settings
            .recent
            .contains(&"refs/heads/feature".into())
    );
    assert!(
        loaded
            .favorite_reference(&id, "invented", true, &base, &CANCEL)
            .is_err()
    );
}
#[test]
fn switch_and_update_fetches_only_the_reviewed_upstream_and_failure_keeps_checkout() {
    let (d, root, origin, mut w, id) = fixture();
    let writer = d.path().join("writer");
    git(
        d.path(),
        &[
            "clone",
            "-b",
            "feature",
            origin.to_str().unwrap(),
            writer.to_str().unwrap(),
        ],
    );
    user(&writer);
    fs::write(writer.join("file.txt"), "remote new\n").unwrap();
    git(&writer, &["commit", "-am", "remote advance"]);
    git(&writer, &["push"]);
    let expected = git(&writer, &["rev-parse", "HEAD"]);
    let result = run(
        &mut w,
        &id,
        Action::SwitchAndUpdate {
            reference: "refs/heads/feature".into(),
            name: None,
            mode: "ff_only".into(),
        },
    );
    assert_eq!(result.state.repository.branch, "feature");
    assert_eq!(git(&root, &["rev-parse", "HEAD"]), expected);
    assert_eq!(
        fs::read_to_string(root.join("file.txt")).unwrap(),
        "remote new\n"
    );
    run(
        &mut w,
        &id,
        Action::Switch {
            reference: "main".into(),
            detach: false,
        },
    );
    git(
        &root,
        &[
            "remote",
            "set-url",
            "origin",
            d.path().join("missing.git").to_str().unwrap(),
        ],
    );
    let state = w.open(&id, &CANCEL).unwrap();
    let plan = w
        .prepare(
            &id,
            &state.revision,
            Action::SwitchAndUpdate {
                reference: "refs/heads/feature".into(),
                name: None,
                mode: "ff_only".into(),
            },
            &CANCEL,
        )
        .unwrap();
    let error = w.apply(&plan.id, &CANCEL).err().unwrap().to_string();
    assert!(error.contains("Checkout completed"));
    assert_eq!(git(&root, &["branch", "--show-current"]), "feature");
    assert!(w.open(&id, &CANCEL).unwrap().operation.is_none());
}
#[test]
fn checkout_and_rebase_uses_original_head_and_keeps_the_target_recovery_object() {
    let (_d, root, _origin, mut w, id) = fixture();
    fs::write(root.join("main.txt"), "main").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "main advance"]);
    let main = git(&root, &["rev-parse", "HEAD"]);
    git(&root, &["switch", "feature"]);
    fs::write(root.join("feature.txt"), "feature").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "feature advance"]);
    let old = git(&root, &["rev-parse", "HEAD"]);
    git(&root, &["switch", "main"]);
    let state = w.open(&id, &CANCEL).unwrap();
    let plan = w
        .prepare(
            &id,
            &state.revision,
            Action::SwitchAndRebase {
                reference: "refs/heads/feature".into(),
                name: None,
            },
            &CANCEL,
        )
        .unwrap();
    assert!(plan.dangerous);
    let result = w.apply(&plan.id, &CANCEL).unwrap();
    assert_eq!(result.state.repository.branch, "feature");
    assert_eq!(git(&root, &["rev-parse", "main"]), main);
    git(&root, &["merge-base", "--is-ancestor", &main, "HEAD"]);
    assert_eq!(
        git(
            &root,
            &["rev-parse", result.backup_reference.as_ref().unwrap()]
        ),
        old
    );
}
#[test]
fn push_to_explicit_remote_branch_sets_upstream_and_exact_lease_rejects_stale_remote() {
    let (d, root, origin, mut w, id) = fixture();
    run(
        &mut w,
        &id,
        Action::PushBranch {
            branch: "main".into(),
            remote: "origin".into(),
            target: "custom-target".into(),
            set_upstream: true,
            force_with_lease: false,
        },
    );
    assert_eq!(
        git(&root, &["config", "branch.main.merge"]),
        "refs/heads/custom-target"
    );
    let writer = d.path().join("writer");
    git(
        d.path(),
        &[
            "clone",
            "-b",
            "custom-target",
            origin.to_str().unwrap(),
            writer.to_str().unwrap(),
        ],
    );
    user(&writer);
    fs::write(writer.join("file.txt"), "remote change\n").unwrap();
    git(&writer, &["commit", "-am", "advance remote"]);
    git(&writer, &["push"]);
    let remote_head = git(&writer, &["rev-parse", "HEAD"]);
    let state = w.open(&id, &CANCEL).unwrap();
    let plan = w
        .prepare(
            &id,
            &state.revision,
            Action::PushBranch {
                branch: "main".into(),
                remote: "origin".into(),
                target: "custom-target".into(),
                set_upstream: false,
                force_with_lease: true,
            },
            &CANCEL,
        )
        .unwrap();
    assert!(plan.dangerous);
    assert!(
        plan.arguments
            .iter()
            .any(|a| a.starts_with("--force-with-lease=refs/heads/custom-target:"))
    );
    assert!(w.apply(&plan.id, &CANCEL).is_err());
    assert_eq!(
        git(&origin, &["rev-parse", "refs/heads/custom-target"]),
        remote_head
    );
    git(&root, &["fetch", "origin"]);
    let result = run(
        &mut w,
        &id,
        Action::PushBranch {
            branch: "main".into(),
            remote: "origin".into(),
            target: "custom-target".into(),
            set_upstream: false,
            force_with_lease: true,
        },
    );
    assert_eq!(
        git(&origin, &["rev-parse", "refs/heads/custom-target"]),
        git(&root, &["rev-parse", "main"])
    );
    assert_eq!(
        git(&root, &["rev-parse", &result.backup_reference.unwrap()]),
        remote_head
    );
    let state = w.open(&id, &CANCEL).unwrap();
    assert!(
        w.prepare(
            &id,
            &state.revision,
            Action::PushBranch {
                branch: "main".into(),
                remote: "origin".into(),
                target: "@{-1}".into(),
                set_upstream: false,
                force_with_lease: false
            },
            &CANCEL
        )
        .is_err()
    );
}

#[test]
fn update_rejects_ambiguous_config_and_prepared_checkout_rejects_changed_refs() {
    let (_d, root, _origin, mut w, id) = fixture();
    git(
        &root,
        &["config", "--add", "branch.feature.merge", "refs/heads/main"],
    );
    let state = w.open(&id, &CANCEL).unwrap();
    let error = w
        .prepare(
            &id,
            &state.revision,
            Action::SwitchAndUpdate {
                reference: "refs/heads/feature".into(),
                name: None,
                mode: "ff_only".into(),
            },
            &CANCEL,
        )
        .err()
        .unwrap()
        .to_string();
    assert!(error.contains("exactly one branch"));
    assert_eq!(git(&root, &["branch", "--show-current"]), "main");
    git(&root, &["config", "--unset-all", "branch.feature.merge"]);
    git(
        &root,
        &["config", "branch.feature.merge", "refs/heads/feature"],
    );
    let state = w.open(&id, &CANCEL).unwrap();
    let plan = w
        .prepare(
            &id,
            &state.revision,
            Action::SwitchAndUpdate {
                reference: "refs/heads/feature".into(),
                name: None,
                mode: "ff_only".into(),
            },
            &CANCEL,
        )
        .unwrap();
    fs::write(root.join("main.txt"), "advance main\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "external main advance"]);
    git(&root, &["branch", "-f", "feature", "main"]);
    assert!(w.apply(&plan.id, &CANCEL).is_err());
    assert_eq!(git(&root, &["branch", "--show-current"]), "main");
}

#[cfg(unix)]
#[test]
fn post_checkout_hook_change_stops_the_followup_instead_of_rebasing_a_different_head() {
    use std::os::unix::fs::PermissionsExt;
    let (_d, root, _origin, mut w, id) = fixture();
    fs::write(root.join("main.txt"), "main\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "main advance"]);
    let hook = root.join(".git/hooks/post-checkout");
    fs::write(&hook, "#!/bin/sh\ngit reset --hard main\n").unwrap();
    fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();
    let state = w.open(&id, &CANCEL).unwrap();
    let plan = w
        .prepare(
            &id,
            &state.revision,
            Action::SwitchAndRebase {
                reference: "refs/heads/feature".into(),
                name: None,
            },
            &CANCEL,
        )
        .unwrap();
    let error = w.apply(&plan.id, &CANCEL).err().unwrap().to_string();
    assert!(error.contains("follow-up was not run"));
    assert_eq!(git(&root, &["branch", "--show-current"]), "feature");
    assert_eq!(
        git(&root, &["rev-parse", "feature"]),
        git(&root, &["rev-parse", "main"])
    );
    assert!(w.open(&id, &CANCEL).unwrap().operation.is_none());
    let recovery = plan.backup_reference.unwrap();
    assert_ne!(
        git(&root, &["rev-parse", &recovery]),
        git(&root, &["rev-parse", "HEAD"])
    );
}
