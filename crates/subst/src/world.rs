//! What subst does to the world: reads its arguments and every file it
//! names, asks lib what the substitutions do, and when every one matched
//! as required writes the changed files: each to a tempfile beside it
//! first, then every tempfile renamed over its path; a failure on the way
//! puts back the files already renamed. Then it prints.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use tempfile::NamedTempFile;

use crate::{
    apply, compile, fault_line, help, parse, render, Applied, Asked, Given, Outcome, EXIT_FAILED,
    EXIT_REFUSED, NAME, USAGE,
};

fn fail(msg: &str) -> ExitCode {
    eprintln!("{NAME}: error: {msg}");
    ExitCode::from(EXIT_FAILED)
}

fn dir_of(path: &Path) -> &Path {
    match path.parent() {
        Some(d) if !d.as_os_str().is_empty() => d,
        _ => Path::new("."),
    }
}

/// Writes `bytes` to a tempfile beside `path`, synced, with `path`'s
/// permissions.
fn temp_for(path: &Path, bytes: &[u8]) -> io::Result<NamedTempFile> {
    let perms = fs::metadata(path)?.permissions();
    let mut tmp = tempfile::Builder::new()
        .prefix(".subst.")
        .tempfile_in(dir_of(path))?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    fs::set_permissions(tmp.path(), perms)?;
    Ok(tmp)
}

/// One file read and changed in memory.
struct Changed {
    path: String,
    original: Vec<u8>,
    text: Vec<u8>,
    done: Vec<Applied>,
}

/// Renames a tempfile over each changed file, every tempfile written
/// before the first rename; on a failure puts back what was renamed, and
/// says so.
fn write_all(files: &[Changed]) -> Result<(), String> {
    let mut staged = Vec::new();
    for f in files.iter().filter(|f| f.text != f.original) {
        match temp_for(Path::new(&f.path), &f.text) {
            Ok(t) => staged.push((f, t)),
            Err(e) => {
                return Err(format!(
                    "cannot write {}: {e}; nothing written, every file is as it was; make it and its directory writable",
                    f.path
                ))
            }
        }
    }
    let mut done: Vec<&Changed> = Vec::new();
    for (f, tmp) in staged {
        if let Err(e) = tmp.persist(&f.path) {
            let mut left = Vec::new();
            for d in done.iter().rev() {
                let p = PathBuf::from(&d.path);
                let r = temp_for(&p, &d.original)
                    .and_then(|t| t.persist(&p).map(|_| ()).map_err(|e| e.error));
                if let Err(e) = r {
                    left.push(format!("{} ({e})", d.path));
                }
            }
            let back = if left.is_empty() {
                "what was written is put back, every file is as it was".to_string()
            } else {
                format!(
                    "could not put back {}; restore them from version control",
                    left.join(", ")
                )
            };
            return Err(format!(
                "cannot write {}: {}; {back}; make it and its directory writable",
                f.path, e.error
            ));
        }
        done.push(f);
    }
    Ok(())
}

fn run(given: &[Given], paths: &[String], dry_run: bool) -> ExitCode {
    let (substs, mut faults) = match compile(given) {
        Ok(s) => (s, Vec::new()),
        Err(f) => (Vec::new(), f),
    };
    let mut read = Vec::new();
    for p in paths {
        match fs::read(p) {
            Ok(b) => read.push((p, b)),
            Err(e) => faults.push(format!(
                "cannot read {p}: {e}; name a file that exists and is readable, or drop it from the call\n"
            )),
        }
    }
    if !faults.is_empty() {
        eprintln!("{NAME}: error: nothing written, every file is as it was");
        let mut err = io::stderr().lock();
        for f in &faults {
            let _ = err.write_all(f.as_bytes());
        }
        return ExitCode::from(EXIT_FAILED);
    }
    let mut changed = Vec::new();
    let mut refused = Vec::new();
    for (path, original) in read {
        match apply(path, &original, &substs) {
            Outcome::Applied { text, substs: done } => changed.push(Changed {
                path: path.clone(),
                original,
                text,
                done,
            }),
            Outcome::Refused(f) => refused.extend(f),
        }
    }
    if !refused.is_empty() {
        let mut err = io::stderr().lock();
        let _ = writeln!(
            err,
            "{NAME}: refused: {} not matched as required; no file written, every file is as it was",
            if refused.len() == 1 {
                "1 substitution".to_string()
            } else {
                format!("{} substitutions", refused.len())
            }
        );
        for f in &refused {
            let _ = err.write_all(fault_line(f).as_bytes());
        }
        return ExitCode::from(EXIT_REFUSED);
    }
    if !dry_run {
        if let Err(e) = write_all(&changed) {
            return fail(&e);
        }
    }
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    let res = (|| {
        for (i, c) in changed.iter().enumerate() {
            if i > 0 {
                out.write_all(b"\n")?;
            }
            out.write_all(&render(&c.path, &c.original, &c.text, &substs, &c.done))?;
        }
        out.flush()
    })();
    if dry_run {
        eprintln!("{NAME}: dry run: nothing written");
    }
    match res {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => fail(&format!("cannot write stdout: {e}")),
    }
}

pub fn main() -> ExitCode {
    let mut args = Vec::new();
    for a in std::env::args_os().skip(1) {
        match a.into_string() {
            Ok(s) => args.push(s),
            Err(a) => {
                eprintln!("{USAGE}");
                return fail(&format!(
                    "argument {} is not UTF-8; see {NAME} --help",
                    a.to_string_lossy()
                ));
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
        Ok(Asked::Apply {
            substs,
            files,
            dry_run,
        }) => run(&substs, &files, dry_run),
        Err(e) => {
            eprintln!("{USAGE}");
            fail(&format!("{e}; see {NAME} --help"))
        }
    }
}
