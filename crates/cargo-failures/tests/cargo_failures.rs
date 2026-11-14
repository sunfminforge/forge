//! cargo-failures' binary run on fixture workspaces with the real cargo:
//! one that compiles clean and passes, one with a compile error (and a
//! warning), one with a failing test that panics (and a passing test that
//! prints). Then against stand-in cargos on PATH: where cargo is looked
//! for, a cargo killed by a signal, no cargo at all, and each refusal.
//! Each test prints what the binary printed, seen with `-- --nocapture`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_cargo-failures");

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A fresh directory of this test run's own.
fn scratch(name: &str) -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("cargo-failures")
        .join(format!("{name}-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        let target = to.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &target);
        } else {
            fs::copy(e.path(), target).unwrap();
        }
    }
}

/// A fixture workspace copied out of the source tree, so cargo's lock
/// file and build land in the scratch directory, never beside the source.
fn fixture(name: &str) -> PathBuf {
    let dir = scratch(name);
    let ws = dir.join("ws");
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
        &ws,
    );
    ws
}

fn show(o: &Output) {
    println!(
        "--- stdout:\n{}--- stderr:\n{}--- exit: {:?}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr),
        o.status.code()
    );
}

/// cargo-failures with the real cargo, in a fixture workspace.
fn on_fixture(name: &str, args: &[&str]) -> (String, String, i32) {
    let ws = fixture(name);
    let o = Command::new(BIN)
        .args(args)
        .current_dir(&ws)
        .env("CARGO_TARGET_DIR", ws.join("target"))
        .env_remove("CARGO_TERM_COLOR")
        .output()
        .unwrap();
    show(&o);
    (
        String::from_utf8(o.stdout).unwrap(),
        String::from_utf8(o.stderr).unwrap(),
        o.status.code().expect("exited, not killed"),
    )
}

/// libtest's text with what changes from run to run taken out: the time
/// after `finished in` and the thread id rustc puts after a test's name.
fn steady(s: &str) -> String {
    s.lines()
        .map(|l| {
            let l = match l.find("; finished in ") {
                Some(i) => format!("{}; finished in <t>", &l[..i]),
                None => l.to_string(),
            };
            match (
                l.starts_with("thread '"),
                l.find("' ("),
                l.find(") panicked at "),
            ) {
                (true, Some(a), Some(b)) if a < b => format!("{}'{}", &l[..a], &l[b + 1..]),
                _ => l,
            }
        })
        .map(|l| l + "\n")
        .collect()
}

#[test]
fn clean_test_prints_only_its_test_result_lines() {
    let (out, _, code) = on_fixture("clean", &["test", "--offline"]);
    assert_eq!(code, 0);
    assert_eq!(
        steady(&out),
        concat!(
            "test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in <t>\n",
            "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in <t>\n",
        )
    );
}

#[test]
fn clean_build_prints_nothing() {
    let (out, _, code) = on_fixture("clean", &["build"]);
    assert_eq!(code, 0);
    assert_eq!(out, "");
}

const E1: &str = concat!("error[E0308]: mismatched types\n", " --> src/lib.rs:7:21\n",);
const E2: &str = concat!("error[E0308]: mismatched types\n", " --> src/lib.rs:8:5\n",);

fn assert_compile_errors(out: &str) {
    assert!(out.contains(E1), "first error with its file:line");
    assert!(out.contains(E2), "second error with its file:line");
    assert!(out.find(E1) < out.find(E2), "in rustc's order");
    assert_eq!(out.matches("error[E0308]").count(), 2, "each error once");
    assert!(!out.contains("warning"), "no warning");
    assert!(!out.contains("unused"), "no warning");
    assert!(!out.contains("rustc --explain"), "no failure note");
    assert!(!out.contains("test result:"), "no test ran");
}

#[test]
fn compile_error_on_test_prints_each_error_once_and_exits_101() {
    let (out, _, code) = on_fixture("compile-error", &["test"]);
    assert_compile_errors(&out);
    assert_eq!(code, 101);
}

#[test]
fn compile_error_on_build_prints_each_error_and_exits_101() {
    let (out, _, code) = on_fixture("compile-error", &["build"]);
    assert_compile_errors(&out);
    assert_eq!(code, 101);
}

const FAILURE: &str = concat!(
    "---- tests::adds_wrong stdout ----\n",
    "thread 'tests::adds_wrong' panicked at src/lib.rs:17:9:\n",
    "assertion `left == right` failed: two and two\n",
    "  left: 4\n",
    " right: 5\n",
    "\n",
);

#[test]
fn failing_test_prints_its_panic_and_test_result_and_exits_101() {
    let (out, _, code) = on_fixture("failing-test", &["test"]);
    assert_eq!(
        steady(&out),
        format!("{FAILURE}test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in <t>\n")
    );
    assert!(!out.contains("printed"), "nothing of a test's own output");
    assert!(!out.contains("tests::adds "), "nothing of a passing test");
    assert_eq!(code, 101);
}

#[test]
fn failing_test_with_no_fail_fast_prints_every_test_result_line() {
    let (out, _, code) = on_fixture("failing-test", &["test", "--no-fail-fast"]);
    assert_eq!(
        steady(&out),
        format!(
            "{FAILURE}{}{}",
            "test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in <t>\n",
            "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in <t>\n",
        )
    );
    assert_eq!(code, 101);
}

#[test]
fn failing_test_args_after_the_double_dash_reach_libtest() {
    let (out, _, code) = on_fixture("failing-test", &["test", "--", "--exact", "tests::adds"]);
    assert_eq!(
        steady(&out),
        concat!(
            "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out; finished in <t>\n",
            "test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in <t>\n",
        )
    );
    assert_eq!(code, 0);
}

#[test]
fn a_failure_nothing_explains_passes_cargo_stderr_on() {
    let (out, err, code) = on_fixture("clean", &["test", "-p", "no-such-package"]);
    assert_eq!(out, "");
    assert!(err.contains("no-such-package"), "cargo's own reason");
    assert_eq!(code, 101);
}

// Stand-in cargos.

/// An executable shell script at `dir/cargo`: it records its arguments in
/// `dir/args`, then runs `body`.
fn stand_in(dir: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(dir).unwrap();
    let p = dir.join("cargo");
    fs::write(
        &p,
        format!(
            "#!/bin/sh\necho \"$@\" > '{}'\n{body}\n",
            dir.join("args").display()
        ),
    )
    .unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
}

/// cargo-failures with only these as its PATH, CARGO_HOME and HOME.
fn with_env(args: &[&str], path: &Path, cargo_home: Option<&Path>, home: &Path) -> Output {
    let mut c = Command::new(BIN);
    c.args(args)
        .env_clear()
        .env("PATH", path)
        .env("HOME", home)
        .current_dir(home);
    if let Some(h) = cargo_home {
        c.env("CARGO_HOME", h);
    }
    let o = c.output().unwrap();
    show(&o);
    o
}

const RESULT: &str =
    "test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s";

#[test]
fn cargo_on_path_is_run_with_the_json_format_and_the_args_as_given() {
    let d = scratch("path");
    stand_in(&d.join("bin"), &format!("echo '{RESULT}'\nexit 3"));
    let o = with_env(
        &["test", "-p", "x", "--", "--exact"],
        &d.join("bin"),
        None,
        &d,
    );
    assert_eq!(String::from_utf8_lossy(&o.stdout), format!("{RESULT}\n"));
    assert_eq!(o.status.code(), Some(3), "cargo's code, unchanged");
    assert_eq!(
        fs::read_to_string(d.join("bin/args")).unwrap(),
        "test --message-format=json -p x -- --exact\n"
    );
}

#[test]
fn cargo_not_on_path_is_found_in_cargo_home_bin() {
    let d = scratch("cargo-home");
    fs::create_dir_all(d.join("empty")).unwrap();
    // On PATH but not executable: passed over.
    fs::write(d.join("empty/cargo"), "not a program").unwrap();
    stand_in(&d.join("ch/bin"), &format!("echo '{RESULT}'"));
    stand_in(&d.join(".cargo/bin"), "exit 9");
    let o = with_env(&["build"], &d.join("empty"), Some(&d.join("ch")), &d);
    assert_eq!(String::from_utf8_lossy(&o.stdout), format!("{RESULT}\n"));
    assert_eq!(o.status.code(), Some(0));
}

#[test]
fn cargo_found_nowhere_else_is_found_in_home_cargo_bin() {
    let d = scratch("home");
    fs::create_dir_all(d.join("empty")).unwrap();
    stand_in(&d.join(".cargo/bin"), "exit 4");
    let o = with_env(&["build"], &d.join("empty"), None, &d);
    assert_eq!(o.status.code(), Some(4));
    assert_eq!(
        fs::read_to_string(d.join(".cargo/bin/args")).unwrap(),
        "build --message-format=json\n"
    );
}

#[test]
fn no_cargo_anywhere_exits_127_with_one_line_naming_the_fix() {
    let d = scratch("none");
    fs::create_dir_all(d.join("empty")).unwrap();
    let o = with_env(&["test"], &d.join("empty"), Some(&d.join("ch")), &d);
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(127));
    assert_eq!(err.lines().count(), 1);
    assert!(err.starts_with("cargo-failures: cargo not found"));
    assert!(err.contains("put cargo's directory on PATH"));
    assert_eq!(o.stdout, b"");
}

#[test]
fn a_cargo_that_cannot_start_exits_127() {
    let d = scratch("no-start");
    // Executable, but no interpreter at its #! line.
    use std::os::unix::fs::PermissionsExt;
    fs::create_dir_all(d.join("bin")).unwrap();
    fs::write(d.join("bin/cargo"), "#!/no/such/interpreter\n").unwrap();
    fs::set_permissions(d.join("bin/cargo"), fs::Permissions::from_mode(0o755)).unwrap();
    let o = with_env(&["test"], &d.join("bin"), None, &d);
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(127));
    assert_eq!(err.lines().count(), 1);
    assert!(err.starts_with("cargo-failures: cannot start "));
}

#[test]
fn a_cargo_killed_by_a_signal_exits_non_zero_with_the_reason() {
    let d = scratch("signal");
    stand_in(&d.join("bin"), "echo 'Compiling x' >&2\nkill -9 $$");
    let o = with_env(&["test"], &d.join("bin"), None, &d);
    let err = String::from_utf8_lossy(&o.stderr);
    assert_eq!(o.status.code(), Some(137));
    assert!(err.contains("cargo-failures: cargo was killed by signal 9; exiting 137"));
}

#[test]
fn a_crashed_test_binary_passes_cargo_stderr_on() {
    let d = scratch("crash");
    stand_in(
        &d.join("bin"),
        "echo 'running 2 tests'\necho 'test a ... ok'\necho 'error: test failed: process didn'\"'\"'t exit successfully (signal: 6, SIGABRT)' >&2\nexit 101",
    );
    let o = with_env(&["test"], &d.join("bin"), None, &d);
    assert_eq!(o.stdout, b"");
    assert!(String::from_utf8_lossy(&o.stderr).contains("SIGABRT"));
    assert_eq!(o.status.code(), Some(101));
}

// Refusals and help.

#[test]
fn a_message_format_of_ones_own_is_refused_with_its_fix() {
    let o = Command::new(BIN)
        .args(["build", "--message-format", "short"])
        .output()
        .unwrap();
    show(&o);
    assert_eq!(o.status.code(), Some(2));
    let err = String::from_utf8_lossy(&o.stderr);
    assert!(err.contains("--message-format is not taken"));
    assert!(err.contains("drop it"));
}

#[test]
fn a_missing_or_unknown_subcommand_is_refused_with_the_usage() {
    for args in [&[][..], &["check"][..]] {
        let o = Command::new(BIN).args(args).output().unwrap();
        show(&o);
        assert_eq!(o.status.code(), Some(2));
        let err = String::from_utf8_lossy(&o.stderr);
        assert!(err.starts_with("usage: cargo-failures build|test"));
        assert!(err.contains("build or test") || err.contains("build and test"));
    }
}

#[test]
fn help_and_version() {
    let o = Command::new(BIN).arg("--help").output().unwrap();
    let help = String::from_utf8_lossy(&o.stdout);
    assert_eq!(o.status.code(), Some(0));
    assert!(help.starts_with("usage: cargo-failures build"));
    assert!(
        help.contains("Exits 127 "),
        "exit codes put in from the constants"
    );
    assert!(!help.contains('{'), "no placeholder left");
    let o = Command::new(BIN).arg("--version").output().unwrap();
    assert_eq!(
        String::from_utf8_lossy(&o.stdout),
        format!("cargo-failures {}\n", env!("CARGO_PKG_VERSION"))
    );
}
