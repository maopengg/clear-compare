use clear_contracts::workbench::*;
use clear_core::workbench::Workspace;
use std::{fs, path::Path, process::Command, sync::atomic::AtomicBool};
const CANCEL: AtomicBool = AtomicBool::new(false);
fn fixture() -> (tempfile::TempDir, Workspace, String) {
    let dir = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .status()
            .unwrap()
            .success()
    );
    fs::write(dir.path().join("file.txt"), "original").unwrap();
    fs::create_dir(dir.path().join("target")).unwrap();
    let mut w = Workspace::default();
    let c = w
        .scan(
            ScanSettings {
                roots: vec![dir.path().display().to_string()],
                exclusions: vec![],
            },
            &CANCEL,
        )
        .unwrap();
    let id = c.repositories[0].id.clone();
    (dir, w, id)
}
fn file(w: &mut Workspace, repo: &str, name: &str) -> String {
    w.file_tree(repo, None, 0, &CANCEL)
        .unwrap()
        .files
        .into_iter()
        .find(|f| f.name == name)
        .unwrap()
        .id
}
fn no_trash(_: &Path) -> clear_core::Result<()> {
    panic!("non-trash action invoked recycle callback")
}
#[test]
fn rename_and_move_are_one_use_no_replace_and_backup_verified() {
    let (d, mut w, id) = fixture();
    let base = tempfile::tempdir().unwrap();
    let f = file(&mut w, &id, "file.txt");
    let plan = w
        .source_file_prepare(
            &id,
            &f,
            SourceFileAction::Rename {
                name: "renamed.txt".into(),
            },
            &CANCEL,
        )
        .unwrap();
    assert_eq!(plan.files, 1);
    let result = w
        .source_file_apply(&plan.id, base.path(), &CANCEL, no_trash)
        .unwrap();
    assert_eq!(
        fs::read_to_string(d.path().join("renamed.txt")).unwrap(),
        "original"
    );
    assert!(!d.path().join("file.txt").exists());
    assert_eq!(
        fs::read_to_string(base.path().join(&plan.id).join("source")).unwrap(),
        "original"
    );
    assert!(
        fs::read_to_string(result.journal)
            .unwrap()
            .contains("completed")
    );
    assert!(
        w.source_file_apply(&plan.id, base.path(), &CANCEL, no_trash)
            .is_err()
    );
    let f = result.entry.unwrap().id;
    let target = file(&mut w, &id, "target");
    let plan = w
        .source_file_prepare(
            &id,
            &f,
            SourceFileAction::Move {
                repository: id.clone(),
                directory: Some(target),
            },
            &CANCEL,
        )
        .unwrap();
    w.source_file_apply(&plan.id, base.path(), &CANCEL, no_trash)
        .unwrap();
    assert!(d.path().join("target/renamed.txt").exists());
}
#[test]
fn folder_copy_preserves_empty_directories_bytes_permissions_and_source() {
    let (d, mut w, id) = fixture();
    fs::create_dir_all(d.path().join("source/empty")).unwrap();
    fs::write(d.path().join("source/code.sh"), "echo fixture\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            d.path().join("source/code.sh"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    let f = file(&mut w, &id, "source");
    let target = file(&mut w, &id, "target");
    let base = tempfile::tempdir().unwrap();
    let plan = w
        .source_file_prepare(
            &id,
            &f,
            SourceFileAction::Copy {
                repository: id.clone(),
                directory: Some(target),
            },
            &CANCEL,
        )
        .unwrap();
    assert_eq!(plan.directories, 2);
    assert_eq!(plan.files, 1);
    w.source_file_apply(&plan.id, base.path(), &CANCEL, no_trash)
        .unwrap();
    assert!(d.path().join("source/code.sh").exists());
    assert!(d.path().join("target/source/empty").is_dir());
    assert_eq!(
        fs::read(d.path().join("target/source/code.sh")).unwrap(),
        fs::read(d.path().join("source/code.sh")).unwrap()
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            fs::metadata(d.path().join("target/source/code.sh"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o755
        );
    }
}
#[test]
fn stale_sources_targets_open_buffers_and_git_metadata_reject_without_writes() {
    let (d, mut w, id) = fixture();
    let f = file(&mut w, &id, "file.txt");
    let target = file(&mut w, &id, "target");
    let base = tempfile::tempdir().unwrap();
    for name in ["../escape", ".git", "target"] {
        assert!(
            w.source_file_prepare(
                &id,
                &f,
                SourceFileAction::Rename { name: name.into() },
                &CANCEL
            )
            .is_err()
        );
    }
    let plan = w
        .source_file_prepare(
            &id,
            &f,
            SourceFileAction::Rename {
                name: "stale.txt".into(),
            },
            &CANCEL,
        )
        .unwrap();
    fs::write(d.path().join("file.txt"), "external").unwrap();
    assert!(
        w.source_file_apply(&plan.id, base.path(), &CANCEL, no_trash)
            .is_err()
    );
    assert!(!d.path().join("stale.txt").exists());
    let plan = w
        .source_file_prepare(
            &id,
            &f,
            SourceFileAction::Copy {
                repository: id.clone(),
                directory: Some(target),
            },
            &CANCEL,
        )
        .unwrap();
    fs::write(d.path().join("target/file.txt"), "occupied").unwrap();
    assert!(
        w.source_file_apply(&plan.id, base.path(), &CANCEL, no_trash)
            .is_err()
    );
    assert_eq!(
        fs::read_to_string(d.path().join("target/file.txt")).unwrap(),
        "occupied"
    );
    let plan = w
        .source_file_prepare(&id, &f, SourceFileAction::Trash, &CANCEL)
        .unwrap();
    let doc = w.source_open(&id, &f, None, &CANCEL).unwrap();
    assert!(
        w.source_file_apply(&plan.id, base.path(), &CANCEL, no_trash)
            .is_err()
    );
    assert!(
        w.source_file_prepare(&id, &f, SourceFileAction::Trash, &CANCEL)
            .is_err()
    );
    w.source_close(&doc.document.id, false).unwrap();
    fs::create_dir_all(d.path().join("nested/.git")).unwrap();
    let folder = file(&mut w, &id, "nested");
    assert!(
        w.source_file_prepare(&id, &folder, SourceFileAction::Trash, &CANCEL)
            .is_err()
    );
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(d.path().join("file.txt"), d.path().join("link")).unwrap();
        w.file_settings(
            &id,
            FileSettings {
                show_hidden: true,
                respect_gitignore: false,
                ..Default::default()
            },
            &CANCEL,
        )
        .unwrap();
        let link = file(&mut w, &id, "link");
        assert!(
            w.source_file_prepare(&id, &link, SourceFileAction::Trash, &CANCEL)
                .is_err()
        );
    }
}
#[test]
fn recycle_is_explicit_cancelled_or_failed_recycle_keeps_original_and_backup() {
    let (d, mut w, id) = fixture();
    let f = file(&mut w, &id, "file.txt");
    let base = tempfile::tempdir().unwrap();
    let trash = tempfile::tempdir().unwrap();
    let plan = w
        .source_file_prepare(&id, &f, SourceFileAction::Trash, &CANCEL)
        .unwrap();
    assert!(
        w.source_file_apply(&plan.id, base.path(), &AtomicBool::new(true), no_trash)
            .is_err()
    );
    assert!(d.path().join("file.txt").exists());
    let plan = w
        .source_file_prepare(&id, &f, SourceFileAction::Trash, &CANCEL)
        .unwrap();
    assert!(
        w.source_file_apply(&plan.id, base.path(), &CANCEL, |_| Err(
            clear_core::Error::Invalid("fixture recycle failure".into())
        ))
        .is_err()
    );
    assert_eq!(
        fs::read_to_string(d.path().join("file.txt")).unwrap(),
        "original"
    );
    assert_eq!(
        fs::read_to_string(base.path().join(plan.id).join("source")).unwrap(),
        "original"
    );
    let plan = w
        .source_file_prepare(&id, &f, SourceFileAction::Trash, &CANCEL)
        .unwrap();
    w.source_file_apply(&plan.id, base.path(), &CANCEL, |path| {
        fs::rename(path, trash.path().join("recycled"))?;
        Ok(())
    })
    .unwrap();
    assert!(!d.path().join("file.txt").exists());
    assert!(trash.path().join("recycled").exists());
}
#[test]
fn cross_repository_copies_and_folder_moves_use_captured_destination_ids() {
    let (d, mut w, id) = fixture();
    let (other, _, other_id) = fixture();
    w.scan(
        ScanSettings {
            roots: vec![
                d.path().display().to_string(),
                other.path().display().to_string(),
            ],
            exclusions: vec![],
        },
        &CANCEL,
    )
    .unwrap();
    let source = file(&mut w, &id, "file.txt");
    let target = file(&mut w, &other_id, "target");
    let base = tempfile::tempdir().unwrap();
    let plan = w
        .source_file_prepare(
            &id,
            &source,
            SourceFileAction::Copy {
                repository: other_id.clone(),
                directory: Some(target.clone()),
            },
            &CANCEL,
        )
        .unwrap();
    w.source_file_apply(&plan.id, base.path(), &CANCEL, no_trash)
        .unwrap();
    assert_eq!(
        fs::read_to_string(other.path().join("target/file.txt")).unwrap(),
        "original"
    );
    assert!(d.path().join("file.txt").is_file());
    fs::create_dir_all(d.path().join("folder/empty")).unwrap();
    fs::write(d.path().join("folder/data"), "moved").unwrap();
    let folder = file(&mut w, &id, "folder");
    assert!(
        w.source_file_prepare(
            &id,
            &folder,
            SourceFileAction::Move {
                repository: id.clone(),
                directory: Some(target.clone())
            },
            &CANCEL
        )
        .is_err()
    );
    let plan = w
        .source_file_prepare(
            &id,
            &folder,
            SourceFileAction::Move {
                repository: other_id,
                directory: Some(target),
            },
            &CANCEL,
        )
        .unwrap();
    w.source_file_apply(&plan.id, base.path(), &CANCEL, no_trash)
        .unwrap();
    assert!(!d.path().join("folder").exists());
    assert!(other.path().join("target/folder/empty").is_dir());
    assert_eq!(
        fs::read_to_string(other.path().join("target/folder/data")).unwrap(),
        "moved"
    );
}
#[test]
fn context_exclusions_are_literal_persisted_and_do_not_delete_files() {
    let (d, mut w, id) = fixture();
    fs::write(d.path().join("a[1].txt"), "keep").unwrap();
    fs::write(d.path().join("a1.txt"), "keep too").unwrap();
    let f = file(&mut w, &id, "a[1].txt");
    let base = tempfile::tempdir().unwrap();
    let settings = w
        .source_file_exclude(&id, &f, base.path(), &CANCEL)
        .unwrap();
    assert!(!settings.exclusions.is_empty());
    let names = w
        .file_tree(&id, None, 0, &CANCEL)
        .unwrap()
        .files
        .into_iter()
        .map(|f| f.name)
        .collect::<Vec<_>>();
    assert!(!names.iter().any(|n| n == "a[1].txt"));
    assert!(names.iter().any(|n| n == "a1.txt"));
    assert!(d.path().join("a[1].txt").exists());
    assert_eq!(
        Workspace::load(base.path()).unwrap().catalog.file_settings[&id],
        settings
    );
}
