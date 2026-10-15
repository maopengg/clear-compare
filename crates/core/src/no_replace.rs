//! Native atomic no-replace rename; no copy/unlink or overwriting fallback.
use crate::Result;
use std::path::Path;
pub(crate) fn rename(source: &Path, destination: &Path) -> Result<()> {
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        rustix::fs::renameat_with(
            rustix::fs::CWD,
            source,
            rustix::fs::CWD,
            destination,
            rustix::fs::RenameFlags::NOREPLACE,
        )
        .map_err(std::io::Error::from)?;
        Ok(())
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let old = source
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        let new = destination
            .as_os_str()
            .encode_wide()
            .chain(Some(0))
            .collect::<Vec<_>>();
        if old[..old.len() - 1].contains(&0) || new[..new.len() - 1].contains(&0) {
            return Err(crate::Error::Invalid("Rename path contains NUL".into()));
        }
        // SAFETY: Both buffers are live, NUL-terminated UTF-16 paths. Zero flags
        // intentionally omit REPLACE_EXISTING, COPY_ALLOWED and delayed moves.
        let result = unsafe {
            windows_sys::Win32::Storage::FileSystem::MoveFileExW(old.as_ptr(), new.as_ptr(), 0)
        };
        if result == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        let _ = (source, destination);
        Err(crate::Error::Invalid(
            "Atomic no-replace rename is unavailable on this OS".into(),
        ))
    }
}
