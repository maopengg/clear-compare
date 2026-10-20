use clear_contracts::workbench::*;
use clear_core::workbench::Workspace;
use std::{fs, path::Path, process::Command, sync::atomic::AtomicBool};
const RUN: AtomicBool = AtomicBool::new(false);
fn git(root: &Path, args: &[&str]) {
    let o = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}
fn init(root: &Path) {
    fs::create_dir_all(root).unwrap();
    git(root, &["init", "-b", "main"]);
    fs::write(root.join("file.txt"), "retained\n").unwrap();
}
fn prepare(w: &mut Workspace, path: &Path, old: Option<&str>) -> RepositoryRegistration {
    w.registration_prepare(path.to_str().unwrap(), old, &RUN)
        .unwrap()
}
#[test]
fn manual_registration_is_exact_persistent_and_survives_rescan() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("空 格 repo");
    init(&repo);
    init(&repo.join("nested"));
    let base = temp.path().join("settings");
    let mut w = Workspace::default();
    let p = prepare(&mut w, &repo, None);
    assert!(w.catalog.repositories.is_empty());
    assert!(!base.exists());
    let c = w.registration_apply(&p.id, &base, &RUN).unwrap();
    assert_eq!(c.repositories.len(), 1);
    let id = c.repositories[0].id.clone();
    assert!(w.registration_apply(&p.id, &base, &RUN).is_err());
    let mut w = Workspace::load(&base).unwrap();
    let c = w.scan(ScanSettings::default(), &RUN).unwrap();
    assert_eq!(c.repositories.len(), 1);
    assert_eq!(c.repositories[0].id, id);
    assert!(c.repositories[0].available);
    w.remove(&id).unwrap();
    w.save(&base).unwrap();
    let mut w = Workspace::load(&base).unwrap();
    assert!(
        w.scan(ScanSettings::default(), &RUN)
            .unwrap()
            .repositories
            .is_empty()
    );
    assert_eq!(
        fs::read_to_string(repo.join("file.txt")).unwrap(),
        "retained\n"
    );
}
#[test]
fn relocated_missing_repository_preserves_preferences_and_ignores_old_location() {
    let temp = tempfile::tempdir().unwrap();
    let old = temp.path().join("old");
    let new = temp.path().join("moved");
    init(&old);
    git(
        &old,
        &[
            "remote",
            "add",
            "origin",
            "https://example.com/team/repo.git",
        ],
    );
    let mut w = Workspace::default();
    let p = prepare(&mut w, &old, None);
    let base = temp.path().join("settings");
    let c = w.registration_apply(&p.id, &base, &RUN).unwrap();
    let id = c.repositories[0].id.clone();
    w.favorite(&id, true).unwrap();
    w.open(&id, &RUN).unwrap();
    w.catalog.reference_settings.insert(
        id.clone(),
        ReferenceSettings {
            favorites: vec!["main".into()],
            recent: vec!["main".into()],
        },
    );
    w.catalog.file_settings.insert(
        id.clone(),
        FileSettings {
            exclusions: "build/".into(),
            ..Default::default()
        },
    );
    let recent = w.catalog.repositories[0].last_opened.clone();
    fs::rename(&old, &new).unwrap();
    let plan = prepare(&mut w, &new, Some(&id));
    assert_eq!(plan.previous.as_ref().unwrap().id, id);
    let new_id = plan.repository.id.clone();
    let c = w.registration_apply(&plan.id, &base, &RUN).unwrap();
    assert_eq!(c.repositories.len(), 1);
    assert!(c.repositories[0].favorite);
    assert_eq!(c.repositories[0].last_opened, recent);
    assert!(!c.file_settings.contains_key(&id));
    assert_eq!(c.file_settings[&new_id].exclusions, "build/");
    assert_eq!(c.reference_settings[&new_id].favorites, vec!["main"]);
    init(&old);
    let mut w = Workspace::load(&base).unwrap();
    let c = w
        .scan(
            ScanSettings {
                roots: vec![temp.path().to_str().unwrap().into()],
                exclusions: vec!["settings".into()],
            },
            &RUN,
        )
        .unwrap();
    assert_eq!(c.repositories.len(), 1);
    assert_eq!(c.repositories[0].id, new_id);
    assert_eq!(
        fs::read_to_string(new.join("file.txt")).unwrap(),
        "retained\n"
    );
}
#[test]
fn duplicate_active_original_unknown_and_nonroot_are_refused() {
    let temp = tempfile::tempdir().unwrap();
    let a = temp.path().join("a");
    let b = temp.path().join("b");
    init(&a);
    init(&b);
    fs::create_dir(a.join("subdir")).unwrap();
    let mut w = Workspace::default();
    let p = prepare(&mut w, &a, None);
    let c = w
        .registration_apply(&p.id, &temp.path().join("settings"), &RUN)
        .unwrap();
    let id = c.repositories[0].id.clone();
    assert!(
        w.registration_prepare(a.to_str().unwrap(), None, &RUN)
            .is_err()
    );
    assert!(
        w.registration_prepare(b.to_str().unwrap(), Some(&id), &RUN)
            .unwrap_err()
            .to_string()
            .contains("still available")
    );
    assert!(
        w.registration_prepare(b.to_str().unwrap(), Some("unknown"), &RUN)
            .is_err()
    );
    assert!(
        w.registration_prepare(a.join("subdir").to_str().unwrap(), None, &RUN)
            .is_err()
    );
    assert!(w.registration_prepare("relative", None, &RUN).is_err());
}
#[test]
fn save_failure_keeps_catalogue_and_registration_one_shot() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    init(&repo);
    let mut w = Workspace::default();
    let plan = prepare(&mut w, &repo, None);
    let base = temp.path().join("bad");
    fs::write(&base, "blocked").unwrap();
    assert!(w.registration_apply(&plan.id, &base, &RUN).is_err());
    assert!(w.catalog.repositories.is_empty());
    assert!(w.repositories.is_empty());
    assert!(w.registration_apply(&plan.id, temp.path(), &RUN).is_err());
}
#[test]
fn identity_remote_catalogue_and_original_reappearance_are_rechecked() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    init(&repo);
    let mut w = Workspace::default();
    let p = prepare(&mut w, &repo, None);
    fs::rename(&repo, temp.path().join("other")).unwrap();
    init(&repo);
    assert!(
        w.registration_apply(&p.id, &temp.path().join("settings"), &RUN)
            .is_err()
    );
    assert!(w.catalog.repositories.is_empty());
    let p = prepare(&mut w, &repo, None);
    git(
        &repo,
        &["remote", "add", "origin", "https://example.com/changed.git"],
    );
    assert!(
        w.registration_apply(&p.id, &temp.path().join("settings"), &RUN)
            .is_err()
    );
    let p = prepare(&mut w, &repo, None);
    let c = w
        .registration_apply(&p.id, &temp.path().join("settings"), &RUN)
        .unwrap();
    let id = c.repositories[0].id.clone();
    let moved = temp.path().join("moved");
    fs::rename(&repo, &moved).unwrap();
    let p = prepare(&mut w, &moved, Some(&id));
    init(&repo);
    assert!(
        w.registration_apply(&p.id, &temp.path().join("settings"), &RUN)
            .is_err()
    );
    assert_eq!(w.catalog.repositories[0].id, id);
}
#[test]
fn open_buffers_and_cancel_protect_existing_catalogue() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    init(&repo);
    let mut w = Workspace::default();
    let p = prepare(&mut w, &repo, None);
    let c = w
        .registration_apply(&p.id, &temp.path().join("settings"), &RUN)
        .unwrap();
    let id = c.repositories[0].id.clone();
    let f = w
        .file_tree(&id, None, 0, &RUN)
        .unwrap()
        .files
        .into_iter()
        .find(|f| f.name == "file.txt")
        .unwrap();
    let doc = w.source_open(&id, &f.id, None, &RUN).unwrap();
    let moved = temp.path().join("moved");
    fs::rename(&repo, &moved).unwrap();
    assert!(
        w.registration_prepare(moved.to_str().unwrap(), Some(&id), &RUN)
            .is_err()
    );
    w.source_close(&doc.document.id, false).unwrap();
    let p = prepare(&mut w, &moved, Some(&id));
    assert!(
        w.registration_apply(&p.id, &temp.path().join("settings"), &AtomicBool::new(true))
            .is_err()
    );
    assert_eq!(w.catalog.repositories[0].id, id);
}
#[test]
fn bare_registration_and_missing_manual_record_survive_restart() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("bare.git");
    fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "--bare"]);
    let mut w = Workspace::default();
    let p = prepare(&mut w, &repo, None);
    assert_eq!(p.repository.kind, "bare");
    let base = temp.path().join("settings");
    w.registration_apply(&p.id, &base, &RUN).unwrap();
    fs::rename(&repo, temp.path().join("moved.git")).unwrap();
    let mut w = Workspace::load(&base).unwrap();
    let c = w.scan(ScanSettings::default(), &RUN).unwrap();
    assert_eq!(c.repositories.len(), 1);
    assert!(!c.repositories[0].available);
}
#[test]
fn relink_combines_a_destination_already_discovered_after_moving() {
    let temp = tempfile::tempdir().unwrap();
    let old = temp.path().join("old");
    let new = temp.path().join("new");
    init(&old);
    let mut w = Workspace::default();
    let settings = ScanSettings {
        roots: vec![temp.path().to_str().unwrap().into()],
        exclusions: vec!["settings".into()],
    };
    let c = w.scan(settings.clone(), &RUN).unwrap();
    let old_id = c.repositories[0].id.clone();
    w.catalog.file_settings.insert(
        old_id.clone(),
        FileSettings {
            exclusions: "old-rules/".into(),
            ..Default::default()
        },
    );
    fs::rename(&old, &new).unwrap();
    let c = w.scan(settings, &RUN).unwrap();
    assert_eq!(c.repositories.len(), 2);
    let new_id = c
        .repositories
        .iter()
        .find(|r| r.path.ends_with("new"))
        .unwrap()
        .id
        .clone();
    w.favorite(&new_id, true).unwrap();
    w.catalog.file_settings.insert(
        new_id.clone(),
        FileSettings {
            exclusions: "new-rules/".into(),
            ..Default::default()
        },
    );
    let plan = prepare(&mut w, &new, Some(&old_id));
    assert_eq!(plan.existing.as_ref().unwrap().id, new_id);
    let c = w
        .registration_apply(&plan.id, &temp.path().join("settings"), &RUN)
        .unwrap();
    assert_eq!(c.repositories.len(), 1);
    assert!(c.repositories[0].favorite);
    assert_eq!(c.repositories[0].id, new_id);
    assert_eq!(c.file_settings[&new_id].exclusions, "old-rules/");
}
#[test]
fn persisted_removal_rolls_back_on_save_failure_and_can_be_readded() {
    let temp = tempfile::tempdir().unwrap();
    let repo = temp.path().join("repo");
    init(&repo);
    let mut w = Workspace::default();
    let p = prepare(&mut w, &repo, None);
    let base = temp.path().join("settings");
    let c = w.registration_apply(&p.id, &base, &RUN).unwrap();
    let id = c.repositories[0].id.clone();
    let blocked = temp.path().join("blocked");
    fs::write(&blocked, "blocked").unwrap();
    assert!(w.remove_persisted(&id, &blocked).is_err());
    assert_eq!(w.catalog.repositories.len(), 1);
    assert!(w.open(&id, &RUN).is_ok());
    w.remove_persisted(&id, &base).unwrap();
    let mut w = Workspace::load(&base).unwrap();
    assert!(
        w.scan(ScanSettings::default(), &RUN)
            .unwrap()
            .repositories
            .is_empty()
    );
    let p = prepare(&mut w, &repo, None);
    w.registration_apply(&p.id, &base, &RUN).unwrap();
    assert_eq!(
        w.scan(ScanSettings::default(), &RUN)
            .unwrap()
            .repositories
            .len(),
        1
    );
}
