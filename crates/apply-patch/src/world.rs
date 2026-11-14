//! What apply-patch does to the world: reads its arguments, the patch on
//! stdin and every file it names, asks lib what the changes do, and when
//! every change can be made writes them: each new or updated file to a
//! tempfile beside it first, then every tempfile renamed over its path and
//! every deleted file removed; a failure on the way puts back what was
//! done. Then it prints.

use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use tempfile::NamedTempFile;

use crate::{
    help, parse, parse_patch, plan, render, Asked, Done, Found, Planned, EXIT_FAILED, EXIT_REFUSED,
    NAME, USAGE,
};

fn fail(msg: &str) -> ExitCode {
    eprintln!("{NAME}: error: {msg}");
    ExitCode::from(EXIT_FAILED)
}

/// What is at `path` now: nothing, a directory, or a file and its bytes.
fn find(path: &Path) -> Found {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Found::Missing,
        Err(e) => Found::Unreadable(e.to_string()),
        Ok(_) if path.is_dir() => Found::Dir,
        Ok(_) => match fs::read(path) {
            Ok(b) => Found::File(b),
            Err(e) => Found::Unreadable(e.to_string()),
        },
    }
}

fn dir_of(path: &Path) -> &Path {
    match path.parent() {
        Some(d) if !d.as_os_str().is_empty() => d,
        _ => Path::new("."),
    }
}

/// One change made ready: a written tempfile waiting to be renamed, or a
/// file to remove.
enum Staged {
    Write {
        path: PathBuf,
        tmp: NamedTempFile,
        /// The bytes to put back, when it updates a file; none for an add.
        original: Option<Vec<u8>>,
    },
    Delete {
        path: PathBuf,
        original: Vec<u8>,
        perms: fs::Permissions,
    },
}

/// What was done, so it can be put back.
enum Committed {
    Added(PathBuf),
    Updated(PathBuf, Vec<u8>),
    Deleted(PathBuf, Vec<u8>, fs::Permissions),
}

/// Writes `bytes` to a tempfile in `dir`, synced, with `perms`.
fn temp_in(dir: &Path, bytes: &[u8], perms: Option<fs::Permissions>) -> io::Result<NamedTempFile> {
    let mut tmp = tempfile::Builder::new()
        .prefix(".apply-patch.")
        .tempfile_in(dir)?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    match perms {
        Some(p) => fs::set_permissions(tmp.path(), p)?,
        None => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(tmp.path(), fs::Permissions::from_mode(0o644))?;
            }
        }
    }
    Ok(tmp)
}

/// Makes `dir` and its missing parents; the ones it made, deepest last.
fn make_dirs(dir: &Path, made: &mut Vec<PathBuf>) -> io::Result<()> {
    let mut missing: Vec<PathBuf> = dir
        .ancestors()
        .take_while(|a| !a.as_os_str().is_empty() && !a.exists())
        .map(Path::to_path_buf)
        .collect();
    missing.reverse();
    for d in missing {
        fs::create_dir(&d)?;
        made.push(d);
    }
    Ok(())
}

fn remove_dirs(made: &[PathBuf]) {
    for d in made.iter().rev() {
        let _ = fs::remove_dir(d);
    }
}

/// Every tempfile written, none renamed yet; or the error, every
/// tempfile and made directory removed.
fn stage(planned: &[Planned], made: &mut Vec<PathBuf>) -> Result<Vec<Staged>, String> {
    let mut staged = Vec::new();
    for p in planned {
        let path = PathBuf::from(&p.path);
        let r = match &p.done {
            Done::Added { text } => make_dirs(dir_of(&path), made)
                .and_then(|()| temp_in(dir_of(&path), text, None))
                .map(|tmp| {
                    Some(Staged::Write {
                        path: path.clone(),
                        tmp,
                        original: None,
                    })
                }),
            Done::Updated { original, text, .. } if text == original => Ok(None),
            Done::Updated { original, text, .. } => fs::metadata(&path)
                .and_then(|m| temp_in(dir_of(&path), text, Some(m.permissions())))
                .map(|tmp| {
                    Some(Staged::Write {
                        path: path.clone(),
                        tmp,
                        original: Some(original.clone()),
                    })
                }),
            Done::Deleted { original } => fs::metadata(&path).map(|m| {
                Some(Staged::Delete {
                    path: path.clone(),
                    original: original.clone(),
                    perms: m.permissions(),
                })
            }),
        };
        match r {
            Ok(Some(s)) => staged.push(s),
            Ok(None) => {}
            Err(e) => {
                drop(staged);
                remove_dirs(made);
                return Err(format!(
                    "cannot write {}: {e}; nothing written, every file is as it was; make it and its directory writable",
                    p.path
                ));
            }
        }
    }
    Ok(staged)
}

/// Puts back what `done` did, last first; the paths it could not.
fn undo(done: Vec<Committed>, made: &[PathBuf]) -> Vec<String> {
    let mut left = Vec::new();
    for c in done.into_iter().rev() {
        let (path, r) = match c {
            Committed::Added(p) => {
                let r = fs::remove_file(&p);
                (p, r)
            }
            Committed::Updated(p, original) => {
                let r = fs::metadata(&p)
                    .and_then(|m| temp_in(dir_of(&p), &original, Some(m.permissions())))
                    .and_then(|t| t.persist(&p).map(|_| ()).map_err(|e| e.error));
                (p, r)
            }
            Committed::Deleted(p, original, perms) => {
                let r = temp_in(dir_of(&p), &original, Some(perms))
                    .and_then(|t| t.persist_noclobber(&p).map(|_| ()).map_err(|e| e.error));
                (p, r)
            }
        };
        if let Err(e) = r {
            left.push(format!("{} ({e})", path.display()));
        }
    }
    remove_dirs(made);
    left
}

/// Renames every tempfile over its path and removes every deleted file;
/// on a failure puts back what was done, and says so.
fn commit(staged: Vec<Staged>, made: &[PathBuf]) -> Result<(), String> {
    let mut done = Vec::new();
    for s in staged {
        let (path, r) = match s {
            Staged::Write {
                path,
                tmp,
                original: None,
            } => {
                let r = tmp
                    .persist_noclobber(&path)
                    .map(|_| ())
                    .map_err(|e| e.error);
                if r.is_ok() {
                    done.push(Committed::Added(path.clone()));
                }
                (path, r)
            }
            Staged::Write {
                path,
                tmp,
                original: Some(original),
            } => {
                let r = tmp.persist(&path).map(|_| ()).map_err(|e| e.error);
                if r.is_ok() {
                    done.push(Committed::Updated(path.clone(), original));
                }
                (path, r)
            }
            Staged::Delete {
                path,
                original,
                perms,
            } => {
                let r = fs::remove_file(&path);
                if r.is_ok() {
                    done.push(Committed::Deleted(path.clone(), original, perms));
                }
                (path, r)
            }
        };
        if let Err(e) = r {
            let left = undo(done, made);
            let back = if left.is_empty() {
                "what was done is put back, every file is as it was".to_string()
            } else {
                format!(
                    "could not put back {}; restore them from version control",
                    left.join(", ")
                )
            };
            return Err(format!(
                "cannot write {}: {e}; {back}; make it and its directory writable",
                path.display()
            ));
        }
    }
    Ok(())
}

fn run(dry_run: bool) -> ExitCode {
    let stdin = io::stdin();
    if stdin.is_terminal() {
        return fail("standard input is a terminal; send the patch on stdin, e.g. apply-patch < change.patch or a heredoc");
    }
    let mut input = Vec::new();
    if let Err(e) = stdin.lock().read_to_end(&mut input) {
        return fail(&format!("cannot read stdin: {e}"));
    }
    let changes = match parse_patch(&input) {
        Ok(c) => c,
        Err(e) => return fail(&format!("malformed patch: {e}; nothing written")),
    };
    let found: Vec<Found> = changes.iter().map(|c| find(Path::new(&c.path))).collect();
    let planned = match plan(&changes, &found) {
        Ok(p) => p,
        Err(refused) => {
            let mut err = io::stderr().lock();
            let _ = writeln!(
                err,
                "{NAME}: refused: {} of {} change{} cannot be made; nothing written, every file is as it was",
                refused.len(),
                changes.len(),
                if changes.len() == 1 { "" } else { "s" }
            );
            for r in &refused {
                let _ = err.write_all(r.as_bytes());
            }
            return ExitCode::from(EXIT_REFUSED);
        }
    };
    if !dry_run {
        let mut made = Vec::new();
        let written = stage(&planned, &mut made).and_then(|s| commit(s, &made));
        if let Err(e) = written {
            return fail(&e);
        }
    }
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    let res = (|| {
        for (i, p) in planned.iter().enumerate() {
            if i > 0 {
                out.write_all(b"\n")?;
            }
            out.write_all(&render(p))?;
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
        Ok(Asked::Apply { dry_run }) => run(dry_run),
        Err(e) => {
            eprintln!("{USAGE}");
            fail(&format!("{e}; see {NAME} --help"))
        }
    }
}
