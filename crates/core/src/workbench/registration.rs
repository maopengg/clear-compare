//! Reviewed, persistent catalogue edits. Never moves or writes a repository.
use super::*;
pub(super) struct Prepared {
    view: RepositoryRegistration,
    captured: Captured,
}
pub(super) fn validate_manual(paths: &BTreeSet<String>) -> Result<()> {
    if paths.len() > 256
        || paths.iter().any(|p| {
            p.is_empty() || p.len() > 8192 || p.contains('\0') || !Path::new(p).is_absolute()
        })
    {
        return Err(Error::Invalid(
            "Invalid manually registered repository paths".into(),
        ));
    }
    Ok(())
}
fn invalid(message: &str) -> Error {
    Error::Invalid(message.into())
}
impl Workspace {
    pub fn remove_persisted(&mut self, repository_id: &str, base: &Path) -> Result<Catalog> {
        if self.open_sources_in(repository_id) {
            return Err(invalid(
                "Close this repository's source tabs before removing it",
            ));
        }
        let mut next = Workspace {
            catalog: self.catalog.clone(),
            ignored: self.ignored.clone(),
            manual: self.manual.clone(),
            ..Default::default()
        };
        next.remove(repository_id)?;
        next.save(base)?;
        self.catalog = next.catalog;
        self.ignored = next.ignored;
        self.manual = next.manual;
        self.repositories.remove(repository_id);
        self.pending = None;
        self.registration = None;
        Ok(self.catalog.clone())
    }
    pub fn registration_prepare(
        &mut self,
        path: &str,
        previous: Option<&str>,
        cancel: &AtomicBool,
    ) -> Result<RepositoryRegistration> {
        self.registration = None;
        if path.is_empty()
            || path.len() > 8192
            || path.contains('\0')
            || !Path::new(path).is_absolute()
        {
            return Err(invalid("Select an absolute repository root"));
        }
        let root = canonical_root(path)?;
        let mut captured = inspect(&root, cancel)?;
        status::summary(&mut captured, cancel)?;
        let previous = previous
            .map(|id| {
                self.catalog
                    .repositories
                    .iter()
                    .find(|r| r.id == id)
                    .cloned()
                    .ok_or_else(|| invalid("Unknown repository"))
            })
            .transpose()?;
        if let Some(old) = &previous {
            if old.id == captured.view.id {
                return Err(invalid("Select a different repository location"));
            }
            if self.open_sources_in(&old.id) {
                return Err(invalid(
                    "Close this repository's source tabs before relocating it",
                ));
            }
            if Path::new(&old.path).exists()
                && canonical_root(&old.path)
                    .and_then(|p| inspect(&p, cancel))
                    .is_ok()
            {
                return Err(invalid(
                    "Original repository is still available; add the other copy separately",
                ));
            }
            if old.kind != captured.view.kind {
                return Err(invalid("Repository kinds differ; add it separately"));
            }
        }
        if self.open_sources_in(&captured.view.id) {
            return Err(invalid(
                "Close destination repository source tabs before registering it",
            ));
        }
        let existing = self
            .catalog
            .repositories
            .iter()
            .find(|r| r.id == captured.view.id)
            .cloned();
        if previous.is_none() && existing.is_some() {
            return Err(invalid("Repository is already registered"));
        }
        if previous.is_none()
            && (self.catalog.repositories.len() >= 4096 || self.manual.len() >= 256)
        {
            return Err(invalid("Repository registration limit reached"));
        }
        check_cancel(cancel)?;
        let view = RepositoryRegistration {
            id: crate::unique_id(),
            repository: captured.view.clone(),
            previous,
            existing,
        };
        self.registration = Some(Prepared {
            view: view.clone(),
            captured,
        });
        Ok(view)
    }
    pub fn registration_apply(
        &mut self,
        plan_id: &str,
        base: &Path,
        cancel: &AtomicBool,
    ) -> Result<Catalog> {
        let plan = self
            .registration
            .take()
            .ok_or_else(|| invalid("Repository registration expired; review again"))?;
        if plan.view.id != plan_id {
            return Err(invalid("Repository registration expired; review again"));
        }
        check_cancel(cancel)?;
        let mut repo = inspect(&canonical_root(&plan.view.repository.path)?, cancel)?;
        if repo.root_identity != plan.captured.root_identity
            || repo.git_identity != plan.captured.git_identity
            || repo.git != plan.captured.git
            || repo.common != plan.captured.common
            || repo.view.match_key != plan.view.repository.match_key
        {
            return Err(invalid("Repository location changed after review"));
        }
        if self
            .catalog
            .repositories
            .iter()
            .find(|r| r.id == repo.view.id)
            != plan.view.existing.as_ref()
            || self.open_sources_in(&repo.view.id)
        {
            return Err(invalid("Repository catalogue changed; review again"));
        }
        status::summary(&mut repo, cancel)?;
        let mut next = Workspace {
            catalog: self.catalog.clone(),
            ignored: self.ignored.clone(),
            manual: self.manual.clone(),
            ..Default::default()
        };
        if let Some(existing) = &plan.view.existing {
            next.catalog.repositories.retain(|r| r.id != existing.id);
            repo.view.favorite = existing.favorite;
            repo.view.last_opened = existing.last_opened.clone();
        }
        if let Some(old) = &plan.view.previous {
            if !self.catalog.repositories.contains(old) || self.open_sources_in(&old.id) {
                return Err(invalid("Repository catalogue changed; review again"));
            }
            if Path::new(&old.path).exists()
                && canonical_root(&old.path)
                    .and_then(|p| inspect(&p, cancel))
                    .is_ok()
            {
                return Err(invalid(
                    "Original repository is still available; add the other copy separately",
                ));
            }
            next.catalog.repositories.retain(|r| r.id != old.id);
            next.manual.remove(&old.path);
            next.ignored.insert(old.id.clone());
            if let Some(assignment) = next.catalog.organization.assignments.remove(&old.id) {
                next.catalog
                    .organization
                    .assignments
                    .insert(repo.view.id.clone(), assignment);
            }
            repo.view.favorite |= old.favorite;
            repo.view.last_opened = repo.view.last_opened.max(old.last_opened.clone());
            if let Some(value) = next.catalog.reference_settings.remove(&old.id) {
                next.catalog
                    .reference_settings
                    .insert(repo.view.id.clone(), value);
            }
            if let Some(value) = next.catalog.file_settings.remove(&old.id) {
                next.catalog
                    .file_settings
                    .insert(repo.view.id.clone(), value);
            }
        }
        next.ignored.remove(&repo.view.id);
        next.manual.insert(repo.view.path.clone());
        validate_manual(&next.manual)?;
        next.catalog.repositories.push(repo.view.clone());
        next.catalog
            .repositories
            .sort_by(|a, b| a.name.cmp(&b.name).then(a.path.cmp(&b.path)));
        check_cancel(cancel)?;
        // Publish memory only after the atomic settings replacement succeeds.
        next.save(base)?;
        self.catalog = next.catalog;
        self.ignored = next.ignored;
        self.manual = next.manual;
        if let Some(old) = plan.view.previous {
            self.repositories.remove(&old.id);
        }
        self.repositories.insert(repo.view.id.clone(), repo);
        self.pending = None;
        Ok(self.catalog.clone())
    }
}
