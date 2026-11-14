//! with-cargo's binary run against a stub cargo in a temp CARGO_HOME/bin
//! or HOME/.cargo/bin, never the machine's own, with a PATH of
//! /usr/bin:/bin that lacks it. Each test prints what the binary printed,
//! seen with `-- --nocapture`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_with-cargo");
/// The PATH every test runs with-cargo on: no cargo bin dir in it.
const BARE_PATH: &str = "/usr/bin:/bin";

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A fresh directory of this test run's own.
fn scratch(name: &str) -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("with-cargo")
        .join(format!("{name}-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A stub cargo in `bin`: prints its args and the PATH it ran with, and
/// exits with the code in $STUB_EXIT, 0 when unset.
fn stub_cargo(bin: &Path) {
    fs::create_dir_all(bin).unwrap();
    let p = bin.join("cargo");
    fs::write(
        &p,
        "#!/bin/sh\necho \"stub cargo: $*\"\necho \"PATH=$PATH\"\nexit ${STUB_EXIT:-0}\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

/// with-cargo with PATH lacking cargo, and only these as CARGO_HOME and HOME.
fn with_cargo(args: &[&str], cargo_home: Option<&Path>, home: &Path, exit: &str) -> Output {
    let mut c = Command::new(BIN);
    c.args(args)
        .env("PATH", BARE_PATH)
        .env("HOME", home)
        .env("STUB_EXIT", exit)
        .env_remove("CARGO_HOME");
    if let Some(h) = cargo_home {
        c.env("CARGO_HOME", h);
    }
    let o = c.output().unwrap();
    println!(
        "--- stdout:\n{}--- stderr:\n{}--- exit: {:?}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr),
        o.status.code()
    );
    o
}

fn out(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}

fn err(o: &Output) -> String {
    String::from_utf8(o.stderr.clone()).unwrap()
}

#[test]
fn a_stub_cargo_in_cargo_home_bin_runs_with_a_path_lacking_it() {
    let d = scratch("cargo-home");
    let ch = d.join("cargo-home");
    stub_cargo(&ch.join("bin"));
    let o = with_cargo(
        &["cargo", "test", "-p", "feed-parse"],
        Some(&ch),
        &d.join("home"),
        "0",
    );
    assert_eq!(o.status.code(), Some(0));
    let s = out(&o);
    assert!(s.contains("stub cargo: test -p feed-parse"), "{s}");
    assert!(
        s.contains(&format!("PATH={}:{BARE_PATH}", ch.join("bin").display())),
        "{s}"
    );
}

#[test]
fn cargo_home_is_used_before_home_when_both_hold_cargo() {
    let d = scratch("both");
    let ch = d.join("cargo-home");
    let home = d.join("home");
    stub_cargo(&ch.join("bin"));
    stub_cargo(&home.join(".cargo/bin"));
    let o = with_cargo(&["--dir"], Some(&ch), &home, "0");
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(out(&o), format!("{}\n", ch.join("bin").display()));
}

#[test]
fn home_cargo_bin_is_used_when_cargo_home_is_unset() {
    let d = scratch("home");
    let home = d.join("home");
    stub_cargo(&home.join(".cargo/bin"));
    let o = with_cargo(&["cargo", "--version"], None, &home, "0");
    assert_eq!(o.status.code(), Some(0));
    let s = out(&o);
    assert!(s.contains("stub cargo: --version"), "{s}");
    assert!(
        s.contains(&format!(
            "PATH={}:{BARE_PATH}",
            home.join(".cargo/bin").display()
        )),
        "{s}"
    );
}

#[test]
fn the_commands_exit_code_is_passed_through() {
    let d = scratch("exit");
    let home = d.join("home");
    stub_cargo(&home.join(".cargo/bin"));
    for code in ["1", "101", "42"] {
        let o = with_cargo(&["cargo", "test"], None, &home, code);
        assert_eq!(o.status.code(), Some(code.parse().unwrap()));
    }
}

#[test]
fn a_command_other_than_cargo_runs_with_the_dir_on_its_path() {
    let d = scratch("other");
    let home = d.join("home");
    stub_cargo(&home.join(".cargo/bin"));
    let o = with_cargo(&["sh", "-c", "cargo build; exit 7"], None, &home, "0");
    assert_eq!(o.status.code(), Some(7));
    assert!(out(&o).contains("stub cargo: build"));
}

#[test]
fn dir_prints_the_dir_it_would_use() {
    let d = scratch("dir");
    let home = d.join("home");
    stub_cargo(&home.join(".cargo/bin"));
    let o = with_cargo(&["--dir"], None, &home, "0");
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(out(&o), format!("{}\n", home.join(".cargo/bin").display()));
}

#[test]
fn a_missing_cargo_dir_exits_non_zero_naming_the_dirs_looked_in() {
    let d = scratch("missing");
    let ch = d.join("cargo-home");
    let home = d.join("home");
    for args in [&["cargo", "test"][..], &["--dir"][..]] {
        let o = with_cargo(args, Some(&ch), &home, "0");
        assert_eq!(o.status.code(), Some(127));
        assert!(out(&o).is_empty());
        let e = err(&o);
        assert!(e.contains(&ch.join("bin").display().to_string()), "{e}");
        assert!(
            e.contains(&home.join(".cargo/bin").display().to_string()),
            "{e}"
        );
        assert!(e.contains("install Rust with rustup"), "{e}");
    }
}

#[test]
fn a_command_that_cannot_start_exits_127() {
    let d = scratch("nocmd");
    let home = d.join("home");
    stub_cargo(&home.join(".cargo/bin"));
    let o = with_cargo(&["no-such-command-here"], None, &home, "0");
    assert_eq!(o.status.code(), Some(127));
    assert!(err(&o).contains("cannot start no-such-command-here"));
}

#[test]
fn an_unknown_option_or_no_command_is_refused_with_the_usage() {
    let d = scratch("usage");
    for args in [&[][..], &["--nope"][..], &["--dir", "x"][..]] {
        let o = with_cargo(args, None, &d, "0");
        assert_eq!(o.status.code(), Some(2));
        assert!(err(&o).contains("usage: with-cargo"));
    }
}

#[test]
fn help_and_version() {
    let d = scratch("help");
    let o = with_cargo(&["--help"], None, &d, "0");
    assert_eq!(o.status.code(), Some(0));
    let h = out(&o);
    assert!(h.contains("$CARGO_HOME/bin") && h.contains("--dir") && h.contains("127"));
    let o = with_cargo(&["--version"], None, &d, "0");
    assert_eq!(out(&o), "with-cargo 0.1.0\n");
}
