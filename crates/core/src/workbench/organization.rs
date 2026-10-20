//! Catalogue-only grouping preferences. Never changes Git remotes or combines worktrees.
use super::*;
fn invalid(text: &str) -> Error {
    Error::Invalid(text.into())
}
fn name(value: &str) -> Result<String> {
    let value = value.trim();
    if value.is_empty()
        || value.chars().count() > 80
        || value.len() > 320
        || value.chars().any(char::is_control)
    {
        return Err(invalid("Group name must contain 1–80 printable characters"));
    }
    Ok(value.into())
}
pub(super) fn validate(settings: &RepositoryOrganization) -> Result<()> {
    if settings.groups.len() > 128 || settings.assignments.len() > 8192 {
        return Err(invalid("Repository organization limit reached"));
    }
    let mut names = BTreeSet::new();
    for (id, value) in &settings.groups {
        if id.is_empty()
            || id.len() > 128
            || id.chars().any(char::is_control)
            || name(value)? != *value
            || !names.insert(value.to_lowercase())
        {
            return Err(invalid("Invalid or duplicate repository group"));
        }
    }
    for (id, assignment) in &settings.assignments {
        if id.is_empty() || id.len() > 128 || id.chars().any(char::is_control) {
            return Err(invalid("Invalid repository assignment"));
        }
        if let RepositoryAssignment::Manual { id } = assignment
            && !settings.groups.contains_key(id)
        {
            return Err(invalid("Unknown repository group"));
        }
    }
    Ok(())
}
impl Workspace {
    pub fn organize_repositories(
        &mut self,
        action: OrganizeRepositories,
        base: &Path,
        cancel: &AtomicBool,
    ) -> Result<Catalog> {
        check_cancel(cancel)?;
        let mut next = self.catalog.organization.clone();
        match action {
            OrganizeRepositories::RestoreAutomatic => next.assignments.clear(),
            OrganizeRepositories::CreateGroup { name: value } => {
                let value = name(&value)?;
                if next
                    .groups
                    .values()
                    .any(|n| n.to_lowercase() == value.to_lowercase())
                {
                    return Err(invalid("Repository group name is already used"));
                }
                next.groups.insert(crate::unique_id(), value);
            }
            OrganizeRepositories::RenameGroup { id, name: value } => {
                let value = name(&value)?;
                if !next.groups.contains_key(&id) {
                    return Err(invalid("Unknown repository group"));
                }
                if next
                    .groups
                    .iter()
                    .any(|(other, n)| *other != id && n.to_lowercase() == value.to_lowercase())
                {
                    return Err(invalid("Repository group name is already used"));
                }
                next.groups.insert(id, value);
            }
            OrganizeRepositories::DeleteGroup { id } => {
                if next.groups.remove(&id).is_none() {
                    return Err(invalid("Unknown repository group"));
                }
                next.assignments.retain(
                    |_, a| !matches!(a,RepositoryAssignment::Manual{id:assigned}if assigned==&id),
                );
            }
            OrganizeRepositories::Assign {
                repositories,
                assignment,
            } => {
                if repositories.is_empty()
                    || repositories.len() > 4096
                    || repositories.iter().collect::<BTreeSet<_>>().len() != repositories.len()
                    || repositories
                        .iter()
                        .any(|id| !self.catalog.repositories.iter().any(|r| r.id == *id))
                {
                    return Err(invalid(
                        "Select existing repositories before assigning a group",
                    ));
                }
                if let RepositoryAssignment::Manual { id } = &assignment
                    && !next.groups.contains_key(id)
                {
                    return Err(invalid("Unknown repository group"));
                }
                for id in repositories {
                    if assignment == RepositoryAssignment::Automatic {
                        next.assignments.remove(&id);
                    } else {
                        next.assignments.insert(id, assignment.clone());
                    }
                }
            }
            OrganizeRepositories::SetView { view } => next.view = view,
        }
        validate(&next)?;
        check_cancel(cancel)?;
        let mut staged = Workspace {
            catalog: self.catalog.clone(),
            ignored: self.ignored.clone(),
            manual: self.manual.clone(),
            ..Default::default()
        };
        staged.catalog.organization = next;
        staged.save(base)?;
        self.catalog.organization = staged.catalog.organization;
        Ok(self.catalog.clone())
    }
}
