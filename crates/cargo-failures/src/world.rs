//! What cargo-failures does to the world: reads its arguments and the
//! environment, finds cargo, runs it once, and prints what the filter
//! keeps of its output as it arrives. The one module that builds a cargo
//! command line.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

use crate::{
    cargo_args, cargo_candidates, exit_for, help, parse, Asked, Filter, Sub, EXIT_NO_CARGO,
    EXIT_USAGE, NAME, USAGE,
};

/// Whether a path is a file this process may run.
fn runnable(p: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(p) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.is_file() && meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        meta.is_file()
    }
}

/// The cargo to run, or every place looked in.
fn find_cargo() -> Result<PathBuf, Vec<PathBuf>> {
    let var = |k: &str| std::env::var(k).ok();
    let candidates = cargo_candidates(
        var("PATH").as_deref(),
        var("CARGO_HOME").as_deref(),
        var("HOME").as_deref(),
    );
    match candidates.iter().find(|p| runnable(p)) {
        Some(p) => Ok(p.clone()),
        None => Err(candidates),
    }
}

fn fail(code: u8, reason: &str) -> ExitCode {
    eprintln!("{NAME}: {reason}");
    ExitCode::from(code)
}

/// Runs cargo once and prints what the filter keeps; cargo's exit code.
fn run(sub: Sub, rest: &[String]) -> ExitCode {
    let cargo = match find_cargo() {
        Ok(c) => c,
        Err(looked) => {
            let looked: Vec<String> = looked.iter().map(|p| p.display().to_string()).collect();
            return fail(
                EXIT_NO_CARGO,
                &format!(
                    "cargo not found on PATH, in $CARGO_HOME/bin or in ~/.cargo/bin (looked at {}); install Rust with rustup or put cargo's directory on PATH",
                    looked.join(", ")
                ),
            );
        }
    };
    let mut child = match Command::new(&cargo)
        .args(cargo_args(sub, rest))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            return fail(
                EXIT_NO_CARGO,
                &format!("cannot start {}: {e}", cargo.display()),
            )
        }
    };
    // Cargo's stderr is drained beside its stdout, so neither pipe fills
    // and blocks it; it is passed on only when nothing printed explains a
    // failure.
    let mut err_pipe = child.stderr.take().expect("stderr is piped");
    let stderr = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = err_pipe.read_to_end(&mut buf);
        buf
    });
    let mut filter = Filter::new();
    let mut stdout = std::io::stdout().lock();
    let mut reader = BufReader::new(child.stdout.take().expect("stdout is piped"));
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        let text = String::from_utf8_lossy(&line);
        let out = filter.feed(text.trim_end_matches(['\n', '\r']));
        if !out.is_empty() {
            // A closed stdout must not stop cargo: keep reading to its end.
            let _ = stdout
                .write_all(out.as_bytes())
                .and_then(|()| stdout.flush());
        }
    }
    let rest_out = filter.finish();
    let _ = stdout
        .write_all(rest_out.as_bytes())
        .and_then(|()| stdout.flush());
    let status = match child.wait() {
        Ok(s) => s,
        Err(e) => return fail(1, &format!("cannot wait for cargo: {e}")),
    };
    let err_buf = stderr.join().unwrap_or_default();
    #[cfg(unix)]
    let signal = std::os::unix::process::ExitStatusExt::signal(&status);
    #[cfg(not(unix))]
    let signal = None;
    let (code, reason) = exit_for(status.code(), signal);
    if code != 0 && !filter.explains_failure() {
        let mut e = std::io::stderr().lock();
        let _ = e.write_all(&err_buf).and_then(|()| e.flush());
    }
    if let Some(r) = reason {
        eprintln!("{NAME}: {r}");
    }
    ExitCode::from(code)
}

pub fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse(&args) {
        Ok(Asked::Help) => {
            print!("{}", help());
            ExitCode::SUCCESS
        }
        Ok(Asked::Version) => {
            println!("{NAME} {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Ok(Asked::Run { sub, rest }) => run(sub, &rest),
        Err(e) => {
            eprintln!("{USAGE}\n{NAME}: error: {e}; see {NAME} --help");
            ExitCode::from(EXIT_USAGE)
        }
    }
}
