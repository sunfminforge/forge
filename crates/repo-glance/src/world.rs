//! What repo-glance does to the world: runs git read-only (optional
//! locks off, so not even the index is refreshed), stats and reads each
//! path, and prints what lib and peek's lib decide. It never writes.

use std::fs;
use std::io::{self, ErrorKind, Write};
use std::process::{Command, ExitCode};

use peek::{dir_body, error_line, file_body, header, Entry};

use crate::{
    branch, git_read, git_section, help, parse, Asked, Git, COMMITS, EXIT_FAILED, NAME, USAGE,
};

/// The one place a git command line is built: read-only, no colour,
/// in the current directory.
fn git(args: &[&str]) -> io::Result<std::process::Output> {
    Command::new("git")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(["--no-optional-locks", "-c", "color.ui=false"])
        .args(args)
        .output()
}

/// Runs one git command; its stdout, or what the section says instead.
fn run(args: &[&str]) -> Result<String, Git> {
    match git(args) {
        Err(e) if e.kind() == ErrorKind::NotFound => Err(Git::Missing),
        Err(e) => Err(Git::Failed {
            command: format!("git {}", args.join(" ")),
            stderr: e.to_string(),
        }),
        Ok(o) if o.status.success() => Ok(String::from_utf8_lossy(&o.stdout).into_owned()),
        Ok(o) => Err(Git::Failed {
            command: format!("git {}", args.join(" ")),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
        }),
    }
}

/// What git says of the repo the current directory is in.
fn read_git() -> Git {
    let dir = std::env::current_dir()
        .map(|d| d.display().to_string())
        .unwrap_or_else(|_| ".".into());
    match run(&["rev-parse", "--is-inside-work-tree"]) {
        Ok(s) if s.trim() == "true" => {}
        Ok(_) => return Git::NotRepo { dir },
        Err(Git::Failed { stderr, .. }) if stderr.contains("not a git repository") => {
            return Git::NotRepo { dir }
        }
        Err(g) => return g,
    }
    let porcelain_v2 = match run(&[
        "status",
        "--porcelain=v2",
        "--branch",
        "--untracked-files=no",
    ]) {
        Ok(s) => s,
        Err(g) => return g,
    };
    let short = match run(&["status", "--short"]) {
        Ok(s) => s,
        Err(g) => return g,
    };
    let log = if branch(&porcelain_v2).oid.is_none() {
        String::new()
    } else {
        let n = format!("-{COMMITS}");
        match run(&["log", &n, "--format=%h %s"]) {
            Ok(s) => s,
            Err(g) => return g,
        }
    };
    Git::Repo {
        porcelain_v2,
        short,
        log,
    }
}

/// A directory's entries, each marked a directory when it is one or
/// links to one.
fn entries(path: &str) -> io::Result<Vec<Entry>> {
    let mut out = Vec::new();
    for e in fs::read_dir(path)? {
        let e = e?;
        let dir = fs::metadata(e.path()).map(|m| m.is_dir()).unwrap_or(false);
        out.push(Entry {
            name: e.file_name().to_string_lossy().into_owned(),
            dir,
        });
    }
    Ok(out)
}

/// What one path prints after its header, as peek prints it, and
/// whether it was read.
fn body(path: &str) -> (Vec<u8>, bool) {
    let read = fs::metadata(path).and_then(|m| {
        if m.is_dir() {
            entries(path).map(|e| dir_body(e).into_bytes())
        } else {
            fs::read(path).map(|b| file_body(&b))
        }
    });
    match read {
        Ok(b) => (b, true),
        Err(e) => (
            error_line(path, e.kind(), &e.to_string()).into_bytes(),
            false,
        ),
    }
}

/// Prints the git section and every path in order; whether all was read.
fn glance(paths: &[String]) -> io::Result<bool> {
    let g = read_git();
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    out.write_all(git_section(&g).as_bytes())?;
    let mut ok = git_read(&g);
    for path in paths {
        out.write_all(b"\n")?;
        out.write_all(header(path).as_bytes())?;
        let (b, read) = body(path);
        ok &= read;
        out.write_all(&b)?;
    }
    out.flush()?;
    Ok(ok)
}

pub fn main() -> ExitCode {
    let mut args = Vec::new();
    for a in std::env::args_os().skip(1) {
        match a.into_string() {
            Ok(s) => args.push(s),
            Err(a) => {
                eprintln!(
                    "{USAGE}\n{NAME}: error: argument {} is not UTF-8; name the path by a UTF-8 name; see {NAME} --help",
                    a.to_string_lossy()
                );
                return ExitCode::from(EXIT_FAILED);
            }
        }
    }
    match parse(&args) {
        Ok(Asked::Help) => {
            print!("{}", help());
            ExitCode::SUCCESS
        }
        Ok(Asked::Version) => {
            println!("{NAME} {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Ok(Asked::Glance(paths)) => match glance(&paths) {
            Ok(true) => ExitCode::SUCCESS,
            Ok(false) => ExitCode::from(EXIT_FAILED),
            Err(e) => {
                eprintln!("{NAME}: error: cannot write stdout: {e}");
                ExitCode::from(EXIT_FAILED)
            }
        },
        Err(e) => {
            eprintln!("{USAGE}\n{NAME}: error: {e}; see {NAME} --help");
            ExitCode::from(EXIT_FAILED)
        }
    }
}
