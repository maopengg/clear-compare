//! Scope-owned advisory locks, explicitly released before their handle closes.
use std::fs::{File, TryLockError};

pub(crate) struct FileLock {
    file: File,
}
impl FileLock {
    // Callers pass a freshly opened file, never a duplicate of another guard.
    pub(crate) fn try_new(file: File) -> Result<Self, TryLockError> {
        file.try_lock()?;
        Ok(Self { file })
    }
}
impl Drop for FileLock {
    fn drop(&mut self) {
        // Unix fork/dup shares the open file description. Closing our descriptor
        // alone can leave its lock held by a child until exec closes its copy.
        let _ = self.file.unlock();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn open(path: &std::path::Path) -> File {
        File::options().read(true).write(true).open(path).unwrap()
    }
    #[test]
    fn independent_handles_remain_exclusive_until_owner_drops() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let owner = FileLock::try_new(open(file.path())).unwrap();
        assert!(matches!(
            FileLock::try_new(open(file.path())),
            Err(TryLockError::WouldBlock)
        ));
        drop(owner);
        let next = FileLock::try_new(open(file.path())).unwrap();
        assert!(matches!(
            FileLock::try_new(open(file.path())),
            Err(TryLockError::WouldBlock)
        ));
        drop(next);
    }
    #[cfg(unix)]
    #[test]
    fn copied_open_description_does_not_extend_owner_lock_lifetime() {
        let file = tempfile::NamedTempFile::new().unwrap();
        // Reproduce the former close-only behavior without scheduling a fork.
        let legacy = open(file.path());
        legacy.try_lock().unwrap();
        let legacy_copy = legacy.try_clone().unwrap();
        drop(legacy);
        assert!(matches!(
            open(file.path()).try_lock(),
            Err(TryLockError::WouldBlock)
        ));
        legacy_copy.unlock().unwrap();
        drop(legacy_copy);
        let owner = FileLock::try_new(open(file.path())).unwrap();
        // dup and fork inherit references to the same underlying lock.
        let inherited = owner.file.try_clone().unwrap();
        assert!(matches!(
            FileLock::try_new(open(file.path())),
            Err(TryLockError::WouldBlock)
        ));
        drop(owner);
        let next = FileLock::try_new(open(file.path())).unwrap();
        assert!(matches!(
            FileLock::try_new(open(file.path())),
            Err(TryLockError::WouldBlock)
        ));
        // Closing an old inherited descriptor must not unlock the new owner.
        drop(inherited);
        assert!(matches!(
            FileLock::try_new(open(file.path())),
            Err(TryLockError::WouldBlock)
        ));
        drop(next);
        assert!(FileLock::try_new(open(file.path())).is_ok());
    }
    #[cfg(unix)]
    #[test]
    fn a_child_paused_before_exec_cannot_retain_a_finished_operation_lock() {
        use std::{
            io::{Read, Write},
            os::{
                fd::AsFd,
                unix::{net::UnixStream, process::CommandExt},
            },
            process::{Command, Stdio},
            time::Duration,
        };
        let file = tempfile::NamedTempFile::new().unwrap();
        let owner = FileLock::try_new(open(file.path())).unwrap();
        let (mut parent, child_socket) = UnixStream::pair().unwrap();
        parent
            .set_read_timeout(Some(Duration::from_secs(4)))
            .unwrap();
        child_socket
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut command = Command::new("/bin/sh");
        command
            .args(["-c", "exit 0"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // Only async-signal-safe read/write syscalls execute in the forked child.
        // The socket timeout also bounds the child if an assertion prevents release.
        unsafe {
            command.pre_exec(move || {
                rustix::io::write(child_socket.as_fd(), b"r")?;
                let mut release = [0];
                if rustix::io::read(child_socket.as_fd(), &mut release)? != 1 {
                    return Err(std::io::ErrorKind::UnexpectedEof.into());
                }
                Ok(())
            });
        }
        let spawn = std::thread::spawn(move || command.spawn().unwrap());
        let mut ready = [0];
        parent.read_exact(&mut ready).unwrap();
        assert_eq!(ready, *b"r");
        assert!(matches!(
            FileLock::try_new(open(file.path())),
            Err(TryLockError::WouldBlock)
        ));
        drop(owner);
        let next = FileLock::try_new(open(file.path())).unwrap();
        parent.write_all(b"g").unwrap();
        let mut child = spawn.join().unwrap();
        assert!(child.wait().unwrap().success());
        assert!(matches!(
            FileLock::try_new(open(file.path())),
            Err(TryLockError::WouldBlock)
        ));
        drop(next);
    }
    #[test]
    fn unwind_releases_the_lock_without_removing_the_lock_file() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let result = std::panic::catch_unwind(|| {
            let _owner = FileLock::try_new(open(file.path())).unwrap();
            panic!("synthetic operation failure");
        });
        assert!(result.is_err());
        assert!(file.path().is_file());
        assert!(FileLock::try_new(open(file.path())).is_ok());
    }
}
