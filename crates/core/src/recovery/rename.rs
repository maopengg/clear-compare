//! Reconcile and reverse native staged renames without overwriting any name.
use super::*;
use crate::{
    no_replace,
    sync::rename::{absent, exact_exists, validated_paths},
};
use clear_contracts::{FileAction, FilePermissions, SyncPlan};
#[derive(Clone, Copy, PartialEq, Eq)]
enum Live {
    Destination,
    Temporary,
}
pub(super) struct Undo {
    original: PathBuf,
    destination: PathBuf,
    temporary: PathBuf,
    live: Live,
    digest: String,
    permissions: FilePermissions,
}
impl Undo {
    fn observed(&self, root: &Path, relative: &Path, cancel: &AtomicBool) -> Result<bool> {
        if !exact_exists(root, relative, cancel)? {
            return Ok(false);
        }
        let path = checked_path(root, relative, false)?;
        if current(&path, cancel)?.as_deref() != Some(&self.digest)
            || permissions(&fs::symlink_metadata(&path)?) != self.permissions
        {
            return Err(Error::Invalid(
                "Rename recovery found changed contents or permissions".into(),
            ));
        }
        Ok(true)
    }
    fn live_relative(&self) -> &Path {
        match self.live {
            Live::Destination => &self.destination,
            Live::Temporary => &self.temporary,
        }
    }
    fn state(&self, root: &Path, cancel: &AtomicBool) -> Result<Live> {
        if self.observed(root, &self.original, cancel)? {
            return Err(Error::Invalid(
                "Original rename path has been recreated; it will not be overwritten".into(),
            ));
        }
        let live = if self.observed(root, &self.temporary, cancel)? {
            Live::Temporary
        } else if self.observed(root, &self.destination, cancel)? {
            Live::Destination
        } else {
            return Err(Error::Invalid(
                "Rename recovery needs a matching staged or destination file".into(),
            ));
        };
        let original = checked_path(root, &self.original, true)?;
        match fs::symlink_metadata(&original) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
            Ok(_)
                if live == Live::Destination
                    && same_file::is_same_file(
                        &original,
                        checked_path(root, &self.destination, false)?,
                    )? => {}
            Ok(_) => {
                return Err(Error::Invalid(
                    "An existing name aliases the original rename path".into(),
                ));
            }
        }
        Ok(live)
    }
    fn backup(&self, root: &Path, old_root: &Path, cancel: &AtomicBool) -> Result<()> {
        root_valid(old_root)?;
        let path = checked_path(
            old_root,
            &PathBuf::from("files").join(&self.original),
            false,
        )?;
        if hash_file(&path, cancel)? != self.digest
            || permissions(&fs::symlink_metadata(&path)?) != self.permissions
        {
            return Err(Error::Invalid("Original rename backup changed".into()));
        }
        root_valid(root)
    }
    pub(super) fn affected(&self, root: &Path) -> Vec<PathBuf> {
        vec![
            root.join(&self.original),
            root.join(&self.destination),
            root.join(&self.temporary),
        ]
    }
    pub(super) fn relocations(&self, root: &Path) -> Vec<crate::editing::Relocation> {
        [&self.destination, &self.temporary]
            .into_iter()
            .map(|from| crate::editing::Relocation {
                root: root.to_owned(),
                from: from.clone(),
                to: self.original.clone(),
            })
            .collect()
    }
    pub(super) fn label(&self) -> String {
        format!(
            "{} → {}",
            crate::tree::display_path(self.live_relative()),
            crate::tree::display_path(&self.original)
        )
    }
    pub(super) fn execute(
        &self,
        root: &Path,
        old_root: &Path,
        backup_root: &Path,
        cancel: &AtomicBool,
    ) -> Result<()> {
        root_valid(root)?;
        self.backup(root, old_root, cancel)?;
        if self.state(root, cancel)? != self.live {
            return Err(Error::Invalid(
                "Rename state changed after recovery preview".into(),
            ));
        }
        let source = checked_path(root, self.live_relative(), false)?;
        let backup = checked_path(
            backup_root,
            &PathBuf::from("files").join(self.live_relative()),
            true,
        )?;
        fs::create_dir_all(
            backup
                .parent()
                .ok_or_else(|| Error::Invalid("Invalid rename recovery backup".into()))?,
        )?;
        let mut writer = File::create_new(&backup)?;
        copy_to(&source, &mut writer, cancel)?;
        set_permissions(&writer, &self.permissions)?;
        writer.sync_all()?;
        drop(writer);
        if hash_file(&backup, cancel)? != self.digest {
            return Err(Error::Invalid(
                "Rename recovery backup verification failed".into(),
            ));
        }
        flush_ancestors(
            backup
                .parent()
                .ok_or_else(|| Error::Invalid("Invalid rename recovery backup parent".into()))?,
            backup_root,
        )?;
        let _mutation = crate::task::mutation(cancel)?;
        crate::task::stage("revalidate_and_reverse_rename");
        root_valid(root)?;
        self.backup(root, old_root, cancel)?;
        if self.state(root, cancel)? != self.live {
            return Err(Error::Invalid(
                "Rename state changed during recovery backup".into(),
            ));
        }
        let stage = checked_path(root, &self.temporary, true)?;
        if self.live == Live::Destination {
            absent(&stage)?;
            no_replace::rename(&source, &stage)?;
            flush_directory(
                stage
                    .parent()
                    .ok_or_else(|| Error::Invalid("Rename parent missing".into()))?,
            )?;
        }
        check_cancel(cancel)?;
        root_valid(root)?;
        if self.state(root, cancel)? != Live::Temporary {
            return Err(Error::Invalid("Staged rename recovery changed".into()));
        }
        let original = checked_path(root, &self.original, true)?;
        absent(&original)?;
        no_replace::rename(&stage, &original)?;
        flush_directory(
            original
                .parent()
                .ok_or_else(|| Error::Invalid("Rename parent missing".into()))?,
        )?;
        let settled = AtomicBool::new(false);
        if !self.observed(root, &self.original, &settled)?
            || exact_exists(root, &self.temporary, &settled)?
            || (self.live == Live::Destination && exact_exists(root, &self.destination, &settled)?)
        {
            return Err(Error::Invalid(
                "Recovered rename could not be verified".into(),
            ));
        }
        Ok(())
    }
}
pub(super) fn preview(
    root: &Path,
    old_root: &Path,
    plan: &SyncPlan,
    action: &FileAction,
    state: &str,
    cancel: &AtomicBool,
) -> Result<Option<Undo>> {
    let (original, destination, temporary) = validated_paths(plan, action)?;
    let mut undo = Undo {
        original,
        destination,
        temporary,
        live: Live::Destination,
        digest: action
            .target_digest
            .clone()
            .ok_or_else(|| Error::Invalid("Rename digest missing".into()))?,
        permissions: action
            .target_permissions
            .clone()
            .ok_or_else(|| Error::Invalid("Rename permissions missing".into()))?,
    };
    if state != "completed" && undo.observed(root, &undo.original, cancel)? {
        return Ok(None);
    }
    undo.live = undo.state(root, cancel)?;
    undo.backup(root, old_root, cancel)?;
    Ok(Some(undo))
}
