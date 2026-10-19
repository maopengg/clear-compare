//! Own the command and its helpers until shutdown, including error/unwind paths.
use std::{
    io,
    process::{Child, Command, ExitStatus},
};

pub(super) struct ManagedChild {
    child: Child,
    scope: platform::Scope,
    finished: bool,
}
impl ManagedChild {
    pub(super) fn spawn(command: &mut Command) -> io::Result<Self> {
        let (child, scope) = platform::spawn(command)?;
        Ok(Self {
            child,
            scope,
            finished: false,
        })
    }
    pub(super) fn complete(&mut self) -> io::Result<bool> {
        self.scope.complete(&mut self.child)
    }
    pub(super) fn finish(&mut self) -> io::Result<ExitStatus> {
        self.scope.stop(&mut self.child)?;
        let status = self.child.wait()?;
        self.finished = true;
        Ok(status)
    }
}
impl Drop for ManagedChild {
    fn drop(&mut self) {
        if !self.finished {
            let _ = self.finish();
        }
    }
}

#[cfg(unix)]
mod platform {
    use super::*;
    use rustix::{
        io::Errno,
        process::{Pid, Signal, WaitId, WaitIdOptions, kill_process_group, waitid},
    };
    use std::{
        os::unix::process::CommandExt,
        process::Stdio,
        thread,
        time::{Duration, Instant},
    };
    pub(super) struct Scope {
        pid: Pid,
        owned: bool,
    }
    pub(super) fn spawn(command: &mut Command) -> io::Result<(Child, Scope)> {
        command.process_group(0);
        let mut child = command.spawn()?;
        let Some(pid) = i32::try_from(child.id())
            .ok()
            .filter(|id| *id > 1)
            .and_then(Pid::from_raw)
        else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::other("Invalid Git process group"));
        };
        Ok((child, Scope { pid, owned: true }))
    }
    impl Scope {
        pub(super) fn complete(&mut self, _: &mut Child) -> io::Result<bool> {
            if !self.owned {
                return Err(io::Error::other("Git child ownership was lost"));
            }
            loop {
                // WNOWAIT pins the PID even after exit. Never signal a recycled group ID.
                match waitid(
                    WaitId::Pid(self.pid),
                    WaitIdOptions::EXITED | WaitIdOptions::NOHANG | WaitIdOptions::NOWAIT,
                ) {
                    Ok(status) => return Ok(status.is_some()),
                    Err(Errno::INTR) => continue,
                    Err(Errno::CHILD) => {
                        self.owned = false;
                        return Err(io::Error::other("Git child was reaped outside its owner"));
                    }
                    Err(e) => return Err(e.into()),
                }
            }
        }
        pub(super) fn stop(&mut self, child: &mut Child) -> io::Result<()> {
            self.complete(child)?;
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                match kill_process_group(self.pid, Signal::KILL) {
                    Ok(()) | Err(Errno::SRCH) => (),
                    Err(e) => {
                        // macOS can reject signals to protected executable zombies.
                        // Accept only a separately verified quiescent group.
                        if !live_members(self.pid)? {
                            return Ok(());
                        }
                        return Err(e.into());
                    }
                }
                // Zombies cannot execute or fork. Keep the root unreaped until every
                // member is stopped; repeated group kills also cover concurrent forks.
                if !live_members(self.pid)? {
                    return Ok(());
                }
                if Instant::now() >= deadline {
                    return Err(io::Error::other(
                        "Git helper shutdown could not be confirmed",
                    ));
                }
                thread::sleep(Duration::from_millis(2));
            }
        }
    }
    fn live_members(pid: Pid) -> io::Result<bool> {
        let mut child = Command::new("/bin/ps")
            .args(["-axo", "pgid=,stat="])
            .env("LC_ALL", "C")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        // Drain concurrently with ps so large process tables cannot fill the pipe.
        let output = child.stdout.take().expect("piped stdout");
        let reader = thread::spawn(move || {
            use std::io::Read;
            let mut bytes = Vec::new();
            output
                .take(8 * 1024 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map(|_| bytes)
        });
        let start = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break status;
            }
            if start.elapsed() > Duration::from_secs(1) {
                let _ = child.kill();
                let _ = child.wait();
                let _ = reader.join();
                return Err(io::Error::other(
                    "Cannot inspect Git process group before shutdown",
                ));
            }
            thread::sleep(Duration::from_millis(1));
        };
        let bytes = reader
            .join()
            .map_err(|_| io::Error::other("Process inspection failed"))??;
        if !status.success() || bytes.len() > 8 * 1024 * 1024 {
            return Err(io::Error::other(
                "Cannot inspect Git process group before shutdown",
            ));
        }
        let expected = pid.as_raw_nonzero().get().to_string();
        for line in String::from_utf8_lossy(&bytes).lines() {
            let mut fields = line.split_whitespace();
            let (Some(group), Some(state)) = (fields.next(), fields.next()) else {
                continue;
            };
            if group == expected && !state.starts_with(['Z', 'X']) {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::{
        mem::{size_of, zeroed},
        os::windows::{io::AsRawHandle, process::CommandExt},
        ptr, thread,
        time::{Duration, Instant},
    };
    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::*,
            JobObjects::*,
            Threading::{CREATE_SUSPENDED, OpenThread, ResumeThread, THREAD_SUSPEND_RESUME},
        },
    };
    pub(super) struct Scope {
        job: HANDLE,
    }
    impl Drop for Scope {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.job);
            }
        }
    }
    pub(super) fn spawn(command: &mut Command) -> io::Result<(Child, Scope)> {
        unsafe {
            let job = CreateJobObjectW(ptr::null(), ptr::null());
            if job.is_null() {
                return Err(io::Error::last_os_error());
            }
            let scope = Scope { job };
            let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = zeroed();
            limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            // Assign while suspended: a helper cannot escape between spawn and assignment.
            command.creation_flags(CREATE_SUSPENDED);
            let mut child = command.spawn()?;
            let setup = (|| {
                if AssignProcessToJobObject(job, child.as_raw_handle()) == 0 {
                    return Err(io::Error::last_os_error());
                }
                resume(child.id())
            })();
            if let Err(error) = setup {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error);
            }
            Ok((child, scope))
        }
    }
    unsafe fn resume(pid: u32) -> io::Result<()> {
        unsafe {
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }
            let result = (|| {
                let mut entry: THREADENTRY32 = zeroed();
                entry.dwSize = size_of::<THREADENTRY32>() as u32;
                let mut found = Thread32First(snapshot, &mut entry);
                while found != 0 {
                    if entry.th32OwnerProcessID == pid {
                        let handle = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                        if handle.is_null() {
                            return Err(io::Error::last_os_error());
                        }
                        let resumed = ResumeThread(handle);
                        let error = io::Error::last_os_error();
                        CloseHandle(handle);
                        return if resumed == u32::MAX {
                            Err(error)
                        } else {
                            Ok(())
                        };
                    }
                    found = Thread32Next(snapshot, &mut entry);
                }
                Err(io::Error::other("Cannot locate suspended Git thread"))
            })();
            CloseHandle(snapshot);
            result
        }
    }
    impl Scope {
        pub(super) fn complete(&mut self, child: &mut Child) -> io::Result<bool> {
            Ok(child.try_wait()?.is_some())
        }
        pub(super) fn stop(&mut self, _: &mut Child) -> io::Result<()> {
            unsafe {
                if TerminateJobObject(self.job, 1) == 0 {
                    return Err(io::Error::last_os_error());
                }
                let start = Instant::now();
                loop {
                    let mut info: JOBOBJECT_BASIC_ACCOUNTING_INFORMATION = zeroed();
                    if QueryInformationJobObject(
                        self.job,
                        JobObjectBasicAccountingInformation,
                        (&mut info as *mut JOBOBJECT_BASIC_ACCOUNTING_INFORMATION).cast(),
                        size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                        ptr::null_mut(),
                    ) == 0
                    {
                        return Err(io::Error::last_os_error());
                    }
                    if info.ActiveProcesses == 0 {
                        return Ok(());
                    }
                    if start.elapsed() > Duration::from_secs(5) {
                        return Err(io::Error::other(
                            "Git helper shutdown could not be confirmed",
                        ));
                    }
                    thread::sleep(Duration::from_millis(2));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        path::Path,
        process::Stdio,
        thread,
        time::{Duration, Instant},
    };
    const HELPER: &str = "workbench::runner::process::tests::helper";
    fn command(dir: &Path, mode: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args(["--exact", HELPER, "--nocapture"])
            .env("CLEAR_PROCESS_TEST_DIR", dir)
            .env("CLEAR_PROCESS_TEST_MODE", mode)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    }
    fn wait_ready(dir: &Path) {
        let start = Instant::now();
        while !dir.join("ready").exists() {
            assert!(
                start.elapsed() < Duration::from_secs(4),
                "helper did not start"
            );
            thread::sleep(Duration::from_millis(2));
        }
    }
    #[test]
    fn helper() {
        let Some(dir) = std::env::var_os("CLEAR_PROCESS_TEST_DIR").map(std::path::PathBuf::from)
        else {
            return;
        };
        let mode = std::env::var("CLEAR_PROCESS_TEST_MODE").unwrap();
        if mode == "leaf" {
            fs::write(dir.join("heartbeat"), b"0").unwrap();
            fs::write(dir.join("ready"), b"ready").unwrap();
            let start = Instant::now();
            for count in 1..3000 {
                fs::write(dir.join("heartbeat"), count.to_string()).unwrap();
                if start.elapsed() > Duration::from_secs(5) {
                    break;
                }
                thread::sleep(Duration::from_millis(2));
            }
        } else {
            let mut leaf = command(&dir, if mode == "nested" { "middle" } else { "leaf" })
                .spawn()
                .unwrap();
            wait_ready(&dir);
            fs::write(dir.join("root-ready"), b"ready").unwrap();
            if mode == "success" {
                std::process::exit(0);
            }
            if mode == "failure" {
                std::process::exit(17);
            }
            if mode == "stderr" {
                eprintln!("{}", "x".repeat(8192));
            }
            if mode == "output" {
                println!("{}", "x".repeat(8192));
            }
            let _ = leaf.wait();
        }
    }
    fn started(mode: &str) -> (tempfile::TempDir, ManagedChild) {
        let dir = tempfile::tempdir().unwrap();
        let child = ManagedChild::spawn(&mut command(dir.path(), mode)).unwrap();
        wait_ready(dir.path());
        let start = Instant::now();
        while !dir.path().join("root-ready").exists() {
            assert!(start.elapsed() < Duration::from_secs(4));
            thread::sleep(Duration::from_millis(2));
        }
        (dir, child)
    }
    fn assert_stopped(dir: &Path) {
        let before = fs::read(dir.join("heartbeat")).unwrap();
        thread::sleep(Duration::from_millis(50));
        assert_eq!(
            before,
            fs::read(dir.join("heartbeat")).unwrap(),
            "helper kept writing after cleanup"
        );
    }
    #[test]
    fn running_cancel_stops_helpers_and_preserves_unrelated_process() {
        let (dir, mut child) = started("nested");
        let (other_dir, mut unrelated) = started("running");
        let cancel = std::sync::atomic::AtomicBool::new(false);
        cancel.store(true, std::sync::atomic::Ordering::Relaxed);
        let out = tempfile::tempfile().unwrap();
        let err = tempfile::tempfile().unwrap();
        let result = super::super::monitor(
            &mut child,
            &cancel,
            &out,
            &err,
            Duration::from_secs(2),
            1024,
            1024,
        );
        assert!(matches!(result, Err(crate::Error::Cancelled)));
        child.finish().unwrap();
        assert_stopped(dir.path());
        assert!(!unrelated.complete().unwrap());
        unrelated.finish().unwrap();
        assert_stopped(other_dir.path());
    }
    #[test]
    fn successful_and_failed_roots_stop_remaining_helpers_without_losing_exit_status() {
        for (mode, expected) in [("success", 0), ("failure", 17)] {
            let (dir, mut child) = started(mode);
            let start = Instant::now();
            while !child.complete().unwrap() {
                assert!(start.elapsed() < Duration::from_secs(3));
                thread::sleep(Duration::from_millis(2));
            }
            assert_eq!(child.finish().unwrap().code(), Some(expected));
            assert_stopped(dir.path());
        }
    }
    #[test]
    fn drop_cleans_up_on_early_return_and_unwind() {
        let (dir, child) = started("running");
        drop(child);
        assert_stopped(dir.path());
        let (dir, child) = started("running");
        let result = std::panic::catch_unwind(move || {
            let _child = child;
            panic!("synthetic failure");
        });
        assert!(result.is_err());
        assert_stopped(dir.path());
    }
    #[test]
    fn timeout_and_output_limit_stop_running_helpers() {
        for mode in ["running", "output", "stderr"] {
            let dir = tempfile::tempdir().unwrap();
            let out = tempfile::tempfile().unwrap();
            let err = tempfile::tempfile().unwrap();
            let mut command = command(dir.path(), mode);
            command
                .stdout(Stdio::from(out.try_clone().unwrap()))
                .stderr(Stdio::from(err.try_clone().unwrap()));
            let mut child = ManagedChild::spawn(&mut command).unwrap();
            wait_ready(dir.path());
            let cancel = std::sync::atomic::AtomicBool::new(false);
            let result = super::super::monitor(
                &mut child,
                &cancel,
                &out,
                &err,
                if mode == "running" {
                    Duration::from_millis(20)
                } else {
                    Duration::from_secs(2)
                },
                1024,
                1024,
            );
            assert!(
                result
                    .unwrap_err()
                    .to_string()
                    .contains("time/output limit")
            );
            child.finish().unwrap();
            assert_stopped(dir.path());
        }
    }
}
