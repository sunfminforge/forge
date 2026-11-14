//! What read-files does to the world: reads its arguments, opens each
//! file read-only, prints what lib decides and says each failure on
//! stderr. It never writes a file.

use std::fs::File;
use std::io::{self, BufRead, BufReader, Write};
use std::process::ExitCode;

use crate::{
    fit, fit_failure, header, help, number, parse, Asked, Item, Spec, EXIT_FAILED, EXIT_USAGE,
    NAME, USAGE,
};

/// Says one argument's failure on stderr, after what stdout holds so far.
fn say(out: &mut impl Write, arg: &str, reason: &str) {
    let _ = out.flush();
    eprintln!("{NAME}: {arg}: {reason}");
}

/// Opens a file to read, refusing a directory.
fn open(path: &str) -> Result<BufReader<File>, String> {
    let fix =
        "name a file that exists and you may read, relative to the current directory or absolute";
    let file = File::open(path).map_err(|e| format!("cannot open: {e}; {fix}"))?;
    match file.metadata() {
        Ok(m) if m.is_dir() => Err(format!("is a directory; {fix}")),
        _ => Ok(BufReader::new(file)),
    }
}

/// Writes one numbered line, adding the newline a last line may lack.
fn line(out: &mut impl Write, n: u64, bytes: &[u8]) -> io::Result<()> {
    out.write_all(number(n).as_bytes())?;
    out.write_all(bytes)?;
    if bytes.last() != Some(&b'\n') {
        out.write_all(b"\n")?;
    }
    Ok(())
}

/// Prints one argument: whether it printed its header, and the failure
/// to say on stderr. `first` is whether no header was printed before it.
fn print(out: &mut impl Write, spec: &Spec, first: bool) -> (bool, Option<String>) {
    let mut reader = match open(&spec.path) {
        Ok(r) => r,
        Err(e) => return (false, Some(e)),
    };
    match body(out, spec, first, &mut reader) {
        Ok((printed, failure)) => (printed, failure),
        Err(e) => (true, Some(e)),
    }
}

/// Prints an opened file. `Ok` holds whether its header was printed and
/// the range's failure; `Err` a read or write failure after the header.
fn body(
    out: &mut impl Write,
    spec: &Spec,
    first: bool,
    reader: &mut impl BufRead,
) -> Result<(bool, Option<String>), String> {
    let read_err = |e: io::Error| format!("cannot read: {e}; name a file you may read");
    let write_err = |e: io::Error| format!("cannot write stdout: {e}");
    let lead = |out: &mut dyn Write| -> io::Result<()> {
        if !first {
            out.write_all(b"\n")?;
        }
        out.write_all(header(&spec.arg).as_bytes())
    };
    let mut buf = Vec::new();
    let mut n: u64 = 0;
    let Some(range) = spec.range else {
        lead(out).map_err(write_err)?;
        loop {
            buf.clear();
            if reader.read_until(b'\n', &mut buf).map_err(read_err)? == 0 {
                return Ok((true, None));
            }
            n += 1;
            line(out, n, &buf).map_err(write_err)?;
        }
    };
    // The lines asked for are held until the file shows whether the
    // range starts within it, so a range past the end prints nothing.
    let mut held: Vec<Vec<u8>> = Vec::new();
    loop {
        buf.clear();
        if reader.read_until(b'\n', &mut buf).map_err(read_err)? == 0 {
            break;
        }
        n += 1;
        if range.holds(n) {
            held.push(buf.clone());
        }
        if n == range.end() {
            break;
        }
    }
    let fitted = fit(range, n);
    if !held.is_empty() {
        lead(out).map_err(write_err)?;
        for (i, l) in held.iter().enumerate() {
            line(out, range.start() + i as u64, l).map_err(write_err)?;
        }
    }
    Ok((!held.is_empty(), fit_failure(&spec.path, range, fitted)))
}

/// Prints every argument in order; whether every one printed whole.
fn read_all(items: &[Item]) -> bool {
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    let mut ok = true;
    let mut first = true;
    for it in items {
        match it {
            Item::Refused { arg, reason } => {
                ok = false;
                say(&mut out, arg, reason);
            }
            Item::Read(spec) => {
                let (printed, failure) = print(&mut out, spec, first);
                if let Some(reason) = failure {
                    ok = false;
                    say(&mut out, &spec.arg, &reason);
                }
                first = first && !printed;
            }
        }
    }
    if out.flush().is_err() {
        ok = false;
    }
    ok
}

pub fn main() -> ExitCode {
    let mut args = Vec::new();
    for a in std::env::args_os().skip(1) {
        match a.into_string() {
            Ok(s) => args.push(s),
            Err(a) => {
                eprintln!(
                    "{USAGE}\n{NAME}: error: argument {} is not UTF-8; name the file by a UTF-8 path; see {NAME} --help",
                    a.to_string_lossy()
                );
                return ExitCode::from(EXIT_USAGE);
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
        Ok(Asked::Read(items)) => {
            if read_all(&items) {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(EXIT_FAILED)
            }
        }
        Err(e) => {
            eprintln!("{USAGE}\n{NAME}: error: {e}; see {NAME} --help");
            ExitCode::from(EXIT_USAGE)
        }
    }
}
