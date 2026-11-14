//! What test-filters does to the world: reads its arguments and the
//! environment, runs cargo in the current directory, and prints. The one
//! module that builds a cargo command line.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Output};

use serde_json::Value;

use crate::{
    answer, counts, fault, find_cargo, help, parse, refusal, report, tail, Asked, EXIT_OK,
    EXIT_REFUSED, FIX_RUSTUP, NAME, STDERR_TAIL, USAGE, VERSION,
};

fn print(v: &Value, code: u8) -> ExitCode {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{v}");
    ExitCode::from(code)
}

fn refuse(summary: &str, faults: Vec<Value>) -> ExitCode {
    print(&refusal(summary, faults), EXIT_REFUSED)
}

/// `cargo test -p <package> --color never --no-fail-fast <extra>…`, run
/// to its end with both streams read.
fn cargo_test(cargo: &Path, package: &str, extra: &[&str]) -> std::io::Result<Output> {
    Command::new(cargo)
        .args(["test", "-p", package, "--color", "never", "--no-fail-fast"])
        .args(extra)
        .env_remove("CARGO_TERM_COLOR")
        .output()
}

pub fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (package, filters) = match parse(&args) {
        Ok(Asked::Help) => {
            print!("{}", help());
            return ExitCode::from(EXIT_OK);
        }
        Ok(Asked::Version) => {
            println!("{NAME} {VERSION}");
            return ExitCode::from(EXIT_OK);
        }
        Ok(Asked::Run { package, filters }) => (package, filters),
        Err(faults) => {
            let faults = faults.iter().map(|m| fault("usage", m, USAGE)).collect();
            return refuse("the arguments are not what test-filters takes", faults);
        }
    };

    let path = std::env::var("PATH").ok();
    let home = std::env::var("HOME").ok();
    let Some(cargo): Option<PathBuf> =
        find_cargo(path.as_deref(), home.as_deref(), |p| p.is_file())
    else {
        return refuse(
            "cargo not found",
            vec![fault(
                "cargo_not_found",
                "cargo is neither on PATH nor at $HOME/.cargo/bin/cargo",
                FIX_RUSTUP,
            )],
        );
    };

    // Build once, so a build error is one refusal, not one per filter.
    let fix_build = format!("cargo test -p {package} --no-run");
    match cargo_test(&cargo, &package, &["--no-run"]) {
        Err(e) => {
            return refuse(
                "cargo could not be run",
                vec![fault(
                    "build_failed",
                    &format!("{}: {e}", cargo.display()),
                    &fix_build,
                )],
            )
        }
        Ok(o) if !o.status.success() => {
            return refuse(
                "the package's tests do not build",
                vec![fault(
                    "build_failed",
                    &tail(&String::from_utf8_lossy(&o.stderr), STDERR_TAIL),
                    &fix_build,
                )],
            )
        }
        Ok(_) => {}
    }

    let mut reports = Vec::new();
    for filter in &filters {
        let r = match cargo_test(&cargo, &package, &["--", filter, "--color", "never"]) {
            Ok(o) => report(
                filter,
                counts(&String::from_utf8_lossy(&o.stdout)),
                o.status.success(),
                &String::from_utf8_lossy(&o.stderr),
            ),
            Err(e) => report(filter, Default::default(), false, &e.to_string()),
        };
        reports.push(r);
    }
    let (v, code) = answer(&package, reports);
    print(&v, code)
}
