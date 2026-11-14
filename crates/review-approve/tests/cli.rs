//! The real binary, run with the fake guild (`tests/fake/guild.rs`) first on
//! the PATH: its JSON, its exit code, and the guild calls it made.

use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::{json, Value};

/// The directory cargo built the fake guild into: `<target>/<profile>/examples`.
fn fakes() -> PathBuf {
    let bin = Path::new(env!("CARGO_BIN_EXE_review-approve"));
    let dir = bin.parent().unwrap().join("examples");
    assert!(
        dir.join("guild").is_file(),
        "the fake guild is built by cargo test as an example, at {}",
        dir.display()
    );
    dir
}

struct Run {
    json: Value,
    exit: i32,
    calls: Vec<Vec<String>>,
}

/// Runs review-approve with a fake guild whose `fail` is as given.
fn run(args: &[&str], fail: Value) -> Run {
    let dir = tempfile_dir();
    let state = dir.join("guild.json");
    std::fs::write(&state, json!({"fail": fail, "calls": []}).to_string()).unwrap();
    let path = format!(
        "{}:{}",
        fakes().display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let out = Command::new(env!("CARGO_BIN_EXE_review-approve"))
        .args(args)
        .env("PATH", path)
        .env("FAKE_GUILD_STATE", &state)
        .output()
        .unwrap();
    let stdout = String::from_utf8(out.stdout).unwrap();
    assert_eq!(
        stdout.lines().count(),
        1,
        "one JSON object on one line: {stdout}"
    );
    let json: Value = serde_json::from_str(&stdout).unwrap();
    let state: Value = serde_json::from_str(&std::fs::read_to_string(&state).unwrap()).unwrap();
    let calls = serde_json::from_value(state["calls"].clone()).unwrap();
    std::fs::remove_dir_all(&dir).ok();
    Run {
        json,
        exit: out.status.code().unwrap(),
        calls,
    }
}

fn tempfile_dir() -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "review-approve-test-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn words(s: &[&str]) -> Vec<String> {
    s.iter().map(|w| w.to_string()).collect()
}

const PASS: &[&str] = &[
    "pass",
    "guild/tasks#59",
    "--evidence",
    "gate green; every check rerun",
    "--pull",
    "guild/forge#6",
    "--pull",
    "guild/lore#9",
];

#[test]
fn it_reviews_then_approves_each_pull_in_order() {
    let r = run(PASS, json!({}));
    assert_eq!(r.exit, 0, "{}", r.json);
    assert_eq!(
        r.calls,
        vec![
            words(&[
                "task",
                "review",
                "guild/tasks#59",
                "--pass",
                "--evidence",
                "gate green; every check rerun"
            ]),
            words(&["pull", "approve", "guild/forge", "6"]),
            words(&["pull", "approve", "guild/lore", "9"]),
        ]
    );
    assert_eq!(r.json["reviewed"], true);
    assert_eq!(r.json["approved"], json!(["guild/forge#6", "guild/lore#9"]));
    assert_eq!(r.json["steps"].as_array().unwrap().len(), 3);
    assert_eq!(
        r.json["steps"][1]["command"],
        "guild pull approve guild/forge 6"
    );
    assert_eq!(r.json["steps"][0]["stdout"], "review: done\n");
}

#[test]
fn a_failed_review_approves_no_pull() {
    let r = run(PASS, json!({"review": 3}));
    assert_eq!(r.exit, 1, "{}", r.json);
    assert_eq!(r.calls.len(), 1, "only the review ran: {:?}", r.calls);
    assert_eq!(r.calls[0][..2], words(&["task", "review"]));
    assert_eq!(r.json["step"], "review");
    assert_eq!(r.json["reviewed"], false);
    assert_eq!(r.json["approved"], json!([]));
    assert_eq!(
        r.json["not_approved"],
        json!(["guild/forge#6", "guild/lore#9"])
    );
    let fault = &r.json["faults"][0];
    assert_eq!(fault["code"], "review_failed");
    assert!(
        fault["message"].as_str().unwrap().contains("exited 3"),
        "{fault}"
    );
    // The fix is the same command again, the evidence quoted for a shell.
    assert_eq!(
        fault["fix"],
        "review-approve pass guild/tasks#59 --evidence 'gate green; every check rerun' --pull guild/forge#6 --pull guild/lore#9"
    );
    assert_eq!(r.json["steps"][0]["stderr"], "fake guild: review fails\n");
}

#[test]
fn it_stops_at_the_first_approve_that_fails() {
    let r = run(PASS, json!({"approve guild/forge#6": 1}));
    assert_eq!(r.exit, 1, "{}", r.json);
    assert_eq!(
        r.calls.len(),
        2,
        "the second pull was not tried: {:?}",
        r.calls
    );
    assert_eq!(r.json["step"], "approve guild/forge#6");
    assert_eq!(r.json["reviewed"], true);
    assert_eq!(r.json["approved"], json!([]));
    assert_eq!(
        r.json["not_approved"],
        json!(["guild/forge#6", "guild/lore#9"])
    );
    let fault = &r.json["faults"][0];
    assert_eq!(fault["code"], "approve_failed");
    // The fix approves what is left, and does not review again.
    assert_eq!(
        fault["fix"],
        "guild pull approve guild/forge 6 && guild pull approve guild/lore 9"
    );
}

#[test]
fn a_later_approve_that_fails_keeps_the_ones_before_it() {
    let r = run(PASS, json!({"approve guild/lore#9": 1}));
    assert_eq!(r.exit, 1, "{}", r.json);
    assert_eq!(r.calls.len(), 3);
    assert_eq!(r.json["step"], "approve guild/lore#9");
    assert_eq!(r.json["approved"], json!(["guild/forge#6"]));
    assert_eq!(r.json["not_approved"], json!(["guild/lore#9"]));
    assert_eq!(
        r.json["faults"][0]["fix"],
        "guild pull approve guild/lore 9"
    );
}

#[test]
fn a_guild_that_cannot_start_is_a_failed_review() {
    let out = Command::new(env!("CARGO_BIN_EXE_review-approve"))
        .args(PASS)
        .env("PATH", tempfile_dir())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["step"], "review");
    assert_eq!(json["faults"][0]["code"], "review_failed");
    let fix = json["faults"][0]["fix"].as_str().unwrap();
    assert!(fix.starts_with("run review-approve doctor"), "{fix}");
}

#[test]
fn a_bad_request_is_refused_whole_and_runs_nothing() {
    let r = run(
        &[
            "pass",
            "guild/tasks#59",
            "--evidence",
            " ",
            "--pull",
            "forge",
            "--pull",
            "guild/forge#0",
        ],
        json!({}),
    );
    assert_eq!(r.exit, 2, "{}", r.json);
    assert!(r.calls.is_empty(), "nothing ran: {:?}", r.calls);
    let codes: Vec<&str> = r.json["faults"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["code"].as_str().unwrap())
        .collect();
    assert_eq!(codes, ["no_evidence", "bad_pull", "bad_pull"]);
    for fault in r.json["faults"].as_array().unwrap() {
        assert!(!fault["fix"].as_str().unwrap().is_empty());
    }
    assert!(r.json["faults"][1]["fix"]
        .as_str()
        .unwrap()
        .contains("--pull <repo>#<number>"));
}

#[test]
fn no_pull_and_a_pull_twice_are_refused_with_their_fix() {
    let r = run(&["pass", "guild/tasks#59", "--evidence", "ok"], json!({}));
    assert_eq!(r.exit, 2);
    assert_eq!(r.json["faults"][0]["code"], "no_pull");
    assert!(r.json["faults"][0]["fix"]
        .as_str()
        .unwrap()
        .contains("--pull guild/forge#6"));
    assert!(r.calls.is_empty());

    let r = run(
        &[
            "pass",
            "guild/tasks#59",
            "--evidence",
            "ok",
            "--pull",
            "guild/forge#6",
            "--pull",
            "guild/forge#6",
        ],
        json!({}),
    );
    assert_eq!(r.exit, 2);
    assert_eq!(r.json["faults"][0]["code"], "pull_twice");
    assert_eq!(
        r.json["faults"][0]["fix"],
        "drop the second --pull guild/forge#6"
    );
    assert!(r.calls.is_empty());
}

#[test]
fn a_bad_task_is_refused() {
    let r = run(
        &[
            "pass",
            "guild tasks 59",
            "--evidence",
            "ok",
            "--pull",
            "guild/forge#6",
        ],
        json!({}),
    );
    assert_eq!(r.exit, 2);
    assert_eq!(r.json["faults"][0]["code"], "bad_task");
    assert!(r.json["faults"][0]["fix"]
        .as_str()
        .unwrap()
        .contains("guild/tasks#59"));
    assert!(r.calls.is_empty());
}

#[test]
fn a_dry_run_answers_the_commands_and_runs_none() {
    let mut args = PASS.to_vec();
    args.push("--dry-run");
    let r = run(&args, json!({}));
    assert_eq!(r.exit, 0);
    assert!(r.calls.is_empty());
    assert_eq!(r.json["dry_run"], true);
    assert_eq!(
        r.json["steps"],
        json!([
            {"step": "review", "command": "guild task review guild/tasks#59 --pass --evidence 'gate green; every check rerun'"},
            {"step": "approve guild/forge#6", "command": "guild pull approve guild/forge 6"},
            {"step": "approve guild/lore#9", "command": "guild pull approve guild/lore 9"},
        ])
    );
}

#[test]
fn doctor_finds_nothing_with_guild_and_names_the_fix_without() {
    let r = run(&["doctor"], json!({}));
    assert_eq!(r.exit, 0);
    assert_eq!(r.json, json!({"findings": []}));
    assert_eq!(r.calls, vec![words(&["--version"])]);

    let r = run(&["doctor"], json!({"version": 1}));
    assert_eq!(r.exit, 0);
    assert_eq!(
        r.json["findings"][0]["fix"],
        "run guild doctor and do what it says"
    );

    let out = Command::new(env!("CARGO_BIN_EXE_review-approve"))
        .arg("doctor")
        .env("PATH", tempfile_dir())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(0));
    let json: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(json["findings"][0]["fix"]
        .as_str()
        .unwrap()
        .contains("PATH"));
}

#[test]
fn help_shows_the_one_call_that_reviews_and_approves() {
    let out = Command::new(env!("CARGO_BIN_EXE_review-approve"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(out.status.success());
    let help = String::from_utf8(out.stdout).unwrap();
    assert!(help.contains(
        "review-approve pass <task> --evidence <text> --pull <repo>#<number> [--pull <repo>#<number>]... [--dry-run]"
    ));
    assert!(help.contains("review-approve pass --help"));
}
