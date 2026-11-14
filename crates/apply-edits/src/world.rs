//! What apply-edits does to the world: reads its arguments, the blocks on
//! stdin and the file, asks lib what the blocks do, and when every block
//! applied writes the file once (a tempfile beside it, persisted over it,
//! so a kill leaves the old file or the new, never half of one) and
//! prints the regions.

use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::Path;
use std::process::ExitCode;

use crate::{
    apply, fault_line, help, parse, parse_blocks, render, Asked, Outcome, EXIT_FAILED,
    EXIT_REFUSED, NAME, USAGE,
};

fn fail(msg: &str) -> ExitCode {
    eprintln!("{NAME}: error: {msg}");
    ExitCode::from(EXIT_FAILED)
}

/// Writes `bytes` over `path` at once, keeping its permissions: a
/// tempfile beside it, persisted (renamed) over it; the temp file is
/// removed on any failure.
fn write_over(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = match path.parent() {
        Some(d) if !d.as_os_str().is_empty() => d,
        _ => Path::new("."),
    };
    let mut tmp = tempfile::Builder::new()
        .prefix(".apply-edits.")
        .tempfile_in(dir)?;
    tmp.write_all(bytes)?;
    tmp.as_file().sync_all()?;
    fs::set_permissions(tmp.path(), fs::metadata(path)?.permissions())?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

fn run(file: &str, dry_run: bool) -> ExitCode {
    let stdin = io::stdin();
    if stdin.is_terminal() {
        return fail(&format!(
            "standard input is a terminal; send the blocks on stdin, e.g. apply-edits {file} < blocks.txt or a heredoc"
        ));
    }
    let mut input = Vec::new();
    if let Err(e) = stdin.lock().read_to_end(&mut input) {
        return fail(&format!("cannot read stdin: {e}"));
    }
    let path = Path::new(file);
    let original = match fs::metadata(path) {
        Ok(m) if m.is_dir() => {
            return fail(&format!("{file} is a directory; name one file to edit"))
        }
        Ok(_) => match fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                return fail(&format!(
                    "cannot read {file}: {e}; name a file you may read and write"
                ))
            }
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return fail(&format!(
                "no such file: {file}; check the name with peek {}, relative to the current directory or absolute (apply-edits edits files, it does not create them)",
                path.parent()
                    .filter(|p| !p.as_os_str().is_empty())
                    .map_or(".".into(), |p| p.display().to_string())
            ))
        }
        Err(e) => return fail(&format!("cannot read {file}: {e}")),
    };
    let blocks = match parse_blocks(&input) {
        Ok(b) => b,
        Err(e) => return fail(&format!("malformed blocks: {e}; nothing written")),
    };
    match apply(&original, &blocks) {
        Outcome::Refused(faults) => {
            let mut err = io::stderr().lock();
            let _ = writeln!(
                err,
                "{NAME}: refused: {} of {} block{} did not match exactly once; nothing written, {file} is as it was",
                faults.len(),
                blocks.len(),
                if blocks.len() == 1 { "" } else { "s" }
            );
            for f in &faults {
                let _ = err.write_all(fault_line(file, f).as_bytes());
            }
            ExitCode::from(EXIT_REFUSED)
        }
        Outcome::Applied { text, blocks } => {
            if !dry_run && text != original {
                if let Err(e) = write_over(path, &text) {
                    return fail(&format!(
                        "cannot write {file}: {e}; nothing written, it is as it was; make it and its directory writable"
                    ));
                }
            }
            let stdout = io::stdout();
            let mut out = io::BufWriter::new(stdout.lock());
            let res = (|| {
                for (i, a) in blocks.iter().enumerate() {
                    if i > 0 {
                        out.write_all(b"\n")?;
                    }
                    out.write_all(&render(&text, a))?;
                }
                out.flush()
            })();
            if dry_run {
                eprintln!("{NAME}: dry run: {file} not written");
            }
            match res {
                Ok(()) => ExitCode::SUCCESS,
                Err(e) => fail(&format!("cannot write stdout: {e}")),
            }
        }
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
                    "argument {} is not UTF-8; name the file by a UTF-8 name; see {NAME} --help",
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
        Ok(Asked::Apply { file, dry_run }) => run(&file, dry_run),
        Err(e) => {
            eprintln!("{USAGE}");
            fail(&format!("{e}; see {NAME} --help"))
        }
    }
}
