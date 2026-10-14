use clear_contracts::{CompareRequest, FolderSort, SavedSession, SessionView};
use clear_core::sessions;
use std::fs;
fn sample() -> SavedSession {
    SavedSession {
        request: CompareRequest {
            left: "/example/offline-left".into(),
            right: "/example/offline-right".into(),
            exclusions: "-./.git/;-node_modules/".into(),
            content: true,
        },
        english: false,
        view: SessionView {
            search: "中文".into(),
            sort: FolderSort::LeftSize,
            reverse_sync: true,
            ..Default::default()
        },
    }
}
#[test]
fn named_sessions_reopen_update_with_revisions_and_archive_without_data_loss() {
    let t = tempfile::tempdir().unwrap();
    assert!(sessions::list(t.path()).unwrap().is_empty());
    assert!(!t.path().join("sessions").exists());
    let first = sessions::save(t.path(), None, None, "开发比较", &sample()).unwrap();
    let reopened = sessions::list(t.path()).unwrap();
    assert_eq!(reopened[0].name, "开发比较");
    assert_eq!(reopened[0].session.view.search, "中文");
    assert!(reopened[0].session.view.reverse_sync);
    let updated = sessions::save(
        t.path(),
        Some(&first.id),
        Some(&first.revision),
        "更新名称",
        &sample(),
    )
    .unwrap();
    assert_ne!(updated.revision, first.revision);
    assert!(
        sessions::save(
            t.path(),
            Some(&first.id),
            Some(&first.revision),
            "stale",
            &sample()
        )
        .is_err()
    );
    assert!(sessions::archive(t.path(), &first.id, &first.revision).is_err());
    sessions::archive(t.path(), &updated.id, &updated.revision).unwrap();
    assert!(sessions::list(t.path()).unwrap().is_empty());
    let archived = fs::read_dir(t.path().join("sessions/trash"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    let data: serde_json::Value =
        serde_json::from_slice(&fs::read(archived.path()).unwrap()).unwrap();
    assert_eq!(data["name"], "更新名称");
}
#[test]
fn legacy_last_session_defaults_view_and_traversal_ids_are_rejected() {
    let t = tempfile::tempdir().unwrap();
    assert!(sessions::load_last(t.path()).unwrap().is_none());
    let mut legacy = serde_json::to_value(sample()).unwrap();
    legacy.as_object_mut().unwrap().remove("view");
    fs::write(
        t.path().join("session.json"),
        serde_json::to_vec(&legacy).unwrap(),
    )
    .unwrap();
    let loaded = sessions::load_last(t.path()).unwrap().unwrap();
    assert!(loaded.view.recursive);
    assert!(loaded.view.differences_only);
    assert!(loaded.view.search.is_empty());
    sessions::save_last(t.path(), &sample()).unwrap();
    assert_eq!(
        sessions::load_last(t.path()).unwrap().unwrap().view.search,
        "中文"
    );
    assert!(sessions::save(t.path(), Some("../escape"), None, "name", &sample()).is_err());
    assert!(sessions::save(t.path(), None, None, "", &sample()).is_err());
    let mut invalid = sample();
    invalid.request.left.clear();
    assert!(sessions::save(t.path(), None, None, "name", &invalid).is_err());
}
#[cfg(unix)]
#[test]
fn session_symlinks_are_not_followed() {
    let t = tempfile::tempdir().unwrap();
    let outside = t.path().join("outside");
    fs::write(&outside, "unchanged").unwrap();
    std::os::unix::fs::symlink(&outside, t.path().join("session.json")).unwrap();
    assert!(sessions::load_last(t.path()).is_err());
    assert!(sessions::save_last(t.path(), &sample()).is_err());
    assert_eq!(fs::read_to_string(outside).unwrap(), "unchanged");
}

#[test]
fn a_busy_storage_lock_fails_without_creating_or_overwriting_a_session() {
    let t = tempfile::tempdir().unwrap();
    let entry = sessions::save(t.path(), None, None, "first", &sample()).unwrap();
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(t.path().join("sessions/.lock"))
        .unwrap();
    lock.lock().unwrap();
    assert!(
        sessions::save(
            t.path(),
            Some(&entry.id),
            Some(&entry.revision),
            "blocked",
            &sample()
        )
        .is_err()
    );
    assert_eq!(sessions::load(t.path(), &entry.id).unwrap().name, "first");
    drop(lock);
    assert!(
        sessions::save(
            t.path(),
            Some(&entry.id),
            Some(&entry.revision),
            "unblocked",
            &sample()
        )
        .is_ok()
    );
}
