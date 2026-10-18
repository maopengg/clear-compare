//! Bridge captured workbench references into immutable Git comparison sources.
use super::*;
use clear_contracts::{GitCompareRequest, GitSource};
fn invalid(message: &str) -> Error {
    Error::Invalid(message.into())
}
fn commit(root: &Path, object: &str, cancel: &AtomicBool) -> Result<String> {
    runner::text(
        root,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{object}^{{commit}}"),
        ],
        cancel,
    )
}
impl Workspace {
    pub fn compare_references(
        &mut self,
        repository: &str,
        revision: &str,
        request: &ReferenceComparison,
        cancel: &AtomicBool,
    ) -> Result<crate::git::Snapshot> {
        check_cancel(cancel)?;
        let state = self.open(repository, cancel)?;
        if state.revision != revision {
            return Err(invalid(
                "Repository changed; refresh before comparing references",
            ));
        }
        let repo = self
            .repositories
            .get(repository)
            .ok_or_else(|| invalid("Unknown repository"))?;
        if crate::sync::durable::Object::observe(&repo.root, Path::new(""))? != repo.root_identity
            || crate::sync::durable::Object::observe(&repo.git, Path::new(""))? != repo.git_identity
        {
            return Err(invalid("Repository identity changed; scan again"));
        }
        let left = state
            .references
            .iter()
            .find(|r| r.id == request.left_reference_id)
            .ok_or_else(|| invalid("Reference selection expired; refresh first"))?;
        let left_revision = commit(&repo.root, &left.target, cancel)?;
        let (right_source, right_revision, include_untracked) = match &request.target {
            CompareTarget::Current => (
                GitSource::Commit,
                state
                    .repository
                    .head
                    .clone()
                    .ok_or_else(|| invalid("Current branch has no commit"))?,
                false,
            ),
            CompareTarget::Reference { id } => {
                let right = state
                    .references
                    .iter()
                    .find(|r| r.id == *id)
                    .ok_or_else(|| invalid("Target reference selection expired; refresh first"))?;
                (
                    GitSource::Commit,
                    commit(&repo.root, &right.target, cancel)?,
                    false,
                )
            }
            CompareTarget::WorkingTree { include_untracked } => {
                (GitSource::WorkingTree, String::new(), *include_untracked)
            }
            CompareTarget::Index => (GitSource::Index, String::new(), false),
        };
        if repo.view.kind == "bare" && right_source != GitSource::Commit {
            return Err(invalid("Bare repositories have no working tree or index"));
        }
        let exclusions = self
            .catalog
            .file_settings
            .get(repository)
            .map(|s| s.exclusions.clone())
            .unwrap_or_default();
        crate::git::compare(
            &GitCompareRequest {
                repository: repo
                    .root
                    .to_str()
                    .ok_or_else(|| invalid("Repository path is not valid UTF-8"))?
                    .into(),
                left_source: GitSource::Commit,
                right_source,
                left_revision,
                right_revision,
                include_untracked,
                exclusions,
            },
            cancel,
        )
    }
}
