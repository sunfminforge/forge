//! test-filters' binary on the fixture workspace in fixtures/ws, whose
//! tests are known by hand (see fixtures/ws/sample/src/lib.rs): passing,
//! failing, ignored and no-match filters, and each refusal with its fix.
//! It never runs on forge itself.

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_test-filters");

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/ws")
}

/// The binary run in the fixture workspace, building it into this test
/// run's own temp target, never into the crate's source.
fn run_in_fixture(args: &[&str]) -> (Value, i32) {
    let target = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("fixture-target");
    let out = Command::new(BIN)
        .args(args)
        .current_dir(fixture())
        .env("CARGO_TARGET_DIR", target)
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    let v: Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("stdout is not one JSON object ({e}): {stdout}"));
    (v, out.status.code().unwrap())
}

fn filter<'a>(v: &'a Value, name: &str) -> &'a Value {
    v["filters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["filter"] == name)
        .unwrap_or_else(|| panic!("no filter {name} in {v}"))
}

fn ran_passed(v: &Value, name: &str) -> (u64, u64, &'static str) {
    let f = filter(v, name);
    let status = match f["status"].as_str().unwrap() {
        "passed" => "passed",
        "failed" => "failed",
        "empty" => "empty",
        other => panic!("status {other} of {name}: {f}"),
    };
    (
        f["ran"].as_u64().unwrap(),
        f["passed"].as_u64().unwrap(),
        status,
    )
}

#[test]
fn counts_two_matching_filters_and_exits_0() {
    let (v, code) = run_in_fixture(&["-p", "sample", "adds", "halves_even"]);
    // adds: adds_small (unit) and adds_large (tests/more.rs).
    assert_eq!(ran_passed(&v, "adds"), (2, 2, "passed"), "{v}");
    assert_eq!(ran_passed(&v, "halves_even"), (1, 1, "passed"), "{v}");
    assert_eq!(v["ok"], true, "{v}");
    assert_eq!(v["package"], "sample");
    assert_eq!(code, 0);
}

#[test]
fn empty_match_is_0_ran_and_fails_the_call() {
    let (v, code) = run_in_fixture(&["-p", "sample", "adds", "nothing_here"]);
    assert_eq!(ran_passed(&v, "adds"), (2, 2, "passed"), "{v}");
    assert_eq!(ran_passed(&v, "nothing_here"), (0, 0, "empty"), "{v}");
    assert_eq!(v["ok"], false);
    assert_ne!(code, 0);
    assert_eq!(code, 1);
}

#[test]
fn empty_when_the_only_match_is_ignored() {
    let (v, code) = run_in_fixture(&["-p", "sample", "skipped_one"]);
    assert_eq!(ran_passed(&v, "skipped_one"), (0, 0, "empty"), "{v}");
    assert_eq!(filter(&v, "skipped_one")["ignored"], 1);
    assert_eq!(code, 1);
}

#[test]
fn failing_filter_reports_passed_below_ran_and_fails_the_call() {
    let (v, code) = run_in_fixture(&["-p", "sample", "halves", "adds"]);
    let (ran, passed, status) = ran_passed(&v, "halves");
    assert_eq!((ran, passed, status), (2, 1, "failed"), "{v}");
    assert!(passed < ran);
    assert_eq!(filter(&v, "halves")["failed"], 1);
    assert_eq!(ran_passed(&v, "adds"), (2, 2, "passed"), "{v}");
    assert_eq!(v["ok"], false);
    assert_eq!(code, 1);
}

#[test]
fn refuses_arguments_naming_the_usage() {
    let (v, code) = run_in_fixture(&["--exact"]);
    assert_eq!(code, 2);
    let faults = v["faults"].as_array().unwrap();
    assert_eq!(faults.len(), 3, "{v}");
    for f in faults {
        assert_eq!(f["code"], "usage");
        assert_eq!(f["fix"], "test-filters -p <package> <filter>...");
    }
}

#[test]
fn refuses_an_unknown_package_as_build_failed() {
    let (v, code) = run_in_fixture(&["-p", "no-such-crate", "adds"]);
    assert_eq!(code, 2, "{v}");
    assert_eq!(v["faults"][0]["code"], "build_failed");
    assert_eq!(
        v["faults"][0]["fix"],
        "cargo test -p no-such-crate --no-run"
    );
    assert!(
        v["faults"][0]["message"]
            .as_str()
            .unwrap()
            .contains("no-such-crate"),
        "{v}"
    );
    assert!(v.get("filters").is_none());
}

#[test]
fn refuses_without_cargo_naming_rustup() {
    let empty = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("no-cargo-here");
    std::fs::create_dir_all(&empty).unwrap();
    let out = Command::new(BIN)
        .args(["-p", "sample", "adds"])
        .current_dir(fixture())
        .env("PATH", &empty)
        .env("HOME", &empty)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    let v: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["faults"][0]["code"], "cargo_not_found");
    assert!(v["faults"][0]["fix"]
        .as_str()
        .unwrap()
        .contains("https://sh.rustup.rs"));
}

#[test]
fn help_and_version_answer() {
    let out = Command::new(BIN).arg("--help").output().unwrap();
    assert!(out.status.success());
    let h = String::from_utf8(out.stdout).unwrap();
    assert!(h.starts_with("usage: test-filters -p <package> <filter>..."));
    for word in [
        "passed",
        "failed",
        "empty",
        "cargo_failed",
        "cargo_not_found",
        "build_failed",
    ] {
        assert!(h.contains(word), "help names {word}");
    }
    let out = Command::new(BIN).arg("--version").output().unwrap();
    assert_eq!(
        String::from_utf8(out.stdout).unwrap().trim(),
        format!("test-filters {}", env!("CARGO_PKG_VERSION"))
    );
}
