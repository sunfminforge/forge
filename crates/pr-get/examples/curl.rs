//! A stand-in for `curl`, placed first on the PATH by the tests: it reads the
//! config `pr-get` writes on its stdin, answers from one JSON state file and
//! records each call beside it. Nothing reaches a network.
//!
//! The state, in the file `PR_GET_STANDIN` names:
//!
//! - `token`: the one token it lets in; any other is answered 401.
//! - `pulls`: `{"<owner>/<repo>/<number>": <the pull request>}`; any other is 404.
//! - `status`, `body`: when set, every request is answered this status and
//!   this body, as text.
//! - `fail`: when set, curl fails, saying this on stderr, exit 6.
//!
//! Each call is one JSON line in `<state>.calls`: `{"args", "url", "authorization"}`.

use std::io::{Read, Write};

use pr_get::curl::{ARGS, WRITE_OUT};
use serde_json::{Value, json};

fn main() {
    let state_path = std::env::var("PR_GET_STANDIN").expect("PR_GET_STANDIN names the state");
    let state: Value =
        serde_json::from_str(&std::fs::read_to_string(&state_path).expect("the state"))
            .expect("JSON");
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut config = String::new();
    std::io::stdin().read_to_string(&mut config).expect("stdin");
    let mut url = String::new();
    let mut authorization = Value::Null;
    let mut write_out = String::new();
    for line in config.lines() {
        let Some((key, value)) = line.split_once(" = ") else {
            continue;
        };
        let value = value.trim_matches('"').to_string();
        match key {
            "url" => url = value,
            "write-out" => write_out = value,
            "header" => {
                if let Some(auth) = value.strip_prefix("Authorization: ") {
                    authorization = json!(auth);
                }
            }
            _ => {}
        }
    }
    let mut calls = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(format!("{state_path}.calls"))
        .expect("the calls file");
    writeln!(
        calls,
        "{}",
        json!({"args": args, "url": url, "authorization": authorization, "config": config})
    )
    .expect("a call recorded");
    if args != ARGS || write_out != WRITE_OUT {
        eprintln!("curl stand-in: not run as pr-get runs curl: {args:?}, write-out {write_out:?}");
        std::process::exit(2);
    }
    if let Some(Value::String(said)) = state.get("fail") {
        eprintln!("curl: {said}");
        std::process::exit(6);
    }
    let (status, body) = answer(&state, &url, &authorization);
    print!("{body}\n{status}");
}

fn answer(state: &Value, url: &str, authorization: &Value) -> (u16, String) {
    if let Some(status) = state.get("status").and_then(Value::as_u64) {
        let body = state.get("body").and_then(Value::as_str).unwrap_or("");
        return (u16::try_from(status).unwrap_or(500), body.to_string());
    }
    let expected = format!("token {}", state["token"].as_str().unwrap_or(""));
    if authorization.as_str() != Some(expected.as_str()) {
        return (
            401,
            json!({"message": "user does not exist [uid: 0, name: ]"}).to_string(),
        );
    }
    let key = url
        .split_once("/repos/")
        .map(|(_, path)| path.replacen("/pulls/", "/", 1))
        .unwrap_or_default();
    match state["pulls"].get(&key) {
        Some(pull) => (200, pull.to_string()),
        None => (
            404,
            json!({"message": "The target couldn't be found.", "url": url}).to_string(),
        ),
    }
}
