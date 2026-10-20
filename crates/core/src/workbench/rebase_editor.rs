//! Internal Git editor mode shared by the desktop and CLI executables.
pub fn run_from_args() -> Option<i32> {
    use std::path::PathBuf;
    let args = std::env::args_os().collect::<Vec<_>>();
    let mode = args.get(1)?.to_str()?;
    if !matches!(
        mode,
        "--clear-compare-sequence-editor" | "--clear-compare-message-editor"
    ) {
        return None;
    }
    let result = (|| -> Result<(), Box<dyn std::error::Error>> {
        if args.len() != 4 {
            return Err("Invalid internal Git editor arguments".into());
        }
        let payload_path = PathBuf::from(args.get(2).ok_or("Missing rebase plan")?);
        let raw = std::fs::read(&payload_path)?;
        if raw.len() > 16 * 1024 * 1024 {
            return Err("Rebase editor plan exceeds size limit".into());
        }
        let payload: serde_json::Value = serde_json::from_slice(&raw)?;
        let target = PathBuf::from(args.last().ok_or("Missing Git editor target")?);
        if mode == "--clear-compare-sequence-editor" {
            let todo = payload["todo"].as_str().ok_or("Missing rebase sequence")?;
            let parent = target.parent().ok_or("Missing Git rebase directory")?;
            replace_file(&parent.join("clear-compare-editor.json"), &raw)?;
            replace_file(&target, todo.as_bytes())?;
        } else {
            let done = std::fs::read_to_string(
                payload_path
                    .parent()
                    .ok_or("Missing rebase directory")?
                    .join("done"),
            )?;
            let last = done
                .lines()
                .rev()
                .find(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
                .ok_or("Missing current rebase step")?;
            let mut fields = last.split_whitespace();
            if matches!(fields.next(), Some("reword" | "r")) {
                let commit = fields.next().ok_or("Missing reword commit identity")?;
                let message = payload["messages"]
                    .get(commit)
                    .and_then(serde_json::Value::as_str)
                    .ok_or("Reword message is missing from the reviewed plan")?;
                let mut bytes = message.as_bytes().to_vec();
                if !bytes.ends_with(b"\n") {
                    bytes.push(b'\n');
                }
                replace_file(&target, &bytes)?;
            }
        }
        Ok(())
    })();
    match result {
        Ok(()) => Some(0),
        Err(error) => {
            eprintln!("Clear Compare Git editor failed: {error}");
            Some(1)
        }
    }
}

fn replace_file(path: &std::path::Path, contents: &[u8]) -> Result<(), Box<dyn std::error::Error>> {
    use std::io::Write;
    let parent = path.parent().ok_or("Git editor target has no parent")?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(contents)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path)?;
    Ok(())
}
