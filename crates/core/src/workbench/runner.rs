//! Bounded native Git subprocesses with typed arguments and owned internal editors.
mod process;
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
#[derive(Default)]
struct CommandInput<'a> {
    stdin: Option<&'a [u8]>,
    sequence_editor: Option<&'a str>,
    message_editor: Option<&'a str>,
}
pub fn run(repo: &Path, args: &[&str], cancel: &AtomicBool) -> Result<Vec<u8>> {
    run_owned(
        repo,
        &args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
        cancel,
    )
}
pub fn run_owned(repo: &Path, args: &[String], cancel: &AtomicBool) -> Result<Vec<u8>> {
    run_mode(
        repo,
        args,
        cancel,
        false,
        false,
        false,
        CommandInput::default(),
    )
}
pub fn optional(
    repo: &Path,
    args: &[String],
    input: &[u8],
    cancel: &AtomicBool,
) -> Result<Vec<u8>> {
    run_mode(
        repo,
        args,
        cancel,
        false,
        false,
        true,
        CommandInput {
            stdin: Some(input),
            ..CommandInput::default()
        },
    )
}
pub fn mutate(repo: &Path, args: &[String], cancel: &AtomicBool, network: bool) -> Result<Vec<u8>> {
    run_mode(
        repo,
        args,
        cancel,
        network,
        true,
        false,
        CommandInput::default(),
    )
}
pub fn mutate_input(
    repo: &Path,
    args: &[String],
    input: &[u8],
    cancel: &AtomicBool,
) -> Result<Vec<u8>> {
    run_mode(
        repo,
        args,
        cancel,
        false,
        true,
        false,
        CommandInput {
            stdin: Some(input),
            ..CommandInput::default()
        },
    )
}
pub fn mutate_sequence(
    repo: &Path,
    git: &Path,
    args: &[String],
    todo: &[u8],
    messages: &std::collections::BTreeMap<String, String>,
    cancel: &AtomicBool,
) -> Result<Vec<u8>> {
    let executable = std::env::current_exe()?;
    let directory = tempfile::tempdir()?;
    let plan = directory.path().join("rebase-plan.json");
    let mut plan_file = File::create(&plan)?;
    serde_json::to_writer(
        &mut plan_file,
        &serde_json::json!({
            "todo": String::from_utf8(todo.to_vec()).map_err(|_| Error::Invalid("Invalid generated rebase sequence".into()))?,
            "messages": messages,
        }),
    )?;
    plan_file.flush()?;
    let sequence_editor = format!(
        "{} --clear-compare-sequence-editor {}",
        shell_quote(&executable)?,
        shell_quote(&plan)?,
    );
    let message_editor = message_editor(&executable, git)?;
    run_mode(
        repo,
        args,
        cancel,
        false,
        true,
        false,
        CommandInput {
            sequence_editor: Some(&sequence_editor),
            message_editor: Some(&message_editor),
            ..CommandInput::default()
        },
    )
}

pub fn continue_sequence(
    repo: &Path,
    git: &Path,
    args: &[String],
    cancel: &AtomicBool,
) -> Result<Vec<u8>> {
    let directory = git.join("rebase-merge");
    let payload = std::fs::read(directory.join("clear-compare-editor.json"))?;
    if payload.len() > 16 * 1024 * 1024 {
        return Err(Error::Invalid(
            "Managed rebase plan exceeds size limit".into(),
        ));
    }
    let payload: serde_json::Value = serde_json::from_slice(&payload)?;
    let original = payload["todo"]
        .as_str()
        .ok_or_else(|| Error::Invalid("Managed rebase sequence missing".into()))?;
    let mut actual = std::fs::read_to_string(directory.join("done"))?;
    actual.push('\n');
    actual.push_str(&std::fs::read_to_string(directory.join("git-rebase-todo"))?);
    let steps = |text: &str| {
        text.lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    if steps(original) != steps(&actual) {
        return Err(Error::Invalid(
            "The rebase sequence was changed externally; review it in Git before continuing".into(),
        ));
    }
    let editor = message_editor(&std::env::current_exe()?, git)?;
    run_mode(
        repo,
        args,
        cancel,
        false,
        true,
        false,
        CommandInput {
            message_editor: Some(&editor),
            ..CommandInput::default()
        },
    )
}

fn message_editor(executable: &Path, git: &Path) -> Result<String> {
    Ok(format!(
        "{} --clear-compare-message-editor {}",
        shell_quote(executable)?,
        shell_quote(&git.join("rebase-merge/clear-compare-editor.json"))?
    ))
}

fn shell_quote(path: &Path) -> Result<String> {
    let value = path.to_str().ok_or_else(|| {
        Error::Invalid("Managed Git editors require Unicode executable and repository paths".into())
    })?;
    #[cfg(windows)]
    let value = value.replace('\\', "/");
    Ok(format!("'{}'", value.replace('\'', "'\\''")))
}
fn run_mode(
    repo: &Path,
    args: &[String],
    cancel: &AtomicBool,
    network: bool,
    include_stderr: bool,
    no_match: bool,
    input: CommandInput<'_>,
) -> Result<Vec<u8>> {
    let CommandInput {
        stdin: input,
        sequence_editor,
        message_editor,
    } = input;
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
    // Stash synthesizes internal magic pathspecs for cleaning untracked files.
    // Its typed calls accept revisions/messages, never caller-supplied pathspecs.
    if args
        .first()
        .is_none_or(|a| a != "check-ignore" && a != "stash" && a != "submodule")
    {
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
        .env("GIT_EDITOR", message_editor.unwrap_or("true"))
        .env("GIT_SEQUENCE_EDITOR", sequence_editor.unwrap_or("false"))
        .env("LC_ALL", "C")
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(if let Some(file) = &input_file {
            Stdio::from(File::open(file.path())?)
        } else {
            Stdio::null()
        })
        .stdout(Stdio::from(out.as_file().try_clone()?))
        .stderr(Stdio::from(err.as_file().try_clone()?));
    let mut child = process::ManagedChild::spawn(&mut command)
        .map_err(|e| Error::Invalid(format!("Cannot start Git: {e}")))?;
    let outcome = monitor(
        &mut child,
        cancel,
        out.as_file(),
        err.as_file(),
        Duration::from_secs(if network { 120 } else { 30 }),
        16 * 1024 * 1024,
        256 * 1024,
    );
    // Always stop helpers, including on successful root exit and inspection errors.
    // Cleanup errors take precedence: do not claim cancellation has finished otherwise.
    let status = child
        .finish()
        .map_err(|e| Error::Invalid(format!("Git process cleanup failed: {e}")))?;
    outcome?;
    let mut output = Vec::new();
    File::open(out.path())?
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut output)?;
    if output.len() > 16 * 1024 * 1024 {
        return Err(Error::Invalid("Git output exceeded its limit".into()));
    }
    let mut message = Vec::new();
    File::open(err.path())?
        .take(256 * 1024 + 1)
        .read_to_end(&mut message)?;
    if message.len() > 256 * 1024 {
        return Err(Error::Invalid(
            "Git diagnostic output exceeded its limit".into(),
        ));
    }
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
fn monitor(
    child: &mut process::ManagedChild,
    cancel: &AtomicBool,
    out: &File,
    err: &File,
    timeout: Duration,
    out_limit: u64,
    err_limit: u64,
) -> Result<()> {
    let start = Instant::now();
    loop {
        check_cancel(cancel)?;
        if start.elapsed() > timeout
            || out.metadata()?.len() > out_limit
            || err.metadata()?.len() > err_limit
        {
            return Err(Error::Invalid(
                "Git command exceeded its time/output limit".into(),
            ));
        }
        if child.complete()? {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(2));
    }
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
            let Some(scheme) = part.find("://") else {
                return part.to_owned();
            };
            let end = part.trim_end_matches(char::is_whitespace).len();
            let value = &part[..end];
            let whitespace = &part[end..];
            let rest = &value[scheme + 3..];
            let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
            let authority = &rest[..authority_end];
            let safe_authority = if let Some(at) = authority.rfind('@') {
                format!("***@{}", &authority[at + 1..])
            } else {
                authority.into()
            };
            let tail = &rest[authority_end..];
            let tail = if let Some(i) = tail.find(['?', '#']) {
                format!("{}{}***", &tail[..i], &tail[i..i + 1])
            } else {
                tail.into()
            };
            format!(
                "{}{}{}{}",
                &value[..scheme + 3],
                safe_authority,
                tail,
                whitespace
            )
        })
        .collect()
}

/// Translate native absolute paths only at Git argument boundaries. Never rewrite
/// arbitrary arguments such as commit messages, remote URLs or relative names.
pub(super) fn path_arg(path: &Path) -> Result<String> {
    let text = path
        .to_str()
        .ok_or_else(|| Error::Invalid("Git path must be Unicode".into()))?;
    #[cfg(windows)]
    if let Some(rest) = text.strip_prefix(r"\\?\") {
        if let Some(unc) = rest.strip_prefix(r"UNC\") {
            return Ok(format!("//{}", unc.replace('\\', "/")));
        }
        if rest.as_bytes().get(1) == Some(&b':') {
            return Ok(rest.replace('\\', "/"));
        }
        return Err(Error::Invalid(
            "Git does not support this Windows device path".into(),
        ));
    }
    Ok(text.into())
}

#[cfg(all(test, unix))]
mod cancellation_tests {
    use super::*;
    use std::{fs, os::unix::fs::PermissionsExt, sync::atomic::Ordering};
    #[test]
    fn fast_successful_command_cannot_bypass_stderr_limit() {
        let dir = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        run(dir.path(), &["init", "-q"], &cancel).unwrap();
        let script = format!(
            r#"!i=0; while test "$i" -lt 300; do printf '%s' '{}' >&2; i=$((i+1)); done"#,
            "x".repeat(1024)
        );
        run(dir.path(), &["config", "alias.noisy", &script], &cancel).unwrap();
        let error = run(dir.path(), &["noisy"], &cancel).unwrap_err();
        assert!(error.to_string().contains("limit"));
    }
    #[test]
    fn cancelling_a_running_git_hook_stops_its_background_writer() {
        let dir = tempfile::tempdir().unwrap();
        let cancel = AtomicBool::new(false);
        run(dir.path(), &["init", "-q"], &cancel).unwrap();
        run(dir.path(), &["config", "user.name", "Test"], &cancel).unwrap();
        run(
            dir.path(),
            &["config", "user.email", "test@example.com"],
            &cancel,
        )
        .unwrap();
        fs::write(dir.path().join("file"), "data").unwrap();
        run(dir.path(), &["add", "file"], &cancel).unwrap();
        let hook = dir.path().join(".git/hooks/pre-commit");
        fs::write(&hook, "#!/bin/sh\n( i=0; while [ $i -lt 300 ]; do echo $i > heartbeat; i=$((i+1)); sleep 0.01; done ) &\necho ready > ready\nwait\n").unwrap();
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o700)).unwrap();
        thread::scope(|scope| {
            let worker = scope.spawn(|| {
                mutate(
                    dir.path(),
                    &["commit".into(), "-m".into(), "test".into()],
                    &cancel,
                    false,
                )
            });
            let start = Instant::now();
            while !dir.path().join("ready").exists() || !dir.path().join("heartbeat").exists() {
                assert!(start.elapsed() < Duration::from_secs(4));
                thread::sleep(Duration::from_millis(2));
            }
            cancel.store(true, Ordering::Relaxed);
            assert!(matches!(worker.join().unwrap(), Err(Error::Cancelled)));
        });
        let before = fs::read(dir.path().join("heartbeat")).unwrap();
        thread::sleep(Duration::from_millis(60));
        assert_eq!(before, fs::read(dir.path().join("heartbeat")).unwrap());
        cancel.store(false, Ordering::Relaxed);
        assert!(run(dir.path(), &["rev-parse", "--verify", "HEAD"], &cancel).is_err());
        assert!(!dir.path().join(".git/index.lock").exists());
    }
}

#[cfg(all(test, windows))]
mod path_tests {
    use super::*;
    #[test]
    fn native_windows_paths_become_git_paths_without_rewriting_relative_names() {
        assert_eq!(
            path_arg(Path::new(r"\\?\C:\项目 with space\repo")).unwrap(),
            "C:/项目 with space/repo"
        );
        assert_eq!(
            path_arg(Path::new(r"\\?\UNC\server\share\repo")).unwrap(),
            "//server/share/repo"
        );
        assert_eq!(
            path_arg(Path::new("[literal].txt")).unwrap(),
            "[literal].txt"
        );
        assert!(path_arg(Path::new(r"\\?\GLOBALROOT\Device\x")).is_err());
    }
}
