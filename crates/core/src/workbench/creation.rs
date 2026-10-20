//! Isolated repository creation, no-replace publication and persistent registration.
use super::*;
use crate::sync::durable::Object;
pub(super) struct Prepared {
    view: RepositoryCreationPlan,
    parent: PathBuf,
    identity: Option<Object>,
    local: Option<Captured>,
}
fn invalid(text: &str) -> Error {
    Error::Invalid(text.into())
}
fn component(name: &str) -> Result<()> {
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_uppercase();
    if name.is_empty()
        || name.len() > 200
        || name.starts_with('-')
        || name == "."
        || name == ".."
        || name.eq_ignore_ascii_case(".git")
        || name.ends_with(['.', ' '])
        || name
            .chars()
            .any(|c| c.is_control() || "/\\:*?\"<>|".contains(c))
        || [
            "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
            "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
        ]
        .contains(&stem.as_str())
    {
        return Err(invalid("Use a single portable repository folder name"));
    }
    Ok(())
}
fn branch(parent: &Path, value: &str, cancel: &AtomicBool) -> Result<()> {
    if value.is_empty()
        || value.len() > 512
        || value.starts_with('-')
        || value.chars().any(char::is_control)
    {
        return Err(invalid("Invalid initial branch or clone reference"));
    }
    runner::run(
        parent,
        &["check-ref-format", &format!("refs/heads/{value}")],
        cancel,
    )?;
    Ok(())
}
fn source(url: &str, cancel: &AtomicBool) -> Result<(String, Option<Captured>)> {
    if url.is_empty()
        || url.len() > 4096
        || url.starts_with('-')
        || url.chars().any(char::is_control)
        || url.contains(['?', '#'])
    {
        return Err(invalid(
            "Use a Git URL without embedded credentials, query or fragment",
        ));
    }
    if Path::new(url).is_absolute() {
        let root = canonical_root(url)?;
        let captured = inspect(&root, cancel)?;
        return Ok((runner::path_arg(&root)?, Some(captured)));
    }
    if let Some((scheme, rest)) = url.split_once("://") {
        if !["https", "http", "ssh", "git"].contains(&scheme) {
            return Err(invalid("Unsupported clone URL protocol"));
        }
        let authority = rest.split('/').next().unwrap_or_default();
        if authority.is_empty()
            || rest.len() == authority.len()
            || ((scheme != "ssh") && authority.contains('@'))
            || authority
                .split_once('@')
                .is_some_and(|(user, _)| user.contains(':'))
        {
            return Err(invalid(
                "Use a Git URL without embedded credentials, query or fragment",
            ));
        }
    } else if let Some((host, path)) = url.split_once(':') {
        if host.is_empty()
            || host.contains(['/', '\\', ' '])
            || path.is_empty()
            || path.starts_with(':')
            || host
                .split_once('@')
                .is_some_and(|(user, _)| user.is_empty())
        {
            return Err(invalid("Unsupported clone URL protocol"));
        }
    } else {
        return Err(invalid("Use an absolute local source path or Git URL"));
    }
    Ok((url.into(), None))
}
impl Prepared {
    fn check(&self, cancel: &AtomicBool) -> Result<()> {
        check_cancel(cancel)?;
        if fs::canonicalize(&self.parent)? != self.parent
            || Object::observe(&self.parent, Path::new(""))? != self.identity
        {
            return Err(invalid("Repository parent changed after review"));
        }
        if fs::symlink_metadata(&self.view.destination).is_ok() {
            return Err(invalid("Repository destination already exists"));
        }
        if let Some(old) = &self.local {
            let current = inspect(&canonical_root(&runner::path_arg(&old.root)?)?, cancel)?;
            if current.root_identity != old.root_identity
                || current.git_identity != old.git_identity
                || current.git != old.git
                || current.common != old.common
            {
                return Err(invalid("Local clone source changed after review"));
            }
        }
        Ok(())
    }
}
impl Workspace {
    pub fn creation_prepare(
        &mut self,
        parent: &str,
        name: &str,
        mut operation: RepositoryCreation,
        cancel: &AtomicBool,
    ) -> Result<RepositoryCreationPlan> {
        self.creation = None;
        component(name)?;
        if parent.len() > 8192 || parent.contains('\0') || !Path::new(parent).is_absolute() {
            return Err(invalid("Select an absolute repository parent directory"));
        }
        let parent = canonical_root(parent)?;
        if self
            .repositories
            .values()
            .any(|r| parent.starts_with(&r.git) || parent.starts_with(&r.common))
            || parent
                .components()
                .any(|c| c.as_os_str().eq_ignore_ascii_case(".git"))
            || runner::text(&parent, &["rev-parse", "--is-inside-git-dir"], cancel)
                .is_ok_and(|v| v == "true")
        {
            return Err(invalid(
                "Repositories cannot be created inside Git metadata",
            ));
        }
        if self.manual.len() >= 256 || self.catalog.repositories.len() >= 4096 {
            return Err(invalid("Repository registration limit reached"));
        }
        let mut local = None;
        match &mut operation {
            RepositoryCreation::Init { branch: value, .. } => branch(&parent, value, cancel)?,
            RepositoryCreation::Clone {
                url,
                branch: reference,
                depth,
                ..
            } => {
                let (value, pin) = source(url, cancel)?;
                *url = value;
                local = pin;
                if let Some(value) = reference {
                    branch(&parent, value, cancel)?;
                }
                if depth.is_some_and(|n| n == 0 || n > 1_000_000) {
                    return Err(invalid("Clone depth must be 1–1000000"));
                }
            }
        }
        let destination = parent.join(name);
        let identity = Object::observe(&parent, Path::new(""))?;
        let view = RepositoryCreationPlan {
            id: crate::unique_id(),
            destination: runner::path_arg(&destination)?,
            operation,
        };
        let plan = Prepared {
            view: view.clone(),
            parent,
            identity,
            local,
        };
        plan.check(cancel)?;
        self.creation = Some(plan);
        Ok(view)
    }
    pub fn creation_apply(
        &mut self,
        id: &str,
        base: &Path,
        cancel: &AtomicBool,
    ) -> Result<RepositoryCreationResult> {
        let plan = self
            .creation
            .take()
            .ok_or_else(|| invalid("Repository creation expired; review again"))?;
        if plan.view.id != id {
            return Err(invalid("Repository creation expired; review again"));
        }
        plan.check(cancel)?;
        fs::create_dir_all(base)?;
        let journal_root = base.join("git-creation");
        super::remotes::directory(&journal_root)?;
        let journal = journal_root.join(&plan.view.id);
        super::remotes::directory(&journal)?;
        super::remotes::write(
            &journal.join("intent.json"),
            &serde_json::to_vec(&plan.view)?,
        )?;
        let staging = tempfile::Builder::new()
            .prefix(".clear-compare-create-")
            .tempdir_in(&plan.parent)?
            .keep();
        let stage_pin = Object::observe(&staging, Path::new(""))?;
        let mut result = RepositoryCreationResult {
            status: "failed".into(),
            path: staging.to_string_lossy().into(),
            journal: journal.to_string_lossy().into(),
            output: String::new(),
            issue: None,
            catalog: None,
        };
        let attempt = (|| -> Result<()> {
            super::remotes::write(
                &journal.join("staging.json"),
                &serde_json::to_vec(
                    &serde_json::json!({"path":crate::native_path::encode(&staging),"identity":stage_pin}),
                )?,
            )?;
            plan.check(cancel)?;
            let mut args: Vec<String> = vec![];
            match &plan.view.operation {
                RepositoryCreation::Init { branch, bare } => {
                    args.extend(["init".into(), format!("--initial-branch={branch}")]);
                    if *bare {
                        args.push("--bare".into());
                    }
                    args.extend(["--".into(), ".".into()]);
                }
                RepositoryCreation::Clone {
                    url,
                    branch,
                    bare,
                    depth,
                } => {
                    args.extend(["clone".into(), "--no-local".into(), "--no-progress".into()]);
                    if *bare {
                        args.push("--bare".into());
                    }
                    if let Some(value) = branch {
                        args.extend(["--branch".into(), value.clone()]);
                    }
                    if let Some(depth) = depth {
                        args.extend(["--depth".into(), depth.to_string()]);
                    }
                    args.extend(["--".into(), url.clone(), ".".into()]);
                }
            }
            let output = runner::mutate(
                &staging,
                &args,
                cancel,
                matches!(plan.view.operation, RepositoryCreation::Clone { .. }),
            )?;
            result.output = runner::redact(&String::from_utf8_lossy(&output));
            plan.check(cancel)?;
            if Object::observe(&staging, Path::new(""))? != stage_pin {
                return Err(invalid("Repository staging directory changed"));
            }
            inspect(&staging, cancel)?;
            crate::no_replace::rename(&staging, Path::new(&plan.view.destination))?;
            result.path = plan.view.destination.clone();
            result.status = "created_unregistered".into();
            #[cfg(unix)]
            fs::File::open(&plan.parent)?.sync_all()?;
            let registration = self.registration_prepare(&result.path, None, cancel)?;
            let catalog = self.registration_apply(&registration.id, base, cancel)?;
            result.catalog = Some(catalog);
            result.status = "registered".into();
            Ok(())
        })();
        if let Err(error) = attempt {
            if matches!(error, Error::Cancelled) && result.status != "created_unregistered" {
                result.status = "cancelled".into();
            }
            result.issue = Some(runner::redact(&error.to_string()));
        }
        if let Err(error) =
            super::remotes::write(&journal.join("result.json"), &serde_json::to_vec(&result)?)
        {
            let issue = format!(
                "{}; creation journal: {error}",
                result.issue.take().unwrap_or_default()
            );
            result.issue = Some(issue);
        }
        Ok(result)
    }
}
