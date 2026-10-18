//! Reference preferences and validated checkout/upstream recipes.
use super::*;
fn invalid(message: &str) -> Error {
    Error::Invalid(message.into())
}
pub(super) struct Checkout {
    pub args: Vec<String>,
    pub name: String,
    pub tip: String,
    pub remote: Option<(String, String)>,
}
pub(super) fn local_name(root: &Path, value: &str, cancel: &AtomicBool) -> Result<String> {
    if value.is_empty()
        || value.starts_with('-')
        || value.len() > 2048
        || value.chars().any(char::is_control)
    {
        return Err(invalid("Invalid branch name"));
    }
    runner::text(root, &["check-ref-format", "--branch", value], cancel)
}
pub(super) fn target_name(root: &Path, value: &str, cancel: &AtomicBool) -> Result<String> {
    let value = value.strip_prefix("refs/heads/").unwrap_or(value);
    if value.is_empty()
        || value.len() > 2048
        || value.starts_with('-')
        || value.chars().any(char::is_control)
    {
        return Err(invalid("Invalid target branch name"));
    }
    runner::text(
        root,
        &["check-ref-format", &format!("refs/heads/{value}")],
        cancel,
    )?;
    Ok(value.into())
}
pub(super) fn reference<'a>(state: &'a RepositoryState, value: &str) -> Result<&'a Reference> {
    let exact = state.references.iter().find(|r| r.full_name == value);
    if let Some(reference) = exact {
        return Ok(reference);
    }
    let candidates = state
        .references
        .iter()
        .filter(|r| r.name == value)
        .collect::<Vec<_>>();
    if candidates.len() != 1 {
        return Err(invalid("Choose an unambiguous local or remote reference"));
    }
    Ok(candidates[0])
}
pub(super) fn remote_branch(
    state: &RepositoryState,
    reference: &Reference,
) -> Result<(String, String)> {
    if reference.kind != "remote" || reference.name.ends_with("/HEAD") {
        return Err(invalid("Choose a remote branch, not a symbolic HEAD"));
    }
    let remote = state
        .remotes
        .iter()
        .filter(|r| reference.name.starts_with(&format!("{}/", r.name)))
        .max_by_key(|r| r.name.len())
        .ok_or_else(|| invalid("Remote branch has no configured remote"))?;
    Ok((
        remote.name.clone(),
        reference.name[remote.name.len() + 1..].into(),
    ))
}
pub(super) fn upstream(
    root: &Path,
    state: &RepositoryState,
    name: &str,
    cancel: &AtomicBool,
) -> Result<(String, String)> {
    let name = local_name(root, name, cancel)?;
    let remote = runner::text(
        root,
        &["config", "--get-all", &format!("branch.{name}.remote")],
        cancel,
    )
    .map_err(|_| invalid("Set an upstream branch before updating"))?;
    if remote.lines().count() != 1 || remote.is_empty() {
        return Err(invalid("Upstream must have exactly one remote"));
    }
    if remote != "." && !state.remotes.iter().any(|r| r.name == remote) {
        return Err(invalid("Upstream remote is no longer configured"));
    }
    let reference = runner::text(
        root,
        &["config", "--get-all", &format!("branch.{name}.merge")],
        cancel,
    )?;
    if reference.lines().count() != 1 {
        return Err(invalid("Upstream must track exactly one branch"));
    }
    let branch = reference
        .strip_prefix("refs/heads/")
        .ok_or_else(|| invalid("Upstream must track one remote branch"))?;
    Ok((remote, local_name(root, branch, cancel)?))
}
pub(super) fn checkout(
    root: &Path,
    state: &RepositoryState,
    value: &str,
    name: Option<&str>,
    cancel: &AtomicBool,
) -> Result<Checkout> {
    let reference = reference(state, value)?;
    if state.repository.kind == "bare" {
        return Err(invalid("Bare repositories have no checkout"));
    }
    match reference.kind.as_str() {
        "local" => Ok(Checkout {
            args: vec!["switch".into(), "--no-guess".into(), reference.name.clone()],
            name: reference.name.clone(),
            tip: reference.target.clone(),
            remote: None,
        }),
        "remote" => {
            let remote = remote_branch(state, reference)?;
            let name = local_name(root, name.unwrap_or(&remote.1), cancel)?;
            if state
                .references
                .iter()
                .any(|r| r.kind == "local" && r.name == name)
            {
                return Err(invalid(
                    "Local branch already exists; select it instead of overwriting it",
                ));
            }
            Ok(Checkout {
                args: vec![
                    "switch".into(),
                    "--create".into(),
                    name.clone(),
                    "--track".into(),
                    reference.full_name.clone(),
                ],
                name,
                tip: reference.target.clone(),
                remote: Some(remote),
            })
        }
        _ => Err(invalid(
            "Select a branch; tags and commits use detached checkout",
        )),
    }
}
pub(super) fn pull(remote: String, branch: String, mode: &str) -> Result<Vec<String>> {
    let flag = match mode {
        "ff_only" => "--ff-only",
        "merge" => "--no-rebase",
        "rebase" => "--rebase",
        _ => return Err(invalid("Unknown update mode")),
    };
    Ok(vec!["pull".into(), flag.into(), remote, branch])
}
impl Workspace {
    pub(super) fn record_reference_use(&mut self, repository: &str, state: &mut RepositoryState) {
        let settings = self
            .catalog
            .reference_settings
            .entry(repository.into())
            .or_default();
        let valid = state
            .references
            .iter()
            .map(|r| r.full_name.as_str())
            .collect::<BTreeSet<_>>();
        settings.favorites.retain(|r| valid.contains(r.as_str()));
        settings.recent.retain(|r| valid.contains(r.as_str()));
        if let Some(current) = state
            .references
            .iter()
            .find(|r| r.current && r.kind == "local")
        {
            settings.recent.retain(|r| r != &current.full_name);
            settings.recent.insert(0, current.full_name.clone());
        }
        settings.recent.truncate(32);
        state.reference_settings = settings.clone();
    }
    pub fn favorite_reference(
        &mut self,
        repository: &str,
        reference_id: &str,
        favorite: bool,
        base: &Path,
        cancel: &AtomicBool,
    ) -> Result<ReferenceSettings> {
        let previous = self.catalog.reference_settings.get(repository).cloned();
        let state = self.open(repository, cancel)?;
        let reference = state
            .references
            .iter()
            .find(|r| r.id == reference_id)
            .ok_or_else(|| invalid("Reference selection expired; refresh first"))?;
        let settings = self
            .catalog
            .reference_settings
            .entry(repository.into())
            .or_default();
        settings.favorites.retain(|r| r != &reference.full_name);
        if favorite {
            if settings.favorites.len() >= 1024 {
                return Err(invalid("At most 1024 favorite references per repository"));
            }
            settings.favorites.push(reference.full_name.clone());
            settings.favorites.sort();
        }
        let result = settings.clone();
        if let Err(error) = self.save(base) {
            if let Some(previous) = previous {
                self.catalog
                    .reference_settings
                    .insert(repository.into(), previous);
            } else {
                self.catalog.reference_settings.remove(repository);
            }
            return Err(error);
        }
        Ok(result)
    }
    pub(super) fn rename_reference_preference(&mut self, repository: &str, old: &str, new: &str) {
        if let Some(settings) = self.catalog.reference_settings.get_mut(repository) {
            for name in settings
                .favorites
                .iter_mut()
                .chain(settings.recent.iter_mut())
            {
                if name == old {
                    *name = new.into();
                }
            }
        }
    }
}
