//! Bounded native Git subprocesses; no shell command concatenation.
use crate::{Error, Result, compare::check_cancel};
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
    process::{Command, Stdio},
    sync::atomic::AtomicBool,
    thread,
    time::{Duration, Instant},
};
pub fn run(repo: &Path, args: &[&str], cancel: &AtomicBool) -> Result<Vec<u8>> {
    run_owned(
        repo,
        &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        cancel,
    )
}
pub fn run_owned(repo: &Path, args: &[String], cancel: &AtomicBool) -> Result<Vec<u8>> {
    run_mode(repo, args, cancel, false, false, false, None)
}
pub fn optional(
    repo: &Path,
    args: &[String],
    input: &[u8],
    cancel: &AtomicBool,
) -> Result<Vec<u8>> {
    run_mode(repo, args, cancel, false, false, true, Some(input))
}
pub fn mutate(repo: &Path, args: &[String], cancel: &AtomicBool, network: bool) -> Result<Vec<u8>> {
    run_mode(repo, args, cancel, network, true, false, None)
}
pub fn mutate_input(
    repo: &Path,
    args: &[String],
    input: &[u8],
    cancel: &AtomicBool,
) -> Result<Vec<u8>> {
    run_mode(repo, args, cancel, false, true, false, Some(input))
}
fn run_mode(
    repo: &Path,
    args: &[String],
    cancel: &AtomicBool,
    network: bool,
    include_stderr: bool,
    no_match: bool,
    input: Option<&[u8]>,
) -> Result<Vec<u8>> {
    check_cancel(cancel)?;
    let out = tempfile::NamedTempFile::new()?;
    let err = tempfile::NamedTempFile::new()?;
    let input_file = input
        .map(|bytes| -> Result<tempfile::NamedTempFile> {
            let mut file = tempfile::NamedTempFile::new()?;
            file.write_all(bytes)?;
            file.flush()?;
            Ok(file)
        })
        .transpose()?;
    let mut command = Command::new("git");
    for (name, _) in std::env::vars_os() {
        if name.to_str().is_some_and(|n| n.starts_with("GIT_")) {
            command.env_remove(name);
        }
    }
    command.current_dir(repo).args([
        "--no-pager",
        "--no-optional-locks",
        "--no-replace-objects",
        "--no-lazy-fetch",
        "-c",
        "core.fsmonitor=false",
        "-c",
        "protocol.allow=never",
    ]);
    if args.first().is_none_or(|a| a != "check-ignore") {
        command.arg("--literal-pathspecs");
    }
    if network {
        command.args([
            "-c",
            "protocol.https.allow=always",
            "-c",
            "protocol.http.allow=always",
            "-c",
            "protocol.ssh.allow=always",
            "-c",
            "protocol.git.allow=always",
            "-c",
            "protocol.file.allow=always",
        ]);
    }
    command
        .args(args)
        .env("GIT_EDITOR", "true")
        .env("GIT_SEQUENCE_EDITOR", "false")
        .env("LC_ALL", "C")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(if let Some(file) = &input_file {
            Stdio::from(File::open(file.path())?)
        } else {
            Stdio::null()
        })
        .stdout(Stdio::from(out.as_file().try_clone()?))
        .stderr(Stdio::from(err.as_file().try_clone()?));
    let mut child = command
        .spawn()
        .map_err(|e| Error::Invalid(format!("Cannot start Git: {e}")))?;
    let start = Instant::now();
    let status = loop {
        if check_cancel(cancel).is_err()
            || start.elapsed() > Duration::from_secs(if network { 120 } else { 30 })
            || out.as_file().metadata()?.len() > 16 * 1024 * 1024
            || err.as_file().metadata()?.len() > 256 * 1024
        {
            let _ = child.kill();
            let _ = child.wait();
            check_cancel(cancel)?;
            return Err(Error::Invalid(
                "Git command exceeded its time/output limit".into(),
            ));
        }
        match child.try_wait()? {
            Some(s) => break s,
            None => thread::sleep(Duration::from_millis(2)),
        }
    };
    let mut output = Vec::new();
    File::open(out.path())?
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut output)?;
    if output.len() > 16 * 1024 * 1024 {
        return Err(Error::Invalid("Git output exceeded its limit".into()));
    }
    let mut message = Vec::new();
    File::open(err.path())?
        .take(256 * 1024)
        .read_to_end(&mut message)?;
    if !status.success() {
        if no_match && status.code() == Some(1) {
            return Ok(output);
        }
        output.extend_from_slice(&message);
        return Err(Error::Invalid(redact(&String::from_utf8_lossy(&output))));
    }
    if include_stderr && !message.is_empty() {
        if !output.is_empty() && !output.ends_with(b"\n") {
            output.push(b'\n');
        }
        output.extend_from_slice(&message);
    }
    Ok(output)
}
pub fn text(repo: &Path, args: &[&str], cancel: &AtomicBool) -> Result<String> {
    Ok(String::from_utf8_lossy(&run(repo, args, cancel)?)
        .trim_end_matches(['\n', '\r'])
        .to_string())
}
pub fn redact(input: &str) -> String {
    input
        .split_inclusive(char::is_whitespace)
        .map(|part| {
            if let Some(scheme) = part.find("://") {
                let rest = &part[scheme + 3..];
                if let Some(at) = rest.find('@') {
                    return format!("{}***@{}", &part[..scheme + 3], &rest[at + 1..]);
                }
            }
            part.to_owned()
        })
        .collect()
}
