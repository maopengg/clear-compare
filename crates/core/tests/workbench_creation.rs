use clear_contracts::workbench::*;
use clear_core::workbench::Workspace;
use std::{fs, path::Path, process::Command, sync::atomic::AtomicBool};
const RUN: AtomicBool = AtomicBool::new(false);
fn git(root: &Path, args: &[&str]) -> String {
    let o = Command::new("git")
        .current_dir(root)
        .args([
            "-c",
            "user.name=Creation Test",
            "-c",
            "user.email=creation@example.com",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().into()
}
fn source(root: &Path) {
    fs::create_dir_all(root).unwrap();
    git(root, &["init", "-b", "main"]);
    fs::write(root.join("file.txt"), "first\n").unwrap();
    git(root, &["add", "file.txt"]);
    git(root, &["commit", "-m", "first"]);
    fs::write(root.join("file.txt"), "second\n").unwrap();
    git(root, &["commit", "-am", "second"]);
}
fn init(bare: bool) -> RepositoryCreation {
    RepositoryCreation::Init {
        branch: "feature/new".into(),
        bare,
    }
}
fn clone_from(root: &Path) -> RepositoryCreation {
    RepositoryCreation::Clone {
        url: root.to_str().unwrap().into(),
        branch: None,
        bare: false,
        depth: None,
    }
}
fn prepare(
    w: &mut Workspace,
    parent: &Path,
    name: &str,
    op: RepositoryCreation,
) -> RepositoryCreationPlan {
    w.creation_prepare(parent.to_str().unwrap(), name, op, &RUN)
        .unwrap()
}
#[test]
fn initialization_publishes_regular_and_bare_repositories_and_registers_them() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().join("settings");
    let mut w = Workspace::default();
    for bare in [false, true] {
        let name = if bare { "bare.git" } else { "新 仓库" };
        let plan = prepare(&mut w, temp.path(), name, init(bare));
        assert!(!Path::new(&plan.destination).exists());
        assert!(!base.exists() || w.catalog.repositories.len() == 1);
        let r = w.creation_apply(&plan.id, &base, &RUN).unwrap();
        assert_eq!(r.status, "registered", "{:?}", r.issue);
        assert!(r.issue.is_none());
        assert_eq!(
            git(Path::new(&r.path), &["symbolic-ref", "--short", "HEAD"]),
            "feature/new"
        );
        assert_eq!(
            git(Path::new(&r.path), &["rev-parse", "--is-bare-repository"]),
            bare.to_string()
        );
        assert!(Path::new(&r.journal).join("intent.json").is_file());
        assert!(Path::new(&r.journal).join("result.json").is_file());
        assert!(w.creation_apply(&plan.id, &base, &RUN).is_err());
    }
    let mut loaded = Workspace::load(&base).unwrap();
    assert_eq!(
        loaded
            .scan(ScanSettings::default(), &RUN)
            .unwrap()
            .repositories
            .len(),
        2
    );
}
#[test]
fn local_clone_is_independent_supports_branch_tag_shallow_and_bare() {
    let temp = tempfile::tempdir().unwrap();
    let src = temp.path().join("source");
    source(&src);
    git(&src, &["tag", "v1"]);
    let original = git(&src, &["rev-parse", "HEAD"]);
    let mut w = Workspace::default();
    let base = temp.path().join("settings");
    for (name, branch, bare, depth) in [
        ("copy", None, false, None),
        ("shallow", Some("main".into()), false, Some(1)),
        ("tag", Some("v1".into()), false, None),
        ("bare-copy", None, true, None),
    ] {
        let p = prepare(
            &mut w,
            temp.path(),
            name,
            RepositoryCreation::Clone {
                url: src.to_str().unwrap().into(),
                branch,
                bare,
                depth,
            },
        );
        let r = w.creation_apply(&p.id, &base, &RUN).unwrap();
        assert_eq!(r.status, "registered", "{:?}", r.issue);
        assert_eq!(git(Path::new(&r.path), &["rev-parse", "HEAD"]), original);
        if !bare {
            assert_eq!(
                fs::read_to_string(Path::new(&r.path).join("file.txt")).unwrap(),
                "second\n"
            );
        }
        if depth.is_some() {
            assert_eq!(
                git(Path::new(&r.path), &["rev-list", "--count", "HEAD"]),
                "1"
            );
        }
    }
    assert_eq!(git(&src, &["rev-parse", "HEAD"]), original);
    assert_eq!(git(&src, &["status", "--porcelain"]), "");
    fs::remove_dir_all(src).unwrap();
    assert_eq!(
        git(&temp.path().join("copy"), &["show", "HEAD:file.txt"]),
        "second"
    );
}
#[test]
fn failed_clone_preserves_staging_and_does_not_publish_or_register() {
    let temp = tempfile::tempdir().unwrap();
    let src = temp.path().join("source");
    source(&src);
    let mut w = Workspace::default();
    let p = prepare(
        &mut w,
        temp.path(),
        "failed",
        RepositoryCreation::Clone {
            url: src.to_str().unwrap().into(),
            branch: Some("missing".into()),
            bare: false,
            depth: None,
        },
    );
    let r = w
        .creation_apply(&p.id, &temp.path().join("settings"), &RUN)
        .unwrap();
    assert_eq!(r.status, "failed");
    assert!(Path::new(&r.path).is_dir());
    assert!(!Path::new(&p.destination).exists());
    assert!(r.issue.unwrap().contains("missing"));
    assert!(w.catalog.repositories.is_empty());
    assert!(Path::new(&r.journal).join("result.json").is_file());
}
#[test]
fn completed_repository_survives_catalogue_save_failure_and_can_be_registered_later() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().join("settings");
    fs::create_dir_all(base.join("git-workspace.json")).unwrap();
    let mut w = Workspace::default();
    let p = prepare(&mut w, temp.path(), "created", init(false));
    let r = w.creation_apply(&p.id, &base, &RUN).unwrap();
    assert_eq!(r.status, "created_unregistered");
    assert_eq!(r.path, p.destination);
    assert!(r.issue.is_some());
    assert!(Path::new(&r.path).join(".git").is_dir());
    assert!(w.catalog.repositories.is_empty());
    fs::remove_dir(base.join("git-workspace.json")).unwrap();
    let reg = w.registration_prepare(&r.path, None, &RUN).unwrap();
    w.registration_apply(&reg.id, &base, &RUN).unwrap();
    assert_eq!(w.catalog.repositories.len(), 1);
}
#[test]
fn invalid_sources_names_depth_and_existing_destinations_are_rejected() {
    let temp = tempfile::tempdir().unwrap();
    let mut w = Workspace::default();
    for name in [
        "",
        ".git",
        "../outside",
        "a/b",
        "a\\b",
        "CON",
        "nul.txt",
        "bad.",
        "a:",
        "-x",
    ] {
        assert!(
            w.creation_prepare(temp.path().to_str().unwrap(), name, init(false), &RUN)
                .is_err(),
            "{name}"
        );
    }
    for url in [
        "ext::payload",
        "file:///tmp/test",
        "https://token@example.com/team/repo",
        "https://example.com/repo?token=secret",
        "relative/path",
        "--upload-pack=bad",
        "https://example.com",
        "ssh://user:secret@example.com/repo",
    ] {
        let op = RepositoryCreation::Clone {
            url: url.into(),
            branch: None,
            bare: false,
            depth: None,
        };
        assert!(
            w.creation_prepare(temp.path().to_str().unwrap(), "test", op, &RUN)
                .is_err(),
            "{url}"
        );
    }
    fs::create_dir(temp.path().join("existing")).unwrap();
    fs::write(temp.path().join("existing/keep"), "untouched").unwrap();
    assert!(
        w.creation_prepare(temp.path().to_str().unwrap(), "existing", init(false), &RUN)
            .is_err()
    );
    assert_eq!(
        fs::read_to_string(temp.path().join("existing/keep")).unwrap(),
        "untouched"
    );
    let op = RepositoryCreation::Clone {
        url: "https://example.com/repo.git".into(),
        branch: None,
        bare: false,
        depth: Some(0),
    };
    assert!(
        w.creation_prepare(temp.path().to_str().unwrap(), "test", op, &RUN)
            .is_err()
    );
    assert!(
        w.creation_prepare(
            temp.path().to_str().unwrap(),
            "test",
            RepositoryCreation::Init {
                branch: "bad..branch".into(),
                bare: false
            },
            &RUN
        )
        .is_err()
    );
}
#[test]
fn parent_source_and_destination_identity_are_rechecked_before_writes() {
    let temp = tempfile::tempdir().unwrap();
    let parent = temp.path().join("parent");
    fs::create_dir(&parent).unwrap();
    let mut w = Workspace::default();
    let p = prepare(&mut w, &parent, "new", init(false));
    fs::rename(&parent, temp.path().join("original")).unwrap();
    fs::create_dir(&parent).unwrap();
    assert!(
        w.creation_apply(&p.id, &temp.path().join("settings"), &RUN)
            .is_err()
    );
    assert!(!parent.join("new").exists());
    let p = prepare(&mut w, &parent, "new", init(false));
    fs::create_dir(parent.join("new")).unwrap();
    fs::write(parent.join("new/keep"), "keep").unwrap();
    assert!(
        w.creation_apply(&p.id, &temp.path().join("settings"), &RUN)
            .is_err()
    );
    assert_eq!(fs::read_to_string(parent.join("new/keep")).unwrap(), "keep");
    let src = temp.path().join("source");
    source(&src);
    let p = prepare(&mut w, &parent, "clone", clone_from(&src));
    fs::rename(src.join(".git"), src.join("old-git")).unwrap();
    git(&src, &["init", "-b", "main"]);
    assert!(
        w.creation_apply(&p.id, &temp.path().join("settings"), &RUN)
            .is_err()
    );
    assert!(!parent.join("clone").exists());
}
#[test]
fn cancellation_before_start_and_wrong_plan_do_not_create_files() {
    let temp = tempfile::tempdir().unwrap();
    let base = temp.path().join("settings");
    let mut w = Workspace::default();
    let p = prepare(&mut w, temp.path(), "new", init(false));
    assert!(
        w.creation_apply(&p.id, &base, &AtomicBool::new(true))
            .is_err()
    );
    assert!(!base.exists());
    assert!(!Path::new(&p.destination).exists());
    let p = prepare(&mut w, temp.path(), "other", init(false));
    assert!(w.creation_apply("wrong", &base, &RUN).is_err());
    assert!(w.creation_apply(&p.id, &base, &RUN).is_err());
}
#[test]
fn metadata_parent_is_rejected_without_changing_existing_repository() {
    let temp = tempfile::tempdir().unwrap();
    let src = temp.path().join("source");
    source(&src);
    let mut w = Workspace::default();
    assert!(
        w.creation_prepare(
            src.join(".git").to_str().unwrap(),
            "nested",
            init(false),
            &RUN
        )
        .is_err()
    );
    let bare = temp.path().join("bare.git");
    fs::create_dir(&bare).unwrap();
    git(&bare, &["init", "--bare"]);
    assert!(
        w.creation_prepare(bare.to_str().unwrap(), "nested", init(false), &RUN)
            .is_err()
    );
    assert!(!src.join(".git/nested").exists());
    assert!(!bare.join("nested").exists());
}
#[test]
fn cancellation_of_a_live_clone_retains_owned_staging_and_a_terminal_journal() {
    use std::{
        io::Read,
        net::TcpListener,
        sync::atomic::Ordering,
        time::{Duration, Instant},
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let cancel = AtomicBool::new(false);
    std::thread::scope(|scope| {
        let flag = &cancel;
        let server = scope.spawn(move || {
            let start = Instant::now();
            loop {
                match listener.accept() {
                    Ok((mut socket, _)) => {
                        socket
                            .set_read_timeout(Some(Duration::from_secs(5)))
                            .unwrap();
                        let mut bytes = [0; 1024];
                        let _ = socket.read(&mut bytes);
                        flag.store(true, Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(150));
                        return true;
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        if start.elapsed() > Duration::from_secs(8) {
                            return false;
                        }
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(e) => panic!("{e}"),
                }
            }
        });
        let temp = tempfile::tempdir().unwrap();
        let mut w = Workspace::default();
        let p = prepare(
            &mut w,
            temp.path(),
            "cancelled",
            RepositoryCreation::Clone {
                url: format!("http://{address}/repo.git"),
                branch: None,
                bare: false,
                depth: None,
            },
        );
        let start = Instant::now();
        let r = w
            .creation_apply(&p.id, &temp.path().join("settings"), &cancel)
            .unwrap();
        assert!(
            server.join().unwrap(),
            "Git never reached the isolated local HTTP listener"
        );
        assert!(start.elapsed() < Duration::from_secs(10));
        assert_eq!(r.status, "cancelled", "{:?}", r.issue);
        assert!(Path::new(&r.path).is_dir());
        assert!(!Path::new(&p.destination).exists());
        assert!(Path::new(&r.journal).join("result.json").is_file());
        assert!(w.catalog.repositories.is_empty());
    });
}
