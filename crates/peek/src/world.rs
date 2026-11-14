//! What peek does to the world: reads its arguments, stats each path,
//! reads a file or lists a directory read-only, and prints what lib
//! decides. It never writes a file.

use std::fs;
use std::io::{self, Write};
use std::process::ExitCode;

use crate::{
    dir_body, error_line, file_body, header, help, parse, Asked, Entry, EXIT_FAILED, NAME, USAGE,
};

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

/// What one path prints after its header, and whether it was read.
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

/// Prints every path in order; whether every one was read.
fn peek_all(paths: &[String]) -> io::Result<bool> {
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    let mut ok = true;
    for (i, path) in paths.iter().enumerate() {
        if i > 0 {
            out.write_all(b"\n")?;
        }
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
        Ok(Asked::Peek(paths)) => match peek_all(&paths) {
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
