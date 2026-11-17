//! A stand-in for `guild`, put first on the PATH by the tests; it never
//! reaches a Hub.
//!
//! Its state is files named by environment variables: every call's
//! arguments are appended, one line each, space-joined, to
//! `FAKE_GUILD_CALLS`; `gate` prints the file `FAKE_GUILD_OUTPUT` as it is
//! (nothing when unset) and exits `FAKE_GUILD_EXIT` (0 when unset);
//! `--version` prints a version.

use std::io::Write;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Ok(path) = std::env::var("FAKE_GUILD_CALLS") {
        let mut calls = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .expect("the fake writes its calls");
        writeln!(calls, "{}", args.join(" ")).expect("the fake writes its calls");
    }
    match args.first().map(String::as_str) {
        Some("--version") => {
            println!("guild 0.0.0-fake");
            ExitCode::SUCCESS
        }
        Some("gate") => {
            if let Ok(path) = std::env::var("FAKE_GUILD_OUTPUT") {
                print!(
                    "{}",
                    std::fs::read_to_string(path).expect("the fake reads its output")
                );
            }
            let code: u8 = std::env::var("FAKE_GUILD_EXIT")
                .ok()
                .and_then(|c| c.parse().ok())
                .unwrap_or(0);
            ExitCode::from(code)
        }
        _ => {
            eprintln!("fake guild: no such command: {}", args.join(" "));
            ExitCode::from(2)
        }
    }
}
