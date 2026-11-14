//! `curl`, the one place its command line is built. The token never stands
//! on the command line, where `ps` shows it: the request is a curl config on
//! stdin (`--config -`), after `-q`, so no `~/.curlrc` is read.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::decide::Token;

/// The arguments curl runs with: everything else is on stdin.
pub const ARGS: [&str; 3] = ["-q", "--config", "-"];
/// What curl writes after the body: a newline and the HTTP status.
pub const WRITE_OUT: &str = "\\n%{http_code}";
/// How long one request may take, in seconds.
pub const MAX_TIME: u32 = 30;

/// The config curl reads on stdin for a GET of `url`, with `token`.
pub fn config(url: &str, token: &Token) -> String {
    format!(
        "url = \"{url}\"\nheader = \"Authorization: token {}\"\nheader = \"Accept: application/json\"\nsilent\nshow-error\nmax-time = {MAX_TIME}\nwrite-out = \"{WRITE_OUT}\"\n",
        token.secret()
    )
}

/// The first `curl` on `path` (a PATH's value), if any.
pub fn find(path: Option<&str>) -> Option<PathBuf> {
    std::env::split_paths(path?)
        .map(|dir| dir.join("curl"))
        .find(|candidate| executable(candidate))
}

fn executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// What one run of curl said: its stdout, whether it exited 0, its stderr.
pub struct Ran {
    pub stdout: String,
    pub ok: bool,
    pub stderr: String,
}

/// Runs `curl` with `config` on stdin.
pub fn run(curl: &Path, config: &str) -> std::io::Result<Ran> {
    let mut child = Command::new(curl)
        .args(ARGS)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(config.as_bytes())?;
    }
    let out = child.wait_with_output()?;
    Ok(Ran {
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        ok: out.status.success(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    })
}
