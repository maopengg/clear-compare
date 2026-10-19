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
    git(root, &["config", "core.hooksPath", ""]);
}

fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf, Workspace, String) {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("local");
    let remote = d.path().join("remote.git");
    fs::create_dir(&root).unwrap();
    git(&root, &["init", "-b", "main"]);
    user(&root);
    fs::write(root.join("file.txt"), "base\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "base"]);
    git(
        d.path(),
        &["init", "--bare", "-b", "main", remote.to_str().unwrap()],
    );
    git(
        &root,
        &["remote", "add", "origin", remote.to_str().unwrap()],
    );
    git(&root, &["push", "-u", "origin", "main"]);
    let mut w = Workspace::default();
    let c = w
        .scan(
            ScanSettings {
                roots: vec![root.to_str().unwrap().into()],
                exclusions: vec![],
            },
            &CANCEL,
        )
        .unwrap();
    let id = c.repositories[0].id.clone();
    (d, root, remote, w, id)
}
fn run(w: &mut Workspace, id: &str, action: Action) -> ActionResult {
    let state = w.open(id, &CANCEL).unwrap();
    let plan = w.prepare(id, &state.revision, action, &CANCEL).unwrap();
    assert!(plan.dangerous);
    w.apply(&plan.id, &CANCEL).unwrap()
}
#[test]
fn remote_edits_update_matching_preserve_other_urls_and_can_clear_local_push() {
    let (d, root, remote, mut w, id) = fixture();
    let second = d.path().join("other remote.git");
    git(d.path(), &["init", "--bare", second.to_str().unwrap()]);
    let before = fs::read(root.join(".git/config")).unwrap();
    let result = run(
        &mut w,
        &id,
        Action::SetRemote {
            name: "origin".into(),
            url: second.to_str().unwrap().into(),
            push: false,
        },
    );
    assert_eq!(
        result.state.repository.match_key,
        clear_core::workbench::remote_key(second.to_str().unwrap())
    );
    let recovery = PathBuf::from(result.recovery_path.unwrap());
    assert_eq!(fs::read(recovery.join("config")).unwrap(), before);
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(recovery.join("before.json")).unwrap()).unwrap();
    assert_eq!(
        git(
            &root,
            &[
                "rev-parse",
                manifest["references"][0]["recovery"].as_str().unwrap()
            ]
        ),
        git(&root, &["rev-parse", "main"])
    );
    let result = run(
        &mut w,
        &id,
        Action::SetRemote {
            name: "origin".into(),
            url: remote.to_str().unwrap().into(),
            push: true,
        },
    );
    assert!(result.state.remotes[0].push_local);
    assert_eq!(result.state.remotes[0].push, remote.to_str().unwrap());
    let result = run(
        &mut w,
        &id,
        Action::ClearRemotePush {
            name: "origin".into(),
        },
    );
    assert!(!result.state.remotes[0].push_local);
    assert_eq!(result.state.remotes[0].push, result.state.remotes[0].fetch);
    git(
        &root,
        &[
            "remote",
            "set-url",
            "--add",
            "origin",
            remote.to_str().unwrap(),
        ],
    );
    let result = run(
        &mut w,
        &id,
        Action::SetRemote {
            name: "origin".into(),
            url: "https://example.com/next.git".into(),
            push: false,
        },
    );
    assert_eq!(
        result.state.remotes[0].fetch_urls,
        vec!["https://example.com/next.git", remote.to_str().unwrap()]
    );
    assert!(git(&root, &["status", "--porcelain"]).is_empty());
}
#[test]
fn remove_preserves_config_and_tracking_objects_without_deleting_local_branches() {
    let (_d, root, _remote, mut w, id) = fixture();
    let before = fs::read(root.join(".git/config")).unwrap();
    let head = git(&root, &["rev-parse", "HEAD"]);
    let result = run(
        &mut w,
        &id,
        Action::RemoveRemote {
            name: "origin".into(),
        },
    );
    assert!(result.state.remotes.is_empty());
    assert!(result.state.repository.match_key.is_none());
    assert_eq!(git(&root, &["rev-parse", "main"]), head);
    assert!(git(&root, &["for-each-ref", "refs/remotes/origin"]).is_empty());
    let path = PathBuf::from(result.recovery_path.unwrap());
    assert_eq!(fs::read(path.join("config")).unwrap(), before);
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(path.join("before.json")).unwrap()).unwrap();
    assert_eq!(
        git(
            &root,
            &[
                "rev-parse",
                value["references"][0]["recovery"].as_str().unwrap()
            ]
        ),
        head
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(path.join("result.json")).unwrap())
            .unwrap()["success"],
        true
    );
    let (_d, root, _remote, mut w, id) = fixture();
    git(
        &root,
        &[
            "config",
            "remote.origin.fetch",
            "+refs/heads/*:refs/remotes/shared/*",
        ],
    );
    let tree = git(&root, &["rev-parse", "HEAD^{tree}"]);
    let custom = git(
        &root,
        &[
            "commit-tree",
            &tree,
            "-p",
            "HEAD",
            "-m",
            "custom remote object",
        ],
    );
    git(&root, &["update-ref", "refs/remotes/shared/main", &custom]);
    let result = run(
        &mut w,
        &id,
        Action::RemoveRemote {
            name: "origin".into(),
        },
    );
    assert!(git(&root, &["for-each-ref", "refs/remotes/shared"]).is_empty());
    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(PathBuf::from(result.recovery_path.unwrap()).join("before.json")).unwrap(),
    )
    .unwrap();
    let saved = manifest["references"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["name"] == "refs/remotes/shared/main")
        .unwrap();
    assert_eq!(
        git(&root, &["rev-parse", saved["recovery"].as_str().unwrap()]),
        custom
    );
}
#[test]
fn credentials_are_redacted_from_views_plans_and_matching_but_private_backup_is_exact() {
    let (_d, root, _remote, mut w, id) = fixture();
    let url = "https://user:credential-value@example.com:443/team/repo.git?token=secret-query#secret-fragment";
    let state = w.open(&id, &CANCEL).unwrap();
    let p = w
        .prepare(
            &id,
            &state.revision,
            Action::AddRemote {
                name: "web".into(),
                url: url.into(),
            },
            &CANCEL,
        )
        .unwrap();
    let display = format!("{:?}", p);
    assert!(
        !display.contains("credential-value")
            && !display.contains("secret-query")
            && !display.contains("secret-fragment")
    );
    let result = w.apply(&p.id, &CANCEL).unwrap();
    let display = format!("{:?}", result.state);
    assert!(!display.contains("credential-value") && !display.contains("secret-query"));
    let web = result
        .state
        .remotes
        .iter()
        .find(|r| r.name == "web")
        .unwrap();
    assert_eq!(web.match_key.as_deref(), Some("example.com/team/repo"));
    let before = fs::read(root.join(".git/config")).unwrap();
    let changed = run(
        &mut w,
        &id,
        Action::SetRemote {
            name: "web".into(),
            url: "git@example.com:team/new.git".into(),
            push: false,
        },
    );
    let recovery = PathBuf::from(changed.recovery_path.unwrap());
    assert_eq!(fs::read(recovery.join("config")).unwrap(), before);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(&recovery).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(recovery.join("config"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    assert_eq!(
        clear_core::workbench::remote_key("https://example.com/team/email@repo.git?auth=secret"),
        Some("example.com/team/email@repo".into())
    );
}
#[test]
fn config_lock_failure_retains_input_state_and_records_recovery() {
    let (_d, root, _remote, mut w, id) = fixture();
    let before = fs::read(root.join(".git/config")).unwrap();
    let state = w.open(&id, &CANCEL).unwrap();
    let p = w
        .prepare(
            &id,
            &state.revision,
            Action::SetRemote {
                name: "origin".into(),
                url: "https://example.com/repo.git".into(),
                push: false,
            },
            &CANCEL,
        )
        .unwrap();
    fs::write(root.join(".git/config.lock"), "other writer").unwrap();
    let error = w.apply(&p.id, &CANCEL).err().unwrap().to_string();
    assert!(error.contains("Configuration recovery:"));
    assert_eq!(fs::read(root.join(".git/config")).unwrap(), before);
    let path = root.join(".git/clear-compare-remote-backups").join(p.id);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&fs::read(path.join("result.json")).unwrap())
            .unwrap()["success"],
        false
    );
    assert_eq!(fs::read(path.join("config")).unwrap(), before);
    assert_eq!(
        fs::read_to_string(root.join(".git/config.lock")).unwrap(),
        "other writer"
    );
}
#[test]
fn stale_invalid_and_inherited_edits_do_not_change_configuration() {
    let (d, root, _remote, mut w, id) = fixture();
    let state = w.open(&id, &CANCEL).unwrap();
    let p = w
        .prepare(
            &id,
            &state.revision,
            Action::RemoveRemote {
                name: "origin".into(),
            },
            &CANCEL,
        )
        .unwrap();
    git(
        &root,
        &[
            "config",
            "remote.origin.url",
            "https://example.com/external.git",
        ],
    );
    let before = fs::read(root.join(".git/config")).unwrap();
    assert!(w.apply(&p.id, &CANCEL).is_err());
    assert_eq!(fs::read(root.join(".git/config")).unwrap(), before);
    let state = w.open(&id, &CANCEL).unwrap();
    for action in [
        Action::AddRemote {
            name: "bad..name".into(),
            url: "somewhere".into(),
        },
        Action::AddRemote {
            name: "origin".into(),
            url: "somewhere".into(),
        },
        Action::SetRemote {
            name: "origin".into(),
            url: "--invalid".into(),
            push: false,
        },
        Action::ClearRemotePush {
            name: "origin".into(),
        },
    ] {
        assert!(w.prepare(&id, &state.revision, action, &CANCEL).is_err());
    }
    let included = d.path().join("included.cfg");
    fs::write(
        &included,
        "[remote \"origin\"]\n\tpushurl = https://example.com/inherited.git\n",
    )
    .unwrap();
    git(
        &root,
        &["config", "include.path", included.to_str().unwrap()],
    );
    let state = w.open(&id, &CANCEL).unwrap();
    assert!(!state.remotes[0].push_local);
    assert!(
        w.prepare(
            &id,
            &state.revision,
            Action::SetRemote {
                name: "origin".into(),
                url: "new-url".into(),
                push: true
            },
            &CANCEL
        )
        .is_err()
    );
    assert_eq!(
        fs::read_to_string(&included).unwrap(),
        "[remote \"origin\"]\n\tpushurl = https://example.com/inherited.git\n"
    );
}
#[test]
fn probes_use_configured_fetch_and_push_endpoints_without_fetching_objects() {
    let (d, root, remote, mut w, id) = fixture();
    let writer = d.path().join("writer");
    git(
        d.path(),
        &["clone", remote.to_str().unwrap(), writer.to_str().unwrap()],
    );
    user(&writer);
    fs::write(writer.join("file.txt"), "new remote object\n").unwrap();
    git(&writer, &["commit", "-am", "advance remote"]);
    git(&writer, &["push"]);
    let new_object = git(&writer, &["rev-parse", "HEAD"]);
    let available = || {
        Command::new("git")
            .current_dir(&root)
            .args(["--no-lazy-fetch", "cat-file", "-e", &new_object])
            .output()
            .unwrap()
            .status
            .success()
    };
    assert!(!available());
    let config = fs::read(root.join(".git/config")).unwrap();
    let refs = git(
        &root,
        &["for-each-ref", "--format=%(refname):%(objectname)"],
    );
    let p = w.probe_remote(&id, "origin", false, &CANCEL).unwrap();
    assert!(p.references >= 1);
    assert!(!available());
    assert!(!root.join(".git/FETCH_HEAD").exists());
    assert_eq!(p.default_branch.as_deref(), Some("refs/heads/main"));
    assert_eq!(fs::read(root.join(".git/config")).unwrap(), config);
    assert_eq!(
        git(
            &root,
            &["for-each-ref", "--format=%(refname):%(objectname)"]
        ),
        refs
    );
    run(
        &mut w,
        &id,
        Action::SetRemote {
            name: "origin".into(),
            url: d.path().join("missing.git").to_str().unwrap().into(),
            push: true,
        },
    );
    assert!(w.probe_remote(&id, "origin", true, &CANCEL).is_err());
    assert!(w.probe_remote(&id, "origin", false, &CANCEL).is_ok());
    assert!(w.probe_remote(&id, "unknown", false, &CANCEL).is_err());
    assert!(
        w.probe_remote(&id, "origin", false, &AtomicBool::new(true))
            .is_err()
    );
}

#[cfg(unix)]
#[test]
fn symlinked_backup_paths_cannot_redirect_config_copies_outside_git_metadata() {
    use std::os::unix::fs::symlink;
    let (_d, root, _remote, mut w, id) = fixture();
    let outside = tempfile::tempdir().unwrap();
    symlink(
        outside.path(),
        root.join(".git/clear-compare-remote-backups"),
    )
    .unwrap();
    let before = fs::read(root.join(".git/config")).unwrap();
    let state = w.open(&id, &CANCEL).unwrap();
    let p = w
        .prepare(
            &id,
            &state.revision,
            Action::SetRemote {
                name: "origin".into(),
                url: "https://example.com/new.git".into(),
                push: false,
            },
            &CANCEL,
        )
        .unwrap();
    assert!(w.apply(&p.id, &CANCEL).is_err());
    assert_eq!(fs::read(root.join(".git/config")).unwrap(), before);
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
}
