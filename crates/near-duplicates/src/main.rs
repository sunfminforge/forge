//! near-duplicates: a detector of the trainer crew, in the form forge releases.
//!
//! The detector itself is `detector.py` beside this crate, embedded here as it
//! stands: this binary runs it with a python3 of the developer directory,
//! by its framework path (called as /usr/bin/python3 or
//! <developer dir>/usr/bin/python3, python3 writes an xcrun cache outside
//! the detector's sandbox, and the run is stopped). `-I`: nothing of the
//! environment's Python settings; `-B`: no bytecode written; `-c`: the
//! source runs as `__main__` and nothing is written to read it from.

use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::process::{Command, ExitCode};

const NAME: &str = "near-duplicates";
const SOURCE: &str = include_str!("../detector.py");
const FRAMEWORK: &str = "Library/Frameworks/Python3.framework/Versions/Current/bin/python3";

/// The python3s to try, in order: `GUILD_PYTHON3`, then the framework
/// python3 of the selected developer directory, of the Command Line Tools
/// and of Xcode, then Homebrew's.
fn candidates() -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(p) = std::env::var("GUILD_PYTHON3") {
        if !p.is_empty() {
            out.push(p);
        }
    }
    if let Ok(dev) = std::fs::read_link("/var/db/xcode_select_link") {
        out.push(format!("{}/{FRAMEWORK}", dev.display()));
    }
    out.push(format!("/Library/Developer/CommandLineTools/{FRAMEWORK}"));
    out.push(format!("/Applications/Xcode.app/Contents/Developer/{FRAMEWORK}"));
    out.push("/opt/homebrew/bin/python3".into());
    out.push("/usr/local/bin/python3".into());
    out
}

fn executable(path: &str) -> bool {
    std::fs::metadata(path)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    for py in candidates().iter().filter(|p| executable(p)) {
        // exec returns only when it could not run this one: try the next.
        let _ = Command::new(py).args(["-I", "-B", "-c", SOURCE]).args(&args).exec();
    }
    eprintln!("{NAME}: no python3 of a developer directory found");
    ExitCode::from(1)
}
