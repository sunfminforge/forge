//! `guild gate`, the one guild command gate-head runs: its stdout passed on
//! line by line as it comes, and kept; its stderr left to gate-head's own.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

/// The program run, found on the PATH.
pub const GUILD: &str = "guild";

/// The arguments of `guild gate`, `--slow` and `--remote <name>` passed
/// through as given.
pub fn gate_args(slow: bool, remote: Option<&str>) -> Vec<String> {
    let mut args = vec!["gate".to_string()];
    if slow {
        args.push("--slow".into());
    }
    if let Some(name) = remote {
        args.push("--remote".into());
        args.push(name.to_string());
    }
    args
}

/// Runs `guild <args>` once; hands each line of its stdout to `each` as it
/// comes; answers every line and its exit code (`None`: stopped by a
/// signal), or why it could not be started.
pub fn run(
    args: &[String],
    mut each: impl FnMut(&str),
) -> Result<(Vec<String>, Option<i32>), String> {
    let mut child = Command::new(GUILD)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut lines = Vec::new();
    if let Some(out) = child.stdout.take() {
        let mut out = BufReader::new(out);
        let mut buf = Vec::new();
        // Bytes that are not UTF-8 are passed on replaced, not dropped.
        while matches!(out.read_until(b'\n', &mut buf), Ok(n) if n > 0) {
            let text = String::from_utf8_lossy(&buf);
            let line = text.strip_suffix('\n').unwrap_or(&text);
            let line = line.strip_suffix('\r').unwrap_or(line).to_string();
            each(&line);
            lines.push(line);
            buf.clear();
        }
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    Ok((lines, status.code()))
}
