//! Reviewed directory mutation with durable native identity/metadata records.
use super::*;
use crate::native_path;
use clear_contracts::{DirectoryIdentity, NativePath};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Read;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    version: u32,
    relative: NativePath,
    pub identity: DirectoryIdentity,
    pub permissions: FilePermissions,
}
fn identity(path: &Path, metadata: &fs::Metadata) -> Result<DirectoryIdentity> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let _ = path;
        Ok(DirectoryIdentity {
            platform: "unix".into(),
            volume: metadata.dev().to_string(),
            index: metadata.ino().to_string(),
            birth: metadata
                .created()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|t| t.as_nanos().to_string()),
        })
    }
    #[cfg(windows)]
    {
        let _ = metadata;
        let handle = winapi_util::Handle::from_path_any(path)?;
        let info = winapi_util::file::information(&handle)?;
        Ok(DirectoryIdentity {
            platform: "windows".into(),
            volume: info.volume_serial_number().to_string(),
            index: info.file_index().to_string(),
            birth: info.creation_time().map(|t| t.to_string()),
        })
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (path, metadata);
        Err(Error::Invalid(
            "Durable directory identity is unavailable on this OS".into(),
        ))
    }
}
pub(crate) fn observe(root: &Path, relative: &Path) -> Result<Option<Record>> {
    let path = checked_path(root, relative, true)?;
    let metadata = match fs::symlink_metadata(&path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_dir() {
        return Err(Error::Invalid(
            "Expected a real directory; links and other types are refused".into(),
        ));
    }
    let record = Record {
        version: 1,
        relative: native_path::encode(relative),
        identity: identity(&path, &metadata)?,
        permissions: permissions(&metadata),
    };
    validate_identity(&record.identity)?;
    Ok(Some(record))
}
pub(crate) fn empty(path: &Path) -> Result<()> {
    match fs::read_dir(path)?.next() {
        None => Ok(()),
        Some(Err(error)) => Err(error.into()),
        Some(Ok(_)) => Err(Error::Invalid(
            "Directory contains entries and will not be removed".into(),
        )),
    }
}
fn record_path(root: &Path, relative: &Path, created: bool) -> Result<PathBuf> {
    let key = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&native_path::encode(relative))?)
    );
    checked_path(
        root,
        &PathBuf::from(if created {
            "directory-created"
        } else {
            "directory-metadata"
        })
        .join(format!("{key}.json")),
        true,
    )
}
impl Record {
    pub(crate) fn write(&self, root: &Path, relative: &Path, created: bool) -> Result<()> {
        let path = record_path(root, relative, created)?;
        let parent = path
            .parent()
            .ok_or_else(|| Error::Invalid("Directory record parent missing".into()))?;
        fs::create_dir_all(parent)?;
        if path.exists() {
            return Err(Error::Invalid("Directory record already exists".into()));
        }
        atomic_json(&path, self)?;
        flush_ancestors(parent, root)
    }
    pub(crate) fn read(root: &Path, relative: &Path, created: bool) -> Result<Self> {
        let path = record_path(root, relative, created)?;
        if !fs::symlink_metadata(&path)?.is_file() {
            return Err(Error::Invalid(
                "Directory record is not a regular file".into(),
            ));
        }
        let mut data = Vec::new();
        File::open(path)?.take(65537).read_to_end(&mut data)?;
        if data.len() > 65536 {
            return Err(Error::Invalid("Directory record exceeds 64 KiB".into()));
        }
        let record: Self = serde_json::from_slice(&data)?;
        if record.version != 1 || record.relative != native_path::encode(relative) {
            return Err(Error::Invalid(
                "Directory record does not match its native path".into(),
            ));
        }
        validate_identity(&record.identity)?;
        match &record.permissions {
            #[cfg(unix)]
            FilePermissions::Unix { mode } if *mode <= 0o7777 => {}
            #[cfg(windows)]
            FilePermissions::ReadOnly { .. } => {}
            _ => {
                return Err(Error::Invalid(
                    "Directory record permissions belong to another OS".into(),
                ));
            }
        }
        Ok(record)
    }
}
pub(crate) fn validate_identity(identity: &DirectoryIdentity) -> Result<()> {
    if identity.platform != if cfg!(windows) { "windows" } else { "unix" }
        || identity.volume.parse::<u64>().is_err()
        || identity.index.parse::<u64>().is_err()
        || identity.index == "0"
        || identity
            .birth
            .as_ref()
            .is_some_and(|b| b.parse::<u128>().is_err())
    {
        return Err(Error::Invalid(
            "Invalid or unsupported directory identity".into(),
        ));
    }
    Ok(())
}
pub(crate) fn restore_permissions(path: &Path, recorded: &FilePermissions) -> Result<()> {
    match recorded {
        #[cfg(unix)]
        FilePermissions::Unix { mode } if *mode <= 0o7777 => {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(path, fs::Permissions::from_mode(*mode))?;
        }
        #[cfg(windows)]
        FilePermissions::ReadOnly { value } => {
            let mut p = fs::symlink_metadata(path)?.permissions();
            p.set_readonly(*value);
            fs::set_permissions(path, p)?;
        }
        _ => {
            return Err(Error::Invalid(
                "Directory permissions belong to another OS".into(),
            ));
        }
    }
    Ok(())
}
pub(super) fn execute(
    snapshot: &Snapshot,
    plan: &SyncPlan,
    action: &FileAction,
    cancel: &AtomicBool,
) -> Result<()> {
    let _mutation = crate::task::mutation(cancel)?;
    crate::task::stage("directory_mutation");
    roots_unchanged(snapshot)?;
    let relative = snapshot
        .paths
        .get(&action.row_id)
        .ok_or_else(|| Error::Invalid("Unknown directory entry".into()))?;
    if action.relative_native.as_ref() != Some(&native_path::encode(relative))
        || snapshot.rules.excludes(relative, true)
    {
        return Err(Error::Invalid(
            "Directory action differs from its reviewed native path".into(),
        ));
    }
    let root = target_root(snapshot, plan);
    let path = checked_path(root, relative, true)?;
    let parent = path
        .parent()
        .ok_or_else(|| Error::Invalid("Directory parent missing".into()))?;
    if action.operation == FileOperation::CreateDirectory {
        let source = if plan.source_side == Side::Left {
            &snapshot.left
        } else {
            &snapshot.right
        };
        if observe(source, relative)?.is_none() {
            return Err(Error::Invalid("Source directory disappeared".into()));
        }
        if observe(root, relative)?.is_some() {
            return Err(Error::Invalid(
                "Directory creation target already exists".into(),
            ));
        }
        check_cancel(cancel)?;
        fs::create_dir(&path)?;
        flush_directory(&path)?;
        flush_directory(parent)?;
        let record = observe(root, relative)?
            .ok_or_else(|| Error::Invalid("Created directory disappeared".into()))?;
        record.write(Path::new(&plan.backup_root), relative, true)?;
        // Cancellation after mkdir must not hide an applied action.
        if observe(root, relative)?.as_ref() != Some(&record) {
            return Err(Error::Invalid("Created directory identity changed".into()));
        }
    } else {
        super::mirror::source_absent(snapshot, plan, relative)?;
        let record = observe(root, relative)?
            .ok_or_else(|| Error::Invalid("Directory removal target disappeared".into()))?;
        if action.target_permissions.as_ref() != Some(&record.permissions)
            || action.directory_identity.as_ref() != Some(&record.identity)
        {
            return Err(Error::Invalid(
                "Directory permissions changed after preview".into(),
            ));
        }
        empty(&path)?;
        record.write(Path::new(&plan.backup_root), relative, false)?;
        if Record::read(Path::new(&plan.backup_root), relative, false)? != record
            || observe(root, relative)?.as_ref() != Some(&record)
        {
            return Err(Error::Invalid(
                "Directory backup or identity changed".into(),
            ));
        }
        empty(&path)?;
        super::mirror::source_absent(snapshot, plan, relative)?;
        check_cancel(cancel)?;
        fs::remove_dir(&path)?;
        flush_directory(parent)?;
        if observe(root, relative)?.is_some() {
            return Err(Error::Invalid("Removed directory was recreated".into()));
        }
    }
    Ok(())
}
