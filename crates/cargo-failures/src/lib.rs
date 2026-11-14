//! cargo-failures: what it decides, without touching the world. The
//! arguments it takes, the cargo command line it runs, where it looks for
//! cargo, which of cargo's output lines it keeps, and the exit code it
//! answers with. `world` finds and runs cargo and prints.

pub mod world;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

pub const NAME: &str = "cargo-failures";

/// Exit code of an argument cargo-failures does not take.
pub const EXIT_USAGE: u8 = 2;
/// Exit code when cargo is found nowhere or cannot be started.
pub const EXIT_NO_CARGO: u8 = 127;
/// A cargo killed by a signal exits this plus the signal's number, as a
/// shell reports it; never 0.
pub const EXIT_SIGNAL_BASE: u8 = 128;

/// The flag cargo-failures puts after the subcommand: rustc's diagnostics
/// as JSON on stdout, one object per line, beside libtest's own text.
pub const MESSAGE_FORMAT: &str = "--message-format=json";

pub const USAGE: &str = "usage: cargo-failures build|test [cargo args…]";

/// What `--help` answers, before its exit codes are put in by `help`.
const HELP: &str = concat!(
    "usage: cargo-failures build [cargo args…]\n",
    "       cargo-failures test [cargo args…] [-- <test binary args>…]\n",
    "       cargo-failures --version\n\n",
    "Runs `cargo build` or `cargo test` once, in the current directory's Rust workspace, with the arguments given after the subcommand passed to cargo as they are, and prints plain text, never JSON. It prints only what explains a failure: each compile error, each failing test with its panic message, and every 'test result:' line. Warnings, progress and passing tests are dropped. It exits with cargo's own exit code: 0 when cargo succeeded, cargo's 101 when a compile or a test failed.\n\n",
    "commands:\n",
    "  build [cargo args…]       cargo build; prints each compile error\n",
    "  test [cargo args…]        cargo test; prints each compile error, then for each test binary its failing tests and its 'test result:' line\n\n",
    "What it prints, in cargo's order, on stdout:\n",
    "  a compile error           rustc's error as rustc renders it: error[<code>]: <message>, then --> <file>:<line>:<column> and the source it points at; each error once, though cargo test compiles a crate twice\n",
    "  a failing test           ---- <test name> stdout ----, then its panic: thread '<test name>' panicked at <file>:<line>:<column>: and the message; the whole captured output when it did not panic\n",
    "  a test result             test result: ok. <n> passed; … or test result: FAILED. …, one line per test binary and doc-test run\n\n",
    "Cargo is found on PATH, else in $CARGO_HOME/bin, else in ~/.cargo/bin. cargo-failures adds --message-format=json after the subcommand to read rustc's diagnostics; do not pass --message-format yourself. Do not pass -- --nocapture: a test's output then bypasses libtest's capture and its panic message is lost.\n\n",
    "When cargo fails and nothing printed explains it (a manifest cargo refuses, a package that does not exist, a build script that failed, a test binary that crashed), cargo's own stderr is passed on whole, on stderr.\n\n",
    "Exits with cargo's exit code, unchanged. Exits {EXIT_NO_CARGO} with the reason on stderr when cargo is found nowhere or cannot be started; {EXIT_SIGNAL_BASE} plus the signal's number with the reason on stderr when cargo is killed by a signal; {EXIT_USAGE} with the usage on stderr on an argument it does not take.\n",
);

/// What `--help` answers, its exit codes as the constants say.
pub fn help() -> String {
    HELP.replace("{EXIT_NO_CARGO}", &EXIT_NO_CARGO.to_string())
        .replace("{EXIT_SIGNAL_BASE}", &EXIT_SIGNAL_BASE.to_string())
        .replace("{EXIT_USAGE}", &EXIT_USAGE.to_string())
}

/// The cargo subcommands cargo-failures runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Sub {
    Build,
    Test,
}

impl Sub {
    pub fn name(self) -> &'static str {
        match self {
            Sub::Build => "build",
            Sub::Test => "test",
        }
    }
}

/// What the arguments ask.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked {
    Help,
    Version,
    Run { sub: Sub, rest: Vec<String> },
}

/// The arguments, after the program's name. Past the subcommand every
/// argument is cargo's, `--help` included, except a `--message-format`
/// before `--`, which would contradict the one cargo-failures adds.
pub fn parse(args: &[String]) -> Result<Asked, String> {
    let Some((first, rest)) = args.split_first() else {
        return Err("a subcommand is required: build or test".into());
    };
    let sub = match first.as_str() {
        "-h" | "--help" | "help" => return Ok(Asked::Help),
        "-V" | "--version" if rest.is_empty() => return Ok(Asked::Version),
        "build" => Sub::Build,
        "test" => Sub::Test,
        other => {
            return Err(format!(
                "no such subcommand: {other}; the subcommands are build and test"
            ))
        }
    };
    let cargo_part = rest.iter().take_while(|a| a.as_str() != "--");
    if let Some(a) = cargo_part
        .into_iter()
        .find(|a| a.as_str() == "--message-format" || a.starts_with("--message-format="))
    {
        return Err(format!(
            "{a} is not taken: cargo-failures sets --message-format=json itself; drop it"
        ));
    }
    Ok(Asked::Run {
        sub,
        rest: rest.to_vec(),
    })
}

/// The arguments cargo is run with: the subcommand, the message format,
/// then the caller's as given, so a `--` and the test arguments after it
/// stay where they were.
pub fn cargo_args(sub: Sub, rest: &[String]) -> Vec<String> {
    let mut out = vec![sub.name().to_string(), MESSAGE_FORMAT.to_string()];
    out.extend(rest.iter().cloned());
    out
}

/// Where cargo is looked for, in order: each directory of PATH, then
/// $CARGO_HOME/bin, then $HOME/.cargo/bin. The first that holds an
/// executable `cargo` is run.
pub fn cargo_candidates(
    path: Option<&str>,
    cargo_home: Option<&str>,
    home: Option<&str>,
) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = path
        .unwrap_or_default()
        .split(':')
        .filter(|d| !d.is_empty())
        .map(|d| Path::new(d).join("cargo"))
        .collect();
    if let Some(h) = cargo_home.filter(|h| !h.is_empty()) {
        out.push(Path::new(h).join("bin").join("cargo"));
    }
    if let Some(h) = home.filter(|h| !h.is_empty()) {
        out.push(Path::new(h).join(".cargo").join("bin").join("cargo"));
    }
    out
}

/// What cargo-failures exits with, from how cargo ended: its code, or the
/// signal that killed it. With the one-line reason to say on stderr when
/// the code is not cargo's own.
pub fn exit_for(code: Option<i32>, signal: Option<i32>) -> (u8, Option<String>) {
    match (code, signal) {
        (Some(c), _) => match u8::try_from(c) {
            Ok(c) => (c, None),
            // Not on unix, where a code is 0 to 255; never 0 elsewhere.
            Err(_) => (1, Some(format!("cargo exited {c}, past 255; exiting 1"))),
        },
        (None, Some(s)) => {
            let code = u8::try_from(s)
                .ok()
                .and_then(|s| EXIT_SIGNAL_BASE.checked_add(s))
                .filter(|c| *c != 0)
                .unwrap_or(255);
            (
                code,
                Some(format!("cargo was killed by signal {s}; exiting {code}")),
            )
        }
        (None, None) => (
            1,
            Some("cargo ended without an exit code or a signal; exiting 1".into()),
        ),
    }
}

/// Where the filter is in libtest's text.
#[derive(Debug)]
enum At {
    /// Outside any failing test's captured output.
    Out,
    /// Inside `---- <name> stdout ----`: its header and the lines so far.
    Section { header: String, lines: Vec<String> },
}

/// Keeps, of cargo's stdout read a line at a time, the compile errors,
/// the failing tests with their panic messages and the 'test result:'
/// lines, and says what to print as each line arrives.
#[derive(Debug)]
pub struct Filter {
    at: At,
    /// Rendered errors already printed: cargo test compiles a crate twice.
    seen: HashSet<String>,
    /// Failing tests of the current test binary, by name, from their
    /// `test <name> ... FAILED` line, not yet printed.
    unprinted: Vec<String>,
    errors: usize,
    failed: usize,
    running: usize,
    results: usize,
}

impl Default for Filter {
    fn default() -> Self {
        Filter::new()
    }
}

impl Filter {
    pub fn new() -> Filter {
        Filter {
            at: At::Out,
            seen: HashSet::new(),
            unprinted: Vec::new(),
            errors: 0,
            failed: 0,
            running: 0,
            results: 0,
        }
    }

    /// One line of cargo's stdout, without its newline: what to print for
    /// it, each item ending in a newline, possibly nothing.
    pub fn feed(&mut self, line: &str) -> String {
        if let Some(message) = cargo_message(line) {
            return self.message(&message);
        }
        self.text(line)
    }

    /// Whether what was printed explains a cargo that failed: some compile
    /// error or failing test, and every test binary that started running
    /// reached its 'test result:' line. When not, cargo's stderr is passed
    /// on.
    pub fn explains_failure(&self) -> bool {
        (self.errors > 0 || self.failed > 0) && self.running <= self.results
    }

    /// What is left to print once cargo's stdout has ended: a failing
    /// test's captured output that never reached its end.
    pub fn finish(&mut self) -> String {
        match std::mem::replace(&mut self.at, At::Out) {
            At::Section { header, lines } => self.section(&header, &lines),
            At::Out => String::new(),
        }
    }

    fn message(&mut self, m: &Value) -> String {
        if m["reason"] != "compiler-message" {
            return String::new();
        }
        let d = &m["message"];
        let level = d["level"].as_str().unwrap_or_default();
        let text = d["message"].as_str().unwrap_or_default();
        // "error" and "error: internal compiler error"; a "failure-note" or
        // a warning is dropped, and so is rustc's closing count.
        if !level.starts_with("error") || text.starts_with("aborting due to") {
            return String::new();
        }
        let rendered = d["rendered"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| format!("{level}: {text}"));
        let rendered = rendered.trim_end().to_string();
        if !self.seen.insert(rendered.clone()) {
            return String::new();
        }
        self.errors += 1;
        format!("{rendered}\n\n")
    }

    fn text(&mut self, line: &str) -> String {
        let mut out = String::new();
        if let At::Section { .. } = self.at {
            if line == "failures:" || line.starts_with("test result: ") || section_header(line) {
                out.push_str(&self.finish());
            } else if let At::Section { lines, .. } = &mut self.at {
                lines.push(line.to_string());
                return out;
            }
        }
        if section_header(line) {
            self.at = At::Section {
                header: line.to_string(),
                lines: Vec::new(),
            };
        } else if let Some(rest) = line.strip_prefix("test result: ") {
            // A failing test that printed no captured output still shows.
            for name in std::mem::take(&mut self.unprinted) {
                out.push_str(&format!("---- {name} ----\n\n"));
            }
            self.results += 1;
            out.push_str(&format!("test result: {rest}\n"));
        } else if let Some(name) = failed_name(line) {
            self.failed += 1;
            self.unprinted.push(name.to_string());
        } else if is_running(line) {
            self.running += 1;
        }
        out
    }

    /// A failing test's section: its header, then from its panic on (its
    /// own output before the panic dropped), or all of it when it did not
    /// panic; without libtest's backtrace note.
    fn section(&mut self, header: &str, lines: &[String]) -> String {
        let name = header
            .strip_prefix("---- ")
            .and_then(|h| h.strip_suffix(" ----"))
            .and_then(|h| h.rsplit_once(' '))
            .map(|(n, _)| n)
            .unwrap_or_default();
        self.unprinted.retain(|n| n != name);
        let start = lines
            .iter()
            .position(|l| l.starts_with("thread '") && l.contains(" panicked at "))
            .unwrap_or(0);
        let body: Vec<&str> = lines[start..]
            .iter()
            .map(String::as_str)
            .filter(|l| !l.starts_with("note: run with `RUST_BACKTRACE="))
            .collect();
        let end = body
            .iter()
            .rposition(|l| !l.trim().is_empty())
            .map_or(0, |i| i + 1);
        let mut out = format!("{header}\n");
        for l in &body[..end] {
            out.push_str(l);
            out.push('\n');
        }
        out.push('\n');
        out
    }
}

/// A line of cargo's own JSON: an object with a `reason`. A test's
/// printed line that happens to be JSON has no `reason` cargo gives.
fn cargo_message(line: &str) -> Option<Value> {
    if !line.starts_with('{') {
        return None;
    }
    let v: Value = serde_json::from_str(line).ok()?;
    v.get("reason")?.as_str()?;
    Some(v)
}

/// `---- <name> stdout ----` (or stderr), libtest's header of a failing
/// test's captured output.
fn section_header(line: &str) -> bool {
    line.strip_prefix("---- ")
        .and_then(|l| l.strip_suffix(" ----"))
        .is_some_and(|l| l.ends_with(" stdout") || l.ends_with(" stderr"))
}

/// The name in `test <name> ... FAILED`.
fn failed_name(line: &str) -> Option<&str> {
    line.strip_prefix("test ")?.strip_suffix(" ... FAILED")
}

/// `running <n> test(s)`, libtest's first line of a test binary.
fn is_running(line: &str) -> bool {
    line.strip_prefix("running ")
        .and_then(|l| l.strip_suffix(" tests").or_else(|| l.strip_suffix(" test")))
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn parses_and_refuses() {
        assert_eq!(parse(&s(&["--help"])), Ok(Asked::Help));
        assert_eq!(parse(&s(&["--version"])), Ok(Asked::Version));
        assert_eq!(
            parse(&s(&["test", "-p", "x", "--", "--exact", "--help"])),
            Ok(Asked::Run {
                sub: Sub::Test,
                rest: s(&["-p", "x", "--", "--exact", "--help"])
            })
        );
        assert!(parse(&[]).is_err());
        assert!(parse(&s(&["check"]))
            .unwrap_err()
            .contains("build and test"));
        assert!(parse(&s(&["build", "--message-format", "short"]))
            .unwrap_err()
            .contains("drop it"));
        assert!(parse(&s(&["test", "--", "--message-format=x"])).is_ok());
    }

    #[test]
    fn cargo_args_put_the_format_after_the_subcommand() {
        assert_eq!(
            cargo_args(Sub::Test, &s(&["-p", "a", "--", "x"])),
            s(&["test", "--message-format=json", "-p", "a", "--", "x"])
        );
    }

    #[test]
    fn candidates_in_order() {
        assert_eq!(
            cargo_candidates(Some("/a::/b"), Some("/ch"), Some("/home/u")),
            vec![
                PathBuf::from("/a/cargo"),
                PathBuf::from("/b/cargo"),
                PathBuf::from("/ch/bin/cargo"),
                PathBuf::from("/home/u/.cargo/bin/cargo"),
            ]
        );
        assert_eq!(
            cargo_candidates(None, Some(""), Some("/h")),
            vec![PathBuf::from("/h/.cargo/bin/cargo")]
        );
    }

    #[test]
    fn exit_codes() {
        assert_eq!(exit_for(Some(0), None), (0, None));
        assert_eq!(exit_for(Some(101), None), (101, None));
        assert_eq!(exit_for(None, Some(9)).0, 137);
        assert!(exit_for(None, Some(9)).1.unwrap().contains("signal 9"));
        assert_eq!(exit_for(None, Some(200)).0, 255);
        assert_ne!(exit_for(Some(256), None).0, 0);
        assert_ne!(exit_for(None, None).0, 0);
    }

    #[test]
    fn a_failing_test_without_output_is_named() {
        let mut f = Filter::new();
        let mut out = String::new();
        for l in [
            "running 1 test",
            "test t ... FAILED",
            "",
            "failures:",
            "    t",
            "",
            "test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s",
        ] {
            out.push_str(&f.feed(l));
        }
        assert_eq!(
            out,
            "---- t ----\n\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n"
        );
        assert!(f.explains_failure());
    }

    #[test]
    fn a_failing_test_prints_once_from_its_panic() {
        let mut f = Filter::new();
        let mut out = String::new();
        for l in [
            "running 1 test",
            "test m::t ... FAILED",
            "",
            "failures:",
            "",
            "---- m::t stdout ----",
            "its own output",
            "",
            "thread 'm::t' (42) panicked at src/lib.rs:3:5:",
            "boom",
            "note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace",
            "",
            "",
            "failures:",
            "    m::t",
            "",
            "test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s",
        ] {
            out.push_str(&f.feed(l));
        }
        assert_eq!(
            out,
            "---- m::t stdout ----\nthread 'm::t' (42) panicked at src/lib.rs:3:5:\nboom\n\ntest result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s\n"
        );
    }

    #[test]
    fn a_crashed_binary_does_not_explain() {
        let mut f = Filter::new();
        f.feed("running 2 tests");
        f.feed("test a ... FAILED");
        assert!(!f.explains_failure());
    }
}
