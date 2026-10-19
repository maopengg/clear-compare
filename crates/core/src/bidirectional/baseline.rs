use super::*;
pub(super) fn canonical(path: &Path, missing: bool) -> Result<PathBuf> {
    let parent = fs::canonicalize(
        path.parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new(".")),
    )?;
    let name = path
        .file_name()
        .ok_or_else(|| invalid("Baseline file name missing"))?;
    crate::compare::checked_path(&parent, Path::new(name), missing)
}
pub(super) fn outside(path: &Path, snapshot: &Snapshot) -> Result<()> {
    if path.starts_with(&snapshot.left) || path.starts_with(&snapshot.right) {
        return Err(invalid(
            "Baseline must be outside both compared directories",
        ));
    }
    Ok(())
}
pub(super) fn lock(path: &Path, create: bool) -> Result<crate::file_lock::FileLock> {
    let mut name = path
        .file_name()
        .ok_or_else(|| invalid("Baseline lock name missing"))?
        .to_os_string();
    name.push(".lock");
    let file = crate::compare::checked_path(
        path.parent()
            .ok_or_else(|| invalid("Baseline lock parent missing"))?,
        Path::new(&name),
        create,
    )?;
    let mut options = File::options();
    options
        .read(true)
        .write(true)
        .create(create)
        .truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let handle = options.open(file)?;
    crate::file_lock::FileLock::try_new(handle)
        .map_err(|e| invalid(format!("Baseline is in use or cannot be locked: {e}")))
}
pub(super) fn read(path: &Path) -> Result<(Baseline, String)> {
    let (data, hash): (Baseline, String) = durable::read(path, MAX_BASELINE)?;
    if data.version != 1 || data.revision == 0 {
        return Err(invalid("Unsupported baseline version or revision"));
    }
    durable::namespace(&data.id)?;
    inventory(&data.entries)?;
    Ok((data, hash))
}
pub(super) fn info(path: &Path, baseline: &Baseline) -> BaselineInfo {
    BaselineInfo {
        path: path.to_string_lossy().into(),
        id: baseline.id.clone(),
        revision: baseline.revision.to_string(),
        entries: baseline.entries.len(),
    }
}
pub fn inspect(snapshot: &Snapshot, path: &Path, cancel: &AtomicBool) -> Result<BaselineInfo> {
    check_cancel(cancel)?;
    let path = canonical(path, false)?;
    outside(&path, snapshot)?;
    let _lock = lock(&path, false)?;
    let (base, _) = read(&path)?;
    roots(&base, snapshot)?;
    Ok(info(&path, &base))
}
pub fn capture(
    snapshot: &Snapshot,
    path: &Path,
    apply: bool,
    cancel: &AtomicBool,
) -> Result<BaselineInfo> {
    let path = canonical(path, true)?;
    outside(&path, snapshot)?;
    if path.exists() {
        return Err(invalid("Baseline already exists; select a new file"));
    }
    let left = side(snapshot, true)?;
    let right = side(snapshot, false)?;
    if left != right {
        return Err(invalid(
            "Initial baseline requires equal verified trees, including empty directories. Review a directional sync first.",
        ));
    }
    inventory(&store(&left))?;
    let base = Baseline {
        version: 1,
        id: unique_id(),
        revision: 1,
        last_run: None,
        left: native_path::encode(&snapshot.left),
        right: native_path::encode(&snapshot.right),
        left_root: durable::Object::observe(&snapshot.left, Path::new(""))?
            .ok_or_else(|| invalid("Left root unavailable"))?,
        right_root: durable::Object::observe(&snapshot.right, Path::new(""))?
            .ok_or_else(|| invalid("Right root unavailable"))?,
        exclusions: policy(snapshot)?,
        entries: store(&left),
    };
    roots(&base, snapshot)?;
    let _guard = crate::task::validated_mutation(cancel, || {
        roots(&base, snapshot)?;
        sync::revalidate_scope(snapshot, cancel)
    })?;
    if apply {
        let _lock = lock(&path, true)?;
        durable::write(&path, &base, MAX_BASELINE)?;
    }
    Ok(info(&path, &base))
}

pub(super) fn replace(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| invalid("Baseline parent missing"))?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.write_all(bytes)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|e| e.error)?;
    sync::flush_directory(parent)?;
    if durable::digest(&durable::bytes(path, MAX_BASELINE)?) != durable::digest(bytes) {
        return Err(invalid("Baseline replacement verification failed"));
    }
    Ok(())
}
