//! A stand-in for `guild`, put first on the PATH by the tests.
//!
//! Its state is one JSON file, named by `FAKE_GUILD_STATE`:
//! `{"fail": {"<step>": <exit>}, "calls": [[<arg>...]...]}`. Each call is
//! appended to `calls`; a call whose step is a key of `fail` prints to
//! stderr and exits with its value. A step is named as review-approve names
//! it: `review`, `approve <repo>#<number>`, or `version`.

use std::process::ExitCode;

use serde_json::{json, Value};

fn step(args: &[String]) -> String {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["task", "review", ..] => "review".into(),
        ["pull", "approve", repo, number] => format!("approve {repo}#{number}"),
        ["--version"] => "version".into(),
        _ => format!("unknown {}", words.join(" ")),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = std::env::var("FAKE_GUILD_STATE").expect("FAKE_GUILD_STATE names the fake's state");
    let mut state: Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({"fail": {}, "calls": []}));
    if !state["calls"].is_array() {
        state["calls"] = json!([]);
    }
    state["calls"].as_array_mut().unwrap().push(json!(args));
    std::fs::write(&path, state.to_string()).expect("the fake writes its state");
    let step = step(&args);
    if let Some(code) = state["fail"][&step].as_u64() {
        eprintln!("fake guild: {step} fails");
        return ExitCode::from(code as u8);
    }
    match step.as_str() {
        "version" => println!("guild 0.0.0-fake"),
        _ => println!("{step}: done"),
    }
    ExitCode::SUCCESS
}
