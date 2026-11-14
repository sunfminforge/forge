//! What with-cargo does to the world: reads its arguments and the
//! environment, finds the cargo bin dir, and prints it or execs the
//! command with it first on PATH.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use crate::{
    cargo_dirs, help, new_path, not_found, parse, Asked, EXIT_NOT_FOUND, EXIT_USAGE, NAME, USAGE,
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

/// The first dir that holds an executable cargo, or every dir looked in.
fn find_dir() -> Result<PathBuf, Vec<PathBuf>> {
    let dirs = cargo_dirs(
        std::env::var_os("CARGO_HOME").as_deref(),
        std::env::var_os("HOME").as_deref(),
    );
    match dirs.iter().find(|d| runnable(&d.join("cargo"))) {
        Some(d) => Ok(d.clone()),
        None => Err(dirs),
    }
}

fn fail(code: u8, reason: &str) -> ExitCode {
    eprintln!("{NAME}: {reason}");
    ExitCode::from(code)
}

/// Runs the command with the cargo bin dir first on its PATH. On unix it
/// replaces this process, so the command's exit code is with-cargo's.
fn run(command: OsString, args: Vec<OsString>) -> ExitCode {
    let dir = match find_dir() {
        Ok(d) => d,
        Err(looked) => return fail(EXIT_NOT_FOUND, &not_found(&looked)),
    };
    let path = match new_path(&dir, std::env::var_os("PATH").as_deref()) {
        Ok(p) => p,
        Err(e) => return fail(EXIT_NOT_FOUND, &e),
    };
    let mut cmd = Command::new(&command);
    // With PATH set on the command, std looks the command up on this PATH.
    cmd.args(&args).env("PATH", &path);
    let cannot = |e: std::io::Error| {
        fail(
            EXIT_NOT_FOUND,
            &format!(
                "cannot start {}: {e}; name a command on PATH or in {}",
                command.to_string_lossy(),
                dir.display()
            ),
        )
    };
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // exec returns only when it failed.
        cannot(cmd.exec())
    }
    #[cfg(not(unix))]
    {
        match cmd.status() {
            Ok(s) => ExitCode::from(s.code().and_then(|c| u8::try_from(c).ok()).unwrap_or(1)),
            Err(e) => cannot(e),
        }
    }
}

pub fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    match parse(&args) {
        Ok(Asked::Help) => {
            print!("{}", help());
            ExitCode::SUCCESS
        }
        Ok(Asked::Version) => {
            println!("{NAME} {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Ok(Asked::Dir) => match find_dir() {
            Ok(d) => {
                println!("{}", d.display());
                ExitCode::SUCCESS
            }
            Err(looked) => fail(EXIT_NOT_FOUND, &not_found(&looked)),
        },
        Ok(Asked::Run { command, args }) => run(command, args),
        Err(e) => {
            eprintln!("{USAGE}\n{NAME}: error: {e}; see {NAME} --help");
            ExitCode::from(EXIT_USAGE)
        }
    }
}
