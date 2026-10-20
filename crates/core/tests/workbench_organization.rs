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
}
fn fixture() -> (tempfile::TempDir, Workspace, String, String) {
    let temp = tempfile::tempdir().unwrap();
    init(&temp.path().join("a"));
    init(&temp.path().join("b"));
    git(
        &temp.path().join("a"),
        &["remote", "add", "origin", "git@example.com:team/shared.git"],
    );
    git(
        &temp.path().join("b"),
        &[
            "remote",
            "add",
            "origin",
            "https://example.com/team/shared.git",
        ],
    );
    let mut w = Workspace::default();
    let c = w
        .scan(
            ScanSettings {
                roots: vec![temp.path().to_str().unwrap().into()],
                exclusions: vec!["settings".into()],
            },
            &RUN,
        )
        .unwrap();
    let a = c
        .repositories
        .iter()
        .find(|r| r.name == "a")
        .unwrap()
        .id
        .clone();
    let b = c
        .repositories
        .iter()
        .find(|r| r.name == "b")
        .unwrap()
        .id
        .clone();
    (temp, w, a, b)
}
fn create(w: &mut Workspace, base: &Path, name: &str) -> String {
    let c = w
        .organize_repositories(
            OrganizeRepositories::CreateGroup { name: name.into() },
            base,
            &RUN,
        )
        .unwrap();
    c.organization
        .groups
        .into_iter()
        .find(|(_, n)| n == name.trim())
        .unwrap()
        .0
}
#[test]
fn manual_groups_assignments_and_list_preferences_survive_reload_and_scan() {
    let (temp, mut w, a, b) = fixture();
    let base = temp.path().join("settings");
    assert_eq!(
        w.catalog.repositories[0].match_key,
        w.catalog.repositories[1].match_key
    );
    let id = create(&mut w, &base, " 中文项目 ");
    let view = RepositoryListView {
        sort: RepositorySort::Changes,
        descending: true,
        favorites_first: false,
        grouped: false,
    };
    w.organize_repositories(
        OrganizeRepositories::SetView { view: view.clone() },
        &base,
        &RUN,
    )
    .unwrap();
    w.organize_repositories(
        OrganizeRepositories::Assign {
            repositories: vec![a.clone(), b.clone()],
            assignment: RepositoryAssignment::Manual { id: id.clone() },
        },
        &base,
        &RUN,
    )
    .unwrap();
    let expected = w.catalog.organization.clone();
    let mut loaded = Workspace::load(&base).unwrap();
    assert_eq!(loaded.catalog.organization, expected);
    loaded.scan(w.catalog.settings.clone(), &RUN).unwrap();
    assert_eq!(loaded.catalog.organization, expected);
    assert_eq!(loaded.catalog.repositories.len(), 2);
    assert_eq!(loaded.catalog.organization.view, view);
    assert_eq!(
        fs::read_to_string(temp.path().join("a/.git/config"))
            .unwrap()
            .matches("team/shared.git")
            .count(),
        1
    );
}
#[test]
fn separate_and_automatic_assignments_do_not_modify_git_or_rules() {
    let (temp, mut w, a, b) = fixture();
    let base = temp.path().join("settings");
    let config = fs::read(temp.path().join("a/.git/config")).unwrap();
    w.catalog.file_settings.insert(
        a.clone(),
        FileSettings {
            exclusions: "build/".into(),
            ..Default::default()
        },
    );
    w.organize_repositories(
        OrganizeRepositories::Assign {
            repositories: vec![a.clone()],
            assignment: RepositoryAssignment::Separate,
        },
        &base,
        &RUN,
    )
    .unwrap();
    assert_eq!(
        w.catalog.organization.assignments[&a],
        RepositoryAssignment::Separate
    );
    assert!(!w.catalog.organization.assignments.contains_key(&b));
    w.organize_repositories(
        OrganizeRepositories::Assign {
            repositories: vec![a.clone()],
            assignment: RepositoryAssignment::Automatic,
        },
        &base,
        &RUN,
    )
    .unwrap();
    assert!(!w.catalog.organization.assignments.contains_key(&a));
    assert_eq!(fs::read(temp.path().join("a/.git/config")).unwrap(), config);
    assert_eq!(w.catalog.file_settings[&a].exclusions, "build/");
}
#[test]
fn rename_preserves_ids_and_delete_restores_only_affected_assignments() {
    let (temp, mut w, a, b) = fixture();
    let base = temp.path().join("settings");
    let id = create(&mut w, &base, "old");
    w.organize_repositories(
        OrganizeRepositories::Assign {
            repositories: vec![a.clone()],
            assignment: RepositoryAssignment::Manual { id: id.clone() },
        },
        &base,
        &RUN,
    )
    .unwrap();
    w.organize_repositories(
        OrganizeRepositories::Assign {
            repositories: vec![b.clone()],
            assignment: RepositoryAssignment::Separate,
        },
        &base,
        &RUN,
    )
    .unwrap();
    w.organize_repositories(
        OrganizeRepositories::RenameGroup {
            id: id.clone(),
            name: "renamed".into(),
        },
        &base,
        &RUN,
    )
    .unwrap();
    assert_eq!(w.catalog.organization.groups[&id], "renamed");
    assert_eq!(
        w.catalog.organization.assignments[&a],
        RepositoryAssignment::Manual { id: id.clone() }
    );
    w.organize_repositories(OrganizeRepositories::DeleteGroup { id }, &base, &RUN)
        .unwrap();
    assert!(w.catalog.organization.groups.is_empty());
    assert!(!w.catalog.organization.assignments.contains_key(&a));
    assert_eq!(
        w.catalog.organization.assignments[&b],
        RepositoryAssignment::Separate
    );
    assert_eq!(w.catalog.repositories.len(), 2);
}
#[test]
fn invalid_names_ids_duplicates_and_empty_or_foreign_selections_are_rejected() {
    let (temp, mut w, a, _) = fixture();
    let base = temp.path().join("settings");
    let id = create(&mut w, &base, "Project");
    let initial = w.catalog.organization.clone();
    for name in ["", "  ", "project", "bad\nname"] {
        assert!(
            w.organize_repositories(
                OrganizeRepositories::CreateGroup { name: name.into() },
                &base,
                &RUN
            )
            .is_err()
        );
    }
    assert!(
        w.organize_repositories(
            OrganizeRepositories::CreateGroup {
                name: "x".repeat(81)
            },
            &base,
            &RUN
        )
        .is_err()
    );
    for repositories in [vec![], vec![a.clone(), a.clone()], vec!["foreign".into()]] {
        assert!(
            w.organize_repositories(
                OrganizeRepositories::Assign {
                    repositories,
                    assignment: RepositoryAssignment::Manual { id: id.clone() }
                },
                &base,
                &RUN
            )
            .is_err()
        );
    }
    assert!(
        w.organize_repositories(
            OrganizeRepositories::Assign {
                repositories: vec![a],
                assignment: RepositoryAssignment::Manual {
                    id: "foreign".into()
                }
            },
            &base,
            &RUN
        )
        .is_err()
    );
    assert!(
        w.organize_repositories(
            OrganizeRepositories::RenameGroup {
                id: "foreign".into(),
                name: "new".into()
            },
            &base,
            &RUN
        )
        .is_err()
    );
    assert_eq!(w.catalog.organization, initial);
}
#[test]
fn failed_persistence_and_cancel_keep_memory_and_disk_unchanged() {
    let (temp, mut w, a, _) = fixture();
    let base = temp.path().join("settings");
    let id = create(&mut w, &base, "retained");
    let initial = w.catalog.organization.clone();
    let bytes = fs::read(base.join("git-workspace.json")).unwrap();
    let blocked = temp.path().join("blocked");
    fs::write(&blocked, "blocked").unwrap();
    assert!(
        w.organize_repositories(
            OrganizeRepositories::Assign {
                repositories: vec![a.clone()],
                assignment: RepositoryAssignment::Manual { id }
            },
            &blocked,
            &RUN
        )
        .is_err()
    );
    assert!(
        w.organize_repositories(
            OrganizeRepositories::DeleteGroup {
                id: initial.groups.keys().next().unwrap().clone()
            },
            &base,
            &AtomicBool::new(true)
        )
        .is_err()
    );
    assert_eq!(w.catalog.organization, initial);
    assert_eq!(fs::read(base.join("git-workspace.json")).unwrap(), bytes);
}
#[test]
fn relocation_carries_original_assignment_and_combines_discovered_destination() {
    let (temp, mut w, a, _) = fixture();
    let base = temp.path().join("settings");
    let group = create(&mut w, &base, "moving");
    w.organize_repositories(
        OrganizeRepositories::Assign {
            repositories: vec![a.clone()],
            assignment: RepositoryAssignment::Manual { id: group.clone() },
        },
        &base,
        &RUN,
    )
    .unwrap();
    let moved = temp.path().join("moved");
    fs::rename(temp.path().join("a"), &moved).unwrap();
    w.scan(w.catalog.settings.clone(), &RUN).unwrap();
    let new_id = w
        .catalog
        .repositories
        .iter()
        .find(|r| r.name == "moved")
        .unwrap()
        .id
        .clone();
    w.organize_repositories(
        OrganizeRepositories::Assign {
            repositories: vec![new_id.clone()],
            assignment: RepositoryAssignment::Separate,
        },
        &base,
        &RUN,
    )
    .unwrap();
    let p = w
        .registration_prepare(moved.to_str().unwrap(), Some(&a), &RUN)
        .unwrap();
    w.registration_apply(&p.id, &base, &RUN).unwrap();
    assert!(!w.catalog.organization.assignments.contains_key(&a));
    assert_eq!(
        w.catalog.organization.assignments[&new_id],
        RepositoryAssignment::Manual { id: group }
    );
    assert_eq!(
        Workspace::load(&base).unwrap().catalog.organization,
        w.catalog.organization
    );
}
#[test]
fn removal_and_readdition_keep_independent_assignment_without_resurrecting_deleted_groups() {
    let (temp, mut w, a, _) = fixture();
    let base = temp.path().join("settings");
    let group = create(&mut w, &base, "restore");
    w.organize_repositories(
        OrganizeRepositories::Assign {
            repositories: vec![a.clone()],
            assignment: RepositoryAssignment::Manual { id: group.clone() },
        },
        &base,
        &RUN,
    )
    .unwrap();
    w.remove_persisted(&a, &base).unwrap();
    let mut w = Workspace::load(&base).unwrap();
    let p = w
        .registration_prepare(temp.path().join("a").to_str().unwrap(), None, &RUN)
        .unwrap();
    w.registration_apply(&p.id, &base, &RUN).unwrap();
    assert_eq!(
        w.catalog.organization.assignments[&a],
        RepositoryAssignment::Manual { id: group.clone() }
    );
    w.remove_persisted(&a, &base).unwrap();
    w.organize_repositories(OrganizeRepositories::DeleteGroup { id: group }, &base, &RUN)
        .unwrap();
    let p = w
        .registration_prepare(temp.path().join("a").to_str().unwrap(), None, &RUN)
        .unwrap();
    w.registration_apply(&p.id, &base, &RUN).unwrap();
    assert!(!w.catalog.organization.assignments.contains_key(&a));
}
#[test]
fn legacy_catalogues_default_and_malformed_organization_is_refused() {
    let (temp, w, _, _) = fixture();
    let base = temp.path().join("settings");
    w.save(&base).unwrap();
    let file = base.join("git-workspace.json");
    let mut value: serde_json::Value = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    value.as_object_mut().unwrap().remove("organization");
    fs::write(&file, serde_json::to_vec(&value).unwrap()).unwrap();
    assert_eq!(
        Workspace::load(&base).unwrap().catalog.organization,
        RepositoryOrganization::default()
    );
    value["organization"] = serde_json::json!({"groups":{},"assignments":{"repo":{"kind":"manual","id":"foreign"}},"view":{}});
    fs::write(&file, serde_json::to_vec(&value).unwrap()).unwrap();
    assert!(Workspace::load(&base).is_err());
}
#[test]
fn restoring_automatic_clears_retained_overrides_but_keeps_groups_and_rules() {
    let (temp, mut w, a, b) = fixture();
    let base = temp.path().join("settings");
    let group = create(&mut w, &base, "keep");
    w.organize_repositories(
        OrganizeRepositories::Assign {
            repositories: vec![a.clone(), b.clone()],
            assignment: RepositoryAssignment::Separate,
        },
        &base,
        &RUN,
    )
    .unwrap();
    w.remove_persisted(&a, &base).unwrap();
    w.organize_repositories(OrganizeRepositories::RestoreAutomatic, &base, &RUN)
        .unwrap();
    assert!(w.catalog.organization.assignments.is_empty());
    assert_eq!(w.catalog.organization.groups[&group], "keep");
    assert_eq!(
        Workspace::load(&base).unwrap().catalog.organization,
        w.catalog.organization
    );
}
