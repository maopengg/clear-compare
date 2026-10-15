//! Durable path encoding retains non-Unicode file names without reconstruction from labels.
use crate::{Error, Result};
use clear_contracts::NativePath;
use std::path::{Path, PathBuf};
pub fn encode(path: &Path) -> NativePath {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        NativePath::Unix(path.as_os_str().as_bytes().to_vec())
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        NativePath::Windows(path.as_os_str().encode_wide().collect())
    }
    #[cfg(not(any(unix, windows)))]
    {
        NativePath::Utf8(path.to_string_lossy().into())
    }
}
pub fn decode(path: &NativePath) -> Result<PathBuf> {
    match path {
        NativePath::Utf8(text) => Ok(PathBuf::from(text)),
        #[cfg(unix)]
        NativePath::Unix(bytes) => {
            use std::os::unix::ffi::OsStringExt;
            Ok(PathBuf::from(std::ffi::OsString::from_vec(bytes.clone())))
        }
        #[cfg(windows)]
        NativePath::Windows(units) => {
            use std::os::windows::ffi::OsStringExt;
            Ok(PathBuf::from(std::ffi::OsString::from_wide(units)))
        }
        _ => Err(Error::Invalid(
            "Journal paths belong to another operating system".into(),
        )),
    }
}

#[cfg(test)]
mod tests {
    #[cfg(unix)]
    #[test]
    fn native_bytes_round_trip() {
        use std::os::unix::ffi::OsStringExt;
        let p = std::path::PathBuf::from(std::ffi::OsString::from_vec(vec![b'a', 255]));
        assert_eq!(super::decode(&super::encode(&p)).unwrap(), p);
    }

    #[test]
    fn canonical_spelling_is_present_only_when_the_filesystem_resolves_it() {
        use std::{fs, path::Path, sync::atomic::AtomicBool};
        let temp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(temp.path()).unwrap();
        let stored = "cafe\u{301}.txt";
        let requested = "caf\u{e9}.txt";
        fs::write(root.join(stored), b"content").unwrap();
        let alias = root.join(requested).exists();
        let cancel = AtomicBool::new(false);
        assert_eq!(
            super::exact_leaf(&root, Path::new(requested), &cancel).unwrap(),
            alias
        );
        assert!(!super::exact_leaf(&root, Path::new("CAF\u{e9}.txt"), &cancel).unwrap());
        if !alias {
            fs::write(root.join(requested), b"different file").unwrap();
            assert!(super::exact_leaf(&root, Path::new(requested), &cancel).unwrap());
            fs::remove_file(root.join(requested)).unwrap();
            assert!(!super::exact_leaf(&root, Path::new(requested), &cancel).unwrap());
            assert_eq!(fs::read(root.join(stored)).unwrap(), b"content");
        }
    }
}

#[cfg(any(windows, test))]
pub(crate) fn windows_file_name(name: &str) -> bool {
    if name.is_empty()
        || name.ends_with([' ', '.'])
        || name
            .chars()
            .any(|c| c < ' ' || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'))
    {
        return false;
    }
    let base = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    if matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL") {
        return false;
    }
    let device = base
        .strip_prefix("COM")
        .or_else(|| base.strip_prefix("LPT"));
    !device.is_some_and(|n| {
        matches!(
            n,
            "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "\u{b9}" | "\u{b2}" | "\u{b3}"
        )
    })
}
pub(crate) fn single_name(name: &std::ffi::OsStr) -> Result<()> {
    let path = Path::new(name);
    if name.is_empty()
        || name.len() > 16384
        || path.components().count() != 1
        || !matches!(
            path.components().next(),
            Some(std::path::Component::Normal(_))
        )
        || path.file_name() != Some(name)
    {
        return Err(Error::Invalid(
            "Choose one file name without directory traversal".into(),
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        if name.as_bytes().contains(&0) {
            return Err(Error::Invalid("A file name cannot contain NUL".into()));
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        if name.encode_wide().any(|c| c == 0) || !windows_file_name(&name.to_string_lossy()) {
            return Err(Error::Invalid("Unsupported Windows file name".into()));
        }
    }
    Ok(())
}

pub(crate) fn exact_leaf(
    root: &Path,
    relative: &Path,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<bool> {
    use unicode_normalization::UnicodeNormalization;
    if !std::fs::symlink_metadata(root)?.is_dir() || std::fs::canonicalize(root)? != root {
        return Err(Error::Invalid("Native file root changed".into()));
    }
    let parent = relative
        .parent()
        .ok_or_else(|| Error::Invalid("Rename parent missing".into()))?;
    let directory = crate::compare::checked_path(root, parent, false)?;
    let name = relative
        .file_name()
        .ok_or_else(|| Error::Invalid("Rename name missing".into()))?;
    for (count, entry) in std::fs::read_dir(&directory)?.enumerate() {
        crate::compare::check_cancel(cancel)?;
        if count >= crate::compare::MAX_ENTRIES {
            return Err(Error::Invalid(
                "Rename directory enumeration exceeds 200000 entries".into(),
            ));
        }
        let entry = entry?;
        let actual = entry.file_name();
        if actual == name {
            return Ok(true);
        }
        // Some filesystems store a canonically decomposed spelling. Accept it
        // only if the OS resolves the requested spelling to that same regular
        // file. NFC equality is case-sensitive, so an external case-only rename
        // still invalidates an editor's old location. Native paths stay intact.
        if let (Some(wanted), Some(stored)) = (name.to_str(), actual.to_str())
            && wanted.nfc().eq(stored.nfc())
        {
            let requested = directory.join(name);
            let metadata = match std::fs::symlink_metadata(&requested) {
                Ok(metadata) => metadata,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            if metadata.is_file()
                && entry.file_type()?.is_file()
                && same_file::is_same_file(&requested, entry.path())?
            {
                return Ok(true);
            }
        }
    }
    Ok(false)
}
