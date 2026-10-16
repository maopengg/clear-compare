use clear_contracts::{CompareRequest, SavedSession, SessionView};
use clear_core::{compare, rules, sessions};
use std::{fs, sync::atomic::AtomicBool};
fn sample() -> SavedSession {
    SavedSession {
        request: CompareRequest {
            left: "/example/a-left".into(),
            right: "/example/a-right".into(),
            exclusions: "-./docs/".into(),
            content: false,
        },
        english: false,
        view: SessionView::default(),
    }
}
#[test]
fn active_project_migrates_legacy_once_and_retains_independent_settings_across_reopen() {
    let t = tempfile::tempdir().unwrap();
    let first = sample();
    sessions::save_last(t.path(), &first).unwrap();
    let a = sessions::active(t.path()).unwrap().unwrap();
    assert_eq!(a.session.request.exclusions, "-./docs/");
    assert_eq!(sessions::active(t.path()).unwrap().unwrap().id, a.id);
    assert_eq!(sessions::list(t.path()).unwrap().len(), 1);
    let mut second = sample();
    second.request.left = "/example/b-left".into();
    second.request.right = "/example/b-right".into();
    second.request.exclusions = "-*.log;-./cache/".into();
    let b = sessions::save(t.path(), None, None, "项目 B", &second).unwrap();
    sessions::set_active(t.path(), Some(&b.id)).unwrap();
    assert_eq!(
        sessions::active(t.path())
            .unwrap()
            .unwrap()
            .session
            .request
            .exclusions,
        second.request.exclusions
    );
    assert_eq!(
        sessions::load(t.path(), &a.id)
            .unwrap()
            .session
            .request
            .exclusions,
        first.request.exclusions
    );
    second.request.exclusions = "-*.tmp".into();
    let updated = sessions::save(
        t.path(),
        Some(&b.id),
        Some(&b.revision),
        "项目 B 改名",
        &second,
    )
    .unwrap();
    assert!(sessions::save(t.path(), Some(&b.id), Some(&b.revision), "stale", &first).is_err());
    assert_eq!(
        sessions::active(t.path()).unwrap().unwrap().name,
        "项目 B 改名"
    );
    sessions::archive(t.path(), &b.id, &updated.revision).unwrap();
    assert!(sessions::active(t.path()).unwrap().is_none());
    assert_eq!(sessions::list(t.path()).unwrap().len(), 1);
    assert!(sessions::set_active(t.path(), Some("../escape")).is_err());
}
#[test]
fn right_click_exclusion_uses_captured_native_ids_and_only_the_selected_subtree() {
    let t = tempfile::tempdir().unwrap();
    let l = t.path().join("l");
    let r = t.path().join("r");
    fs::create_dir_all(l.join("docs")).unwrap();
    fs::create_dir_all(l.join("nested/docs")).unwrap();
    fs::create_dir(&r).unwrap();
    fs::write(l.join("docs/a.txt"), "a").unwrap();
    fs::write(l.join("nested/docs/a.txt"), "b").unwrap();
    let snapshot = compare::compare(
        &CompareRequest {
            left: l.to_string_lossy().into(),
            right: r.to_string_lossy().into(),
            exclusions: String::new(),
            content: false,
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    let id = &snapshot
        .report
        .rows
        .iter()
        .find(|row| row.path == "docs")
        .unwrap()
        .id;
    let rule = rules::exclude_entry(&snapshot, id, "-*.log").unwrap();
    let parsed = rules::Rules::parse(&rule).unwrap();
    assert!(parsed.excludes(std::path::Path::new("docs/a.txt"), false));
    assert!(!parsed.excludes(std::path::Path::new("nested/docs/a.txt"), false));
    assert!(parsed.excludes(std::path::Path::new("file.log"), false));
    assert_eq!(rules::exclude_entry(&snapshot, id, &rule).unwrap(), rule);
    assert!(rules::exclude_entry(&snapshot, "bad-id", &rule).is_err());
}
#[cfg(unix)]
#[test]
fn literal_glob_characters_and_spaces_do_not_broaden_exclusions() {
    let t = tempfile::tempdir().unwrap();
    let l = t.path().join("l");
    let r = t.path().join("r");
    fs::create_dir(&l).unwrap();
    fs::create_dir(&r).unwrap();
    let name = "a[1]*? .txt ";
    fs::write(l.join(name), "a").unwrap();
    let snapshot = compare::compare(
        &CompareRequest {
            left: l.to_string_lossy().into(),
            right: r.to_string_lossy().into(),
            exclusions: String::new(),
            content: false,
        },
        &AtomicBool::new(false),
    )
    .unwrap();
    let rule = rules::exclude_entry(&snapshot, &snapshot.report.rows[0].id, "").unwrap();
    let parsed = rules::Rules::parse(&rule).unwrap();
    assert!(parsed.excludes(std::path::Path::new(name), false));
    assert!(!parsed.excludes(std::path::Path::new("a1xx .txt"), false));
}
