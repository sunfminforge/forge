//! The fixture tests of shared-failures, against the binary forge releases.
//!
//! Each case serves a fixture as Forgejo's API (issues and comments of
//! guild/tasks, paged as Forgejo pages them) on loopback, runs the detector
//! as the scheduler would, with only GUILD_L3 and GUILD_FORGEJO_* set, and
//! checks what it printed and how it exited.

mod findings;
mod http;

use std::process::Command;

use findings::parse;
use http::{Response, Server};
use serde_json::{json, Value};
use shared_failures::{reasons, Kind, Reason};

const DETECTOR: &str = env!("CARGO_BIN_EXE_shared-failures");
const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures");

/// A fixture's issues and comments, its fillers made comments that say no
/// reason, to page past.
fn load(name: &str) -> (Vec<Value>, Vec<Value>) {
    let text = std::fs::read_to_string(format!("{FIXTURES}/{name}")).unwrap();
    let data: Value = serde_json::from_str(&text).unwrap();
    let mut comments = Vec::new();
    let mut n = 1000;
    for c in data["comments"].as_array().unwrap() {
        if let Some(filler) = c.get("filler").and_then(Value::as_u64) {
            for _ in 0..filler {
                n += 1;
                comments.push(json!({"id": n, "issue": 10, "created_at": "2026-11-03T20:00:00+09:00",
                                     "body": format!("<!--guild:checked id={n}-->\nCheck accepted.")}));
            }
            continue;
        }
        comments.push(c.clone());
    }
    for c in &mut comments {
        let issue = c.as_object_mut().unwrap().remove("issue").unwrap();
        c["issue_url"] = json!(format!("http://forgejo.test/guild/tasks/issues/{issue}"));
    }
    (data["issues"].as_array().unwrap().clone(), comments)
}

/// Forgejo's API of guild/tasks, answering `status` to every call when it is
/// not 200.
fn serve(issues: Vec<Value>, comments: Vec<Value>, status: u16) -> Server {
    Server::start(move |req| {
        let num = |k: &str, d: usize| req.query.get(k).and_then(|v| v.parse().ok()).unwrap_or(d);
        let (limit, page) = (num("limit", 30), num("page", 1));
        let items = match req.path.as_str() {
            _ if status != 200 => None,
            "/api/v1/repos/guild/tasks/issues/comments" => Some(&comments),
            "/api/v1/repos/guild/tasks/issues" => Some(&issues),
            _ => None,
        };
        let Some(items) = items else {
            return Response::json(
                if status != 200 { status } else { 404 },
                r#"{"message":"no"}"#,
            );
        };
        let chunk: Vec<&Value> = items.iter().skip((page - 1) * limit).take(limit).collect();
        Response::json(200, serde_json::to_string(&chunk).unwrap())
    })
}

fn run(url: &str) -> (i32, String) {
    let out = Command::new(DETECTOR)
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("GUILD_L3", "/nonexistent/l3.sqlite")
        .env("GUILD_FORGEJO_URL", url)
        .env("GUILD_FORGEJO_TOKEN", "")
        .output()
        .expect("the detector runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8(out.stdout).unwrap(),
    )
}

#[test]
fn finds_a_send_back_reason_and_a_failure_cause_shared_by_several_roles_across_pages() {
    let (issues, comments) = load("shared.json");
    let server = serve(issues, comments, 200);
    let (code, out) = run(&server.url());
    assert_eq!(code, 0, "exited {code}");
    let found = parse(&out);
    let mut got: Vec<(&str, &str)> = found
        .iter()
        .map(|f| (f.role.as_str(), f.saw.split(" reason ").next().unwrap()))
        .collect();
    got.sort();
    assert_eq!(
        got,
        [
            ("rust-cli-coder", "the same failure"),
            ("rust-cli-coder", "the same send-back"),
            ("trainer", "the same failure"),
            ("trainer", "the same send-back"),
        ]
    );
    let sendback: Vec<_> = found
        .iter()
        .filter(|f| f.saw.contains("send-back"))
        .collect();
    assert_eq!(
        sendback[0].saw, sendback[1].saw,
        "one saw for every role of the reason"
    );
    assert!(
        sendback[0].saw.contains("(rust-cli-coder, trainer)"),
        "{}",
        sendback[0].saw
    );
    assert!(
        sendback[0].saw.contains("permission denied"),
        "{}",
        sendback[0].saw
    );
    let ev = &sendback[0].evidence;
    assert!(
        ev.len() == 2
            && ev[0].starts_with("guild/tasks#10 (rust-cli-coder")
            && ev[1].starts_with("guild/tasks#11 (trainer"),
        "{ev:?}"
    );
    let failure = found.iter().find(|f| f.saw.contains("failure")).unwrap();
    assert!(failure.saw.contains("cause=start"), "{}", failure.saw);
    let tasks: Vec<&str> = failure
        .evidence
        .iter()
        .map(|e| e.split(' ').next().unwrap())
        .collect();
    assert_eq!(
        tasks,
        ["guild/tasks#12", "guild/tasks#13"],
        "a label-less issue and a pull request are no task of a role"
    );
    assert!(
        !found
            .iter()
            .any(|f| f.saw.contains("web-coder") || f.role == "web-coder"),
        "web-coder shares nothing"
    );
}

#[test]
fn prints_byte_for_byte_what_the_script_detector_printed() {
    let (issues, comments) = load("shared.json");
    let server = serve(issues, comments, 200);
    assert_eq!(
        run(&server.url()),
        (0, include_str!("expected/shared.txt").to_string())
    );
}

#[test]
fn stays_quiet_on_reasons_one_role_repeats_and_reasons_no_two_roles_share() {
    let (issues, comments) = load("quiet.json");
    let server = serve(issues, comments, 200);
    let (code, out) = run(&server.url());
    assert_eq!(code, 0, "exited {code}");
    assert_eq!(out, "", "printed {out:?}");
}

#[test]
fn stays_quiet_exit_0_when_forgejo_refuses_it() {
    let server = serve(Vec::new(), Vec::new(), 403);
    assert_eq!(run(&server.url()), (0, String::new()));
}

#[test]
fn stays_quiet_exit_0_when_forgejo_is_not_there() {
    let server = serve(Vec::new(), Vec::new(), 200);
    let url = server.url();
    drop(server);
    assert_eq!(run(&url), (0, String::new()));
}

#[test]
fn reads_a_stewards_requeue_reason_as_a_failure_reason() {
    let said = reasons(
        "<!--guild:requeue-->\nNo fault outside the task: it ended without marking its items.",
    );
    assert_eq!(
        said,
        [Reason {
            kind: Kind::Failure,
            text: "No fault outside the task: it ended without marking its items.".into(),
            exact: false
        }]
    );
    assert_eq!(reasons("<!--guild:checked id=1-->\nCheck accepted."), []);
}

#[test]
fn answers_help_with_its_usage() {
    let out = Command::new(DETECTOR).arg("--help").output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8(out.stdout)
        .unwrap()
        .starts_with("usage: shared-failures\n"));
}

#[test]
fn refuses_an_argument_naming_how_to_run_it() {
    let out = Command::new(DETECTOR).arg("x").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8(out.stderr)
        .unwrap()
        .contains("run it as `shared-failures`"));
}
