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

#[cfg(all(test, unix))]
mod tests {
    #[test]
    fn native_bytes_round_trip() {
        use std::os::unix::ffi::OsStringExt;
        let p = std::path::PathBuf::from(std::ffi::OsString::from_vec(vec![b'a', 255]));
        assert_eq!(super::decode(&super::encode(&p)).unwrap(), p);
    }
}
