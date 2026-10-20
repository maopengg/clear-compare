//! A standalone harness also serves as the managed editor child executable.
use clear_contracts::workbench::{Action, RebaseStepKind, RebaseTodoItem, ScanSettings};
use clear_core::workbench::Workspace;
use std::{fs, path::Path, process::Command, sync::atomic::AtomicBool};

fn git(root: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(root)
        .args([
            "-c",
            "user.name=Rebase Test",
            "-c",
            "user.email=rebase@example.com",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.hooksPath=/dev/null",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

fn fixture(conflicting: bool) -> (tempfile::TempDir, Workspace, String, String) {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "user.name", "Rebase Test"]);
    git(root, &["config", "user.email", "rebase@example.com"]);
    git(root, &["config", "commit.gpgsign", "false"]);
    git(
        root,
        &[
            "config",
            "core.hooksPath",
            root.join(".git/no-hooks").to_str().unwrap(),
        ],
    );
    fs::write(root.join("value.txt"), "base\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "base"]);
    let base = git(root, &["rev-parse", "HEAD"]);
    for (index, message) in ["one", "two", "three"].into_iter().enumerate() {
        let path = if conflicting {
            root.join("value.txt")
        } else {
            root.join(format!("{index}.txt"))
        };
        fs::write(path, format!("{message}\n")).unwrap();
        git(root, &["add", "."]);
        git(root, &["commit", "-m", message]);
    }
    let mut workspace = Workspace::default();
    let catalog = workspace
        .scan(
            ScanSettings {
                roots: vec![root.to_str().unwrap().into()],
                exclusions: vec![],
            },
            &AtomicBool::new(false),
        )
        .unwrap();
    let id = catalog.repositories[0].id.clone();
    (directory, workspace, id, base)
}

fn todo(workspace: &mut Workspace, id: &str, base: &str) -> Vec<RebaseTodoItem> {
    let state = workspace.open(id, &AtomicBool::new(false)).unwrap();
    workspace
        .interactive_rebase_todo(id, &state.revision, base, &AtomicBool::new(false))
        .unwrap()
}

fn apply(
    workspace: &mut Workspace,
    id: &str,
    action: Action,
) -> clear_core::Result<clear_contracts::workbench::ActionResult> {
    let cancel = AtomicBool::new(false);
    let state = workspace.open(id, &cancel)?;
    let plan = workspace.prepare(id, &state.revision, action, &cancel)?;
    workspace.apply(&plan.id, &cancel)
}

fn reorder_reword_drop() {
    let (directory, mut workspace, id, base) = fixture(false);
    let before = git(directory.path(), &["rev-parse", "HEAD"]);
    let mut items = todo(&mut workspace, &id, &base);
    assert_eq!(
        items
            .iter()
            .map(|item| item.subject.as_str())
            .collect::<Vec<_>>(),
        ["one", "two", "three"]
    );
    items.rotate_right(1);
    items[0].action = RebaseStepKind::Reword;
    items[0].message = "中文 'quoted' $(not-a-command)\n\nRetained body".into();
    items[2].action = RebaseStepKind::Drop;
    let result = apply(
        &mut workspace,
        &id,
        Action::InteractiveRebase {
            upstream: base,
            todo: items,
        },
    )
    .unwrap();
    assert!(result.state.operation.is_none());
    let root = directory.path();
    assert_eq!(git(root, &["log", "-1", "--format=%s"]), "one");
    assert_eq!(
        git(root, &["log", "-1", "--format=%B", "HEAD~1"]),
        "中文 'quoted' $(not-a-command)\n\nRetained body"
    );
    assert!(!root.join("1.txt").exists());
    assert_eq!(
        git(
            root,
            &["rev-parse", result.backup_reference.as_deref().unwrap()]
        ),
        before
    );
}

fn squash_and_fixup() {
    let (directory, mut workspace, id, base) = fixture(false);
    let mut items = todo(&mut workspace, &id, &base);
    items[1].action = RebaseStepKind::Fixup;
    items[2].action = RebaseStepKind::Squash;
    let result = apply(
        &mut workspace,
        &id,
        Action::InteractiveRebase {
            upstream: base.clone(),
            todo: items,
        },
    )
    .unwrap();
    assert!(result.state.operation.is_none());
    assert_eq!(
        git(
            directory.path(),
            &["rev-list", "--count", &format!("{base}..HEAD")]
        ),
        "1"
    );
    let message = git(directory.path(), &["log", "-1", "--format=%B"]);
    assert!(
        message.contains("one") && message.contains("three") && !message.contains("two"),
        "{message}"
    );
}

fn pause_and_resume_reword() {
    let (directory, mut workspace, id, base) = fixture(false);
    let mut items = todo(&mut workspace, &id, &base);
    items[0].action = RebaseStepKind::Edit;
    items[1].action = RebaseStepKind::Reword;
    items[1].message = "after edit stop".into();
    let stopped = apply(
        &mut workspace,
        &id,
        Action::InteractiveRebase {
            upstream: base,
            todo: items,
        },
    )
    .unwrap();
    assert_eq!(stopped.state.operation.as_deref(), Some("rebase"));
    assert!(
        directory
            .path()
            .join(".git/rebase-merge/clear-compare-editor.json")
            .is_file()
    );
    let settings = tempfile::tempdir().unwrap();
    workspace.save(settings.path()).unwrap();
    let mut workspace = Workspace::load(settings.path()).unwrap();
    let result = apply(
        &mut workspace,
        &id,
        Action::Continue {
            operation: "rebase".into(),
        },
    )
    .unwrap();
    assert!(result.state.operation.is_none());
    assert_eq!(
        git(directory.path(), &["log", "-1", "--format=%s", "HEAD~1"]),
        "after edit stop"
    );
}

fn conflict_and_resume() {
    let (directory, mut workspace, id, base) = fixture(true);
    let mut items = todo(&mut workspace, &id, &base);
    items.swap(0, 1);
    items[1].action = RebaseStepKind::Reword;
    items[1].message = "resolved reword".into();
    let error = apply(
        &mut workspace,
        &id,
        Action::InteractiveRebase {
            upstream: base,
            todo: items,
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("Recovery reference"));
    for attempt in 0..4 {
        let state = workspace.open(&id, &AtomicBool::new(false)).unwrap();
        if state.operation.is_none() {
            break;
        }
        if !state.changes.is_empty() {
            fs::write(
                directory.path().join("value.txt"),
                format!("resolved {attempt}\n"),
            )
            .unwrap();
            git(directory.path(), &["add", "value.txt"]);
        }
        let _ = apply(
            &mut workspace,
            &id,
            Action::Continue {
                operation: "rebase".into(),
            },
        );
    }
    assert!(
        workspace
            .open(&id, &AtomicBool::new(false))
            .unwrap()
            .operation
            .is_none()
    );
    assert!(git(directory.path(), &["log", "--format=%s"]).contains("resolved reword"));
}

fn reject_invalid_or_changed_todo() {
    let (directory, mut workspace, id, base) = fixture(false);
    let mut items = todo(&mut workspace, &id, &base);
    items[0].action = RebaseStepKind::Squash;
    assert!(
        apply(
            &mut workspace,
            &id,
            Action::InteractiveRebase {
                upstream: base.clone(),
                todo: items
            }
        )
        .is_err()
    );
    let mut items = todo(&mut workspace, &id, &base);
    items[0].action = RebaseStepKind::Edit;
    apply(
        &mut workspace,
        &id,
        Action::InteractiveRebase {
            upstream: base,
            todo: items,
        },
    )
    .unwrap();
    let path = directory.path().join(".git/rebase-merge/git-rebase-todo");
    let text = format!(
        "{}\nexec echo SHOULD_NOT_EXECUTE\n",
        fs::read_to_string(&path).unwrap()
    );
    fs::write(&path, text).unwrap();
    let error = apply(
        &mut workspace,
        &id,
        Action::Continue {
            operation: "rebase".into(),
        },
    )
    .unwrap_err();
    assert!(error.to_string().contains("changed externally"));
    apply(
        &mut workspace,
        &id,
        Action::Abort {
            operation: "rebase".into(),
        },
    )
    .unwrap();
}

fn linked_worktree_pause_and_resume() {
    let (directory, mut workspace, _, base) = fixture(false);
    let before = git(directory.path(), &["rev-parse", "HEAD"]);
    let linked = directory.path().join("linked worktree");
    git(
        directory.path(),
        &["worktree", "add", "-b", "linked", linked.to_str().unwrap()],
    );
    let catalog = workspace
        .scan(
            ScanSettings {
                roots: vec![directory.path().to_str().unwrap().into()],
                exclusions: vec![],
            },
            &AtomicBool::new(false),
        )
        .unwrap();
    let id = catalog
        .repositories
        .iter()
        .find(|entry| Path::new(&entry.path) == linked.canonicalize().unwrap())
        .unwrap()
        .id
        .clone();
    let mut items = todo(&mut workspace, &id, &base);
    items[0].action = RebaseStepKind::Edit;
    items[1].action = RebaseStepKind::Reword;
    items[1].message = "linked worktree reword".into();
    apply(
        &mut workspace,
        &id,
        Action::InteractiveRebase {
            upstream: base,
            todo: items,
        },
    )
    .unwrap();
    apply(
        &mut workspace,
        &id,
        Action::Continue {
            operation: "rebase".into(),
        },
    )
    .unwrap();
    assert_eq!(
        git(&linked, &["log", "-1", "--format=%s", "HEAD~1"]),
        "linked worktree reword"
    );
    assert_eq!(git(directory.path(), &["rev-parse", "HEAD"]), before);
}

fn skip_preserves_later_reword() {
    let (directory, mut workspace, id, base) = fixture(true);
    let mut items = todo(&mut workspace, &id, &base);
    items[2].action = RebaseStepKind::Drop;
    items.swap(0, 1);
    items[1].action = RebaseStepKind::Reword;
    items[1].message = "after skipped conflict".into();
    assert!(
        apply(
            &mut workspace,
            &id,
            Action::InteractiveRebase {
                upstream: base,
                todo: items
            }
        )
        .is_err()
    );
    let result = apply(
        &mut workspace,
        &id,
        Action::Skip {
            operation: "rebase".into(),
        },
    )
    .unwrap();
    assert!(result.state.operation.is_none());
    assert_eq!(
        git(directory.path(), &["log", "-1", "--format=%s"]),
        "after skipped conflict"
    );
}

fn main() {
    if let Some(code) = clear_core::workbench::rebase_editor::run_from_args() {
        std::process::exit(code);
    }
    for (name, test) in [
        (
            "reorder, reword, drop and recovery",
            reorder_reword_drop as fn(),
        ),
        ("squash and fixup", squash_and_fixup),
        (
            "pause and persisted reword continuation",
            pause_and_resume_reword,
        ),
        ("conflict continuation", conflict_and_resume),
        (
            "reject invalid and externally changed sequences",
            reject_invalid_or_changed_todo,
        ),
        (
            "linked worktree continuation",
            linked_worktree_pause_and_resume,
        ),
        ("skip preserves later reword", skip_preserves_later_reword),
    ] {
        test();
        println!("passed: {name}");
    }
    if !std::env::args().any(|arg| arg == "--quoted-path-child") {
        let directory = tempfile::tempdir().unwrap();
        let folder = directory.path().join("editor '中文' path");
        fs::create_dir(&folder).unwrap();
        let executable = folder.join(if cfg!(windows) {
            "rebase suite.exe"
        } else {
            "rebase suite"
        });
        fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
        assert!(
            Command::new(executable)
                .arg("--quoted-path-child")
                .status()
                .unwrap()
                .success()
        );
        println!("passed: executable path with spaces, apostrophes and Unicode");
    }
}
