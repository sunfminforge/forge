//! test-filters: what it decides, without touching the world. The
//! arguments it takes, where cargo is looked for, the counts read from
//! cargo test's output, and the one JSON object it answers. `world` finds
//! cargo, runs it once per filter, and prints.

pub mod world;

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

pub const NAME: &str = "test-filters";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Exit code when every filter ran at least one test and all passed.
pub const EXIT_OK: u8 = 0;
/// Exit code when the answer is given but some filter is not `passed`.
pub const EXIT_FINDING: u8 = 1;
/// Exit code of a refusal: nothing was answered per filter.
pub const EXIT_REFUSED: u8 = 2;

/// How many lines of cargo's stderr a fault or a `cargo_failed` filter
/// carries: enough to show the error, never a whole build log.
pub const STDERR_TAIL: usize = 20;

pub const USAGE: &str = "test-filters -p <package> <filter>...";

/// What `--help` answers.
pub fn help() -> String {
    format!(
        "usage: {USAGE}\n       {NAME} --version\n\n\
Runs the tests of one package of the cargo workspace in the current directory once per filter, with `cargo test -p <package> --no-fail-fast -- <filter>`, and answers one JSON object on stdout: per filter how many tests ran, passed, failed and were ignored. A filter matches a test whose full name contains it, as cargo test reads a filter (`counts` matches `tests::counts_two_filters`); unit, integration and doc tests are all counted. An empty match is a failure, never a pass: give one filter per thing you mean to check, such as one per acceptance item.\n\n\
Needs cargo on PATH or at $HOME/.cargo/bin/cargo; installs nothing and prompts for nothing. It first builds the package's tests once (`cargo test -p <package> --no-run`), so a build error is refused once, not once per filter.\n\n\
Answers, exit {EXIT_OK} when every filter's status is passed, exit {EXIT_FINDING} when any is not:\n\
{{\"package\": \"sample\", \"ok\": false, \"filters\": [{{\"filter\": \"adds\", \"ran\": 2, \"passed\": 2, \"failed\": 0, \"ignored\": 0, \"status\": \"passed\"}}, {{\"filter\": \"nothing_here\", \"ran\": 0, \"passed\": 0, \"failed\": 0, \"ignored\": 0, \"status\": \"empty\"}}]}}\n\n\
- ran: tests that ran (passed + failed); an ignored test is not run.\n\
- status: passed (ran 1 or more, none failed), failed (a test failed: passed < ran), empty (ran 0: the filter matches no test that runs), cargo_failed (cargo exited non-zero with no failed test, say a test binary that crashed; the filter also carries `stderr`, the last {STDERR_TAIL} lines cargo wrote).\n\
- ok: true exactly when every status is passed.\n\n\
Refuses, exit {EXIT_REFUSED}, with {{\"error\": \"<summary>\", \"faults\": [{{\"code\", \"message\", \"fix\"}}]}} and nothing per filter:\n\
- usage: no -p, no filter, a filter starting with -, or an argument it does not take; fix: the usage line.\n\
- cargo_not_found: cargo is neither on PATH nor at $HOME/.cargo/bin/cargo; fix: install Rust with rustup.\n\
- build_failed: the package's tests do not build, or the package is not in this workspace; the message carries cargo's last {STDERR_TAIL} lines of stderr; fix: the cargo command that shows the whole error.\n"
    )
}

/// What the arguments ask for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked {
    Help,
    Version,
    Run {
        package: String,
        filters: Vec<String>,
    },
}

/// Every fault of the arguments at once, or what they ask.
pub fn parse(args: &[String]) -> Result<Asked, Vec<String>> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        return Ok(Asked::Help);
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        return Ok(Asked::Version);
    }
    let mut faults = Vec::new();
    let mut package: Option<String> = None;
    let mut filters = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let value = if a == "-p" || a == "--package" {
            match it.next() {
                Some(v) if !v.starts_with('-') && !v.is_empty() => Some(v.clone()),
                _ => {
                    faults.push(format!("{a} needs a package name"));
                    continue;
                }
            }
        } else {
            a.strip_prefix("--package=")
                .filter(|v| !v.is_empty())
                .map(str::to_string)
        };
        if let Some(v) = value {
            if package.replace(v).is_some() {
                faults.push("-p is given more than once: name one package".into());
            }
        } else if a.starts_with('-') {
            faults.push(format!("{a} is not an argument test-filters takes"));
        } else if a.is_empty() {
            faults.push("an empty filter matches every test: name the test".into());
        } else {
            filters.push(a.clone());
        }
    }
    if package.is_none() {
        faults.push("no package: give -p <package>".into());
    }
    if filters.is_empty() {
        faults.push("no filter: give at least one".into());
    }
    match (faults.is_empty(), package) {
        (true, Some(package)) => Ok(Asked::Run { package, filters }),
        _ => Err(faults),
    }
}

/// Where cargo is: the first `cargo` on PATH, else $HOME/.cargo/bin/cargo.
/// `is_file` is the world's answer to whether a path is a file.
pub fn find_cargo(
    path: Option<&str>,
    home: Option<&str>,
    is_file: impl Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let on_path = path
        .into_iter()
        .flat_map(|p| p.split(':'))
        .filter(|d| !d.is_empty())
        .map(|d| Path::new(d).join("cargo"));
    let in_home = home
        .filter(|h| !h.is_empty())
        .map(|h| Path::new(h).join(".cargo/bin/cargo"));
    on_path.chain(in_home).find(|p| is_file(p))
}

/// Tests counted over every `test result:` line of one cargo test run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub passed: u64,
    pub failed: u64,
    pub ignored: u64,
}

/// One libtest summary line, as libtest writes it:
/// `test result: ok. 2 passed; 0 failed; 1 ignored; 0 measured; 3 filtered out; finished in 0.00s`.
fn summary(line: &str) -> Option<Counts> {
    let rest = line
        .strip_prefix("test result: ok. ")
        .or_else(|| line.strip_prefix("test result: FAILED. "))?;
    let mut parts = rest.split("; ");
    let mut count = |word: &str| -> Option<u64> {
        let (n, w) = parts.next()?.split_once(' ')?;
        (w == word).then_some(())?;
        n.parse().ok()
    };
    let passed = count("passed")?;
    let failed = count("failed")?;
    let ignored = count("ignored")?;
    Some(Counts {
        passed,
        failed,
        ignored,
    })
}

/// The counts of one run, summed over its test binaries (cargo runs each
/// unit, integration and doc test binary in turn, each with its summary);
/// a line counts only in libtest's whole summary shape.
pub fn counts(stdout: &str) -> Counts {
    stdout
        .lines()
        .filter_map(summary)
        .fold(Counts::default(), |a, c| Counts {
            passed: a.passed + c.passed,
            failed: a.failed + c.failed,
            ignored: a.ignored + c.ignored,
        })
}

/// The last `n` lines of `s`.
pub fn tail(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    lines[lines.len().saturating_sub(n)..].join("\n")
}

/// What one filter's run came to.
pub fn report(filter: &str, c: Counts, cargo_ok: bool, stderr: &str) -> Value {
    let ran = c.passed + c.failed;
    let status = if c.failed > 0 {
        "failed"
    } else if !cargo_ok {
        "cargo_failed"
    } else if ran == 0 {
        "empty"
    } else {
        "passed"
    };
    let mut v = json!({
        "filter": filter,
        "ran": ran,
        "passed": c.passed,
        "failed": c.failed,
        "ignored": c.ignored,
        "status": status,
    });
    if status == "cargo_failed" {
        v["stderr"] = Value::String(tail(stderr, STDERR_TAIL));
    }
    v
}

/// The answer over every filter's report, and its exit code.
pub fn answer(package: &str, reports: Vec<Value>) -> (Value, u8) {
    let ok = reports.iter().all(|r| r["status"] == "passed");
    let code = if ok { EXIT_OK } else { EXIT_FINDING };
    (
        json!({"package": package, "ok": ok, "filters": reports}),
        code,
    )
}

/// One fault of a refusal.
pub fn fault(code: &str, message: &str, fix: &str) -> Value {
    json!({"code": code, "message": message, "fix": fix})
}

/// A refusal over its faults.
pub fn refusal(summary: &str, faults: Vec<Value>) -> Value {
    json!({"error": summary, "faults": faults})
}

pub const FIX_RUSTUP: &str =
    "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   (or put cargo on PATH)";

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_package_and_filters() {
        assert_eq!(
            parse(&args(&["-p", "forge-read", "counts", "empty"])),
            Ok(Asked::Run {
                package: "forge-read".into(),
                filters: vec!["counts".into(), "empty".into()],
            })
        );
        assert_eq!(
            parse(&args(&["a", "--package=x"])),
            Ok(Asked::Run {
                package: "x".into(),
                filters: vec!["a".into()],
            })
        );
    }

    #[test]
    fn names_every_argument_fault_at_once() {
        let faults = parse(&args(&["--exact"])).unwrap_err();
        assert_eq!(faults.len(), 3, "{faults:?}");
    }

    #[test]
    fn reads_libtest_summaries() {
        let out = "running 2 tests\n\
test a ... ok\n\
test result: ok. 2 passed; 0 failed; 1 ignored; 0 measured; 3 filtered out; finished in 0.00s\n\
\n\
test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s\n\
test result: ok. lots passed\n";
        assert_eq!(
            counts(out),
            Counts {
                passed: 3,
                failed: 1,
                ignored: 1
            }
        );
    }

    #[test]
    fn looks_on_path_then_home() {
        let found = |want: &'static str| move |p: &Path| p == Path::new(want);
        assert_eq!(
            find_cargo(Some("/a:/b"), Some("/h"), found("/b/cargo")),
            Some("/b/cargo".into())
        );
        assert_eq!(
            find_cargo(Some("/a"), Some("/h"), found("/h/.cargo/bin/cargo")),
            Some("/h/.cargo/bin/cargo".into())
        );
        assert_eq!(find_cargo(None, None, |_| true), None);
    }
}
