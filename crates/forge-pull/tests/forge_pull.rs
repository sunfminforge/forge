//! forge-pull's binary against a stand-in Forgejo: open sends head, base,
//! title and body and prints repo#number and URL, and sends nothing when
//! the pull is already open or with --dry-run; find keeps the pulls saying
//! Closes <task> (not <task>0) across the owner's repos and pages, with
//! their head sha and review state; Forgejo's errors and no match fail.
//! Offline: nothing reaches a real forge.

mod forgejo;

use std::path::PathBuf;
use std::process::{Command, Output};

use forgejo::{Data, Fake, BROKEN, TOKEN, UNPUSHED};
use serde_json::{json, Value};

const BIN: &str = env!("CARGO_BIN_EXE_forge-pull");

fn pull(number: u64, state: &str, merged: bool, head: &str, sha: &str, body: &str) -> Value {
    json!({
        "id": 1000 + number,
        "number": number,
        "state": state,
        "merged": merged,
        "title": format!("Pull {number}"),
        "body": body,
        "html_url": format!("http://forgejo/guild/x/pulls/{number}"),
        "head": {"ref": head, "sha": sha},
        "base": {"ref": "main"},
    })
}

fn review(id: u64, who: &str, state: &str) -> Value {
    json!({"id": id, "user": {"login": who}, "state": state, "dismissed": false})
}

const SHA48: &str = "e6a2fcf0830a349ff649e3734190fc25b9b1642d";

fn data() -> Data {
    // mdsite: more than one page of pulls; the one closing #48 is on the second.
    let mut mdsite: Vec<Value> = (2..=60)
        .rev()
        .map(|n| {
            pull(
                n,
                "closed",
                true,
                &format!("guild/tasks-{}", n + 100),
                "1111111111111111111111111111111111111111",
                &format!("Closes guild/tasks#{}\n\nWork.", n + 100),
            )
        })
        .collect();
    mdsite.push(pull(
        1,
        "closed",
        true,
        "guild/tasks-48",
        SHA48,
        "Closes guild/tasks#48\n\n## What changed\n\nThe workspace.",
    ));
    Data {
        repos: vec![
            ("tasks".into(), vec![]),
            (
                "forge".into(),
                vec![
                    pull(
                        7,
                        "open",
                        false,
                        "guild/tasks-480",
                        "4804804804804804804804804804804804804804",
                        "Closes guild/tasks#480",
                    ),
                    pull(
                        6,
                        "open",
                        false,
                        "guild/tasks-48-again",
                        "6666666666666666666666666666666666666666",
                        "Follow-up.\n\ncloses guild/tasks#48.",
                    ),
                    pull(
                        5,
                        "closed",
                        false,
                        "guild/tasks-226",
                        "5555555555555555555555555555555555555555",
                        "Closes guild/tasks#226",
                    ),
                ],
            ),
            ("mdsite".into(), mdsite),
        ],
        reviews: vec![
            (
                "mdsite".into(),
                1,
                vec![
                    review(1, "warden", "REQUEST_CHANGES"),
                    review(2, "warden", "APPROVED"),
                ],
            ),
            (
                "forge".into(),
                6,
                vec![
                    review(3, "steward", "COMMENT"),
                    review(4, "warden", "REQUEST_CHANGES"),
                ],
            ),
        ],
    }
}

fn run(fake: &Fake, token: &str, args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .env_clear()
        .env("GUILD_FORGEJO_URL", &fake.url)
        .env("GUILD_FORGEJO_TOKEN", token)
        .output()
        .expect("forge-pull runs")
}

fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}

fn stderr(o: &Output) -> String {
    String::from_utf8(o.stderr.clone()).unwrap()
}

/// A body file in a temp directory of the test's own.
fn body_file(test: &str, body: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("forge-pull-{test}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("body.md");
    std::fs::write(&f, body).unwrap();
    f
}

fn no_token_in(fake: &Fake, o: &Output) {
    for r in fake.requests() {
        assert!(!r.target.contains(TOKEN), "token in a URL: {}", r.target);
        assert!(!r.body.contains(TOKEN), "token in a body: {}", r.body);
    }
    assert!(!stdout(o).contains(TOKEN) && !stderr(o).contains(TOKEN));
}

fn posts(fake: &Fake) -> Vec<forgejo::Request> {
    fake.requests()
        .into_iter()
        .filter(|r| r.method != "GET")
        .collect()
}

const BODY: &str = "Closes guild/tasks#226\n\n## What changed\n\n- \"quoted\" and a tab\t.\n";

fn open_args<'a>(file: &'a str, head: &'a str) -> Vec<&'a str> {
    vec![
        "open",
        "guild/forge",
        "--head",
        head,
        "--title",
        "forge-pull: open and find pulls",
        "--body-file",
        file,
    ]
}

#[test]
fn open_sends_head_base_title_and_body() {
    let fake = Fake::new(data());
    let f = body_file("open", BODY);
    let o = run(
        &fake,
        TOKEN,
        &open_args(f.to_str().unwrap(), "guild/tasks-999"),
    );
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let url = fake.url.trim_end_matches('/');
    assert_eq!(
        stdout(&o),
        format!("guild/forge#163 {url}/guild/forge/pulls/163\n")
    );
    let sent = posts(&fake);
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].method, "POST");
    assert_eq!(sent[0].target, "/api/v1/repos/guild/forge/pulls");
    let body: Value = serde_json::from_str(&sent[0].body).unwrap();
    assert_eq!(
        body,
        json!({"head": "guild/tasks-999", "base": "main",
               "title": "forge-pull: open and find pulls", "body": BODY})
    );
    no_token_in(&fake, &o);
}

#[test]
fn open_takes_another_base() {
    let fake = Fake::new(data());
    let f = body_file("base", "b");
    let mut args = open_args(f.to_str().unwrap(), "guild/tasks-999");
    args.extend(["--base", "release"]);
    let o = run(&fake, TOKEN, &args);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let body: Value = serde_json::from_str(&posts(&fake)[0].body).unwrap();
    assert_eq!(body["base"], "release");
}

#[test]
fn open_twice_sends_once() {
    let fake = Fake::new(data());
    let f = body_file("twice", BODY);
    let args = open_args(f.to_str().unwrap(), "guild/tasks-999");
    let first = run(&fake, TOKEN, &args);
    let second = run(&fake, TOKEN, &args);
    assert_eq!(second.status.code(), Some(0), "{}", stderr(&second));
    assert_eq!(stdout(&first), stdout(&second));
    assert!(stderr(&second).contains("already open; nothing was sent"));
    assert_eq!(posts(&fake).len(), 1);
}

#[test]
fn open_dry_run_sends_nothing() {
    let fake = Fake::new(data());
    let f = body_file("dry", "b");
    let mut args = open_args(f.to_str().unwrap(), "guild/tasks-999");
    args.push("--dry-run");
    let o = run(&fake, TOKEN, &args);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(
        stdout(&o),
        "would send POST /api/v1/repos/guild/forge/pulls {\"base\":\"main\",\"body\":\"b\",\"head\":\"guild/tasks-999\",\"title\":\"forge-pull: open and find pulls\"}\n"
    );
    assert!(posts(&fake).is_empty());
}

#[test]
fn open_api_error_prints_status_and_message() {
    let fake = Fake::new(data());
    let f = body_file("err", "b");
    let o = run(&fake, TOKEN, &open_args(f.to_str().unwrap(), UNPUSHED));
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(stdout(&o), "");
    let err = stderr(&o);
    assert!(
        err.contains("Forgejo answered 404 to POST /api/v1/repos/guild/forge/pulls: The target couldn't be found."),
        "{err}"
    );
    assert!(
        err.contains("check the owner/repo (and the branch"),
        "{err}"
    );
    let o = run(&fake, TOKEN, &open_args(f.to_str().unwrap(), "main"));
    assert_eq!(o.status.code(), Some(1));
    let err = stderr(&o);
    assert!(
        err.contains("Forgejo answered 422 to POST /api/v1/repos/guild/forge/pulls: Invalid PullRequest: There are no changes between the head and the base"),
        "{err}"
    );
    assert!(err.contains("fix what Forgejo's message names"), "{err}");
}

#[test]
fn open_refuses_an_unreadable_body_file() {
    let fake = Fake::new(data());
    let o = run(
        &fake,
        TOKEN,
        &open_args("/nonexistent/body.md", "guild/tasks-999"),
    );
    assert_eq!(o.status.code(), Some(1));
    let err = stderr(&o);
    assert!(
        err.contains("the body file /nonexistent/body.md could not be read"),
        "{err}"
    );
    assert!(err.contains("write the pull's body to that file"), "{err}");
    assert!(fake.requests().is_empty(), "nothing asked of Forgejo");
}

#[test]
fn find_prints_each_pull_closing_the_task() {
    let fake = Fake::new(data());
    let o = run(&fake, TOKEN, &["find", "guild/tasks#48"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(
        stdout(&o),
        format!(
            "guild/forge#6 open head=guild/tasks-48-again sha=6666666666666666666666666666666666666666 review=changes-requested\nguild/mdsite#1 merged head=guild/tasks-48 sha={SHA48} review=approved\n"
        )
    );
    let requests = fake.requests();
    assert!(
        requests.iter().all(|r| r.method == "GET"),
        "find only reads"
    );
    // mdsite's 60 pulls are two pages; tasks has pull requests off.
    let mdsite = requests
        .iter()
        .filter(|r| r.target.starts_with("/api/v1/repos/guild/mdsite/pulls?"))
        .count();
    assert_eq!(mdsite, 2);
    assert!(!requests
        .iter()
        .any(|r| r.target.starts_with("/api/v1/repos/guild/tasks/")));
    no_token_in(&fake, &o);
}

#[test]
fn find_review_none_and_closed() {
    let fake = Fake::new(data());
    let o = run(&fake, TOKEN, &["find", "guild/tasks#226"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(
        stdout(&o),
        "guild/forge#5 closed head=guild/tasks-226 sha=5555555555555555555555555555555555555555 review=none\n"
    );
}

#[test]
fn find_with_no_pull_saying_so_fails_saying_none() {
    let fake = Fake::new(data());
    let o = run(&fake, TOKEN, &["find", "guild/tasks#4"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(stdout(&o), "");
    let err = stderr(&o);
    assert!(
        err.contains("none: no pull of guild's 3 repos says Closes guild/tasks#4"),
        "{err}"
    );
    assert!(err.contains("open one with forge-pull open"), "{err}");
}

#[test]
fn find_api_error_prints_status_and_message() {
    let mut d = data();
    d.repos.push((BROKEN.into(), vec![]));
    let fake = Fake::new(d);
    let o = run(&fake, TOKEN, &["find", "guild/tasks#48"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(stdout(&o), "");
    let err = stderr(&o);
    assert!(
        err.contains(
            "Forgejo answered 500 to GET /api/v1/repos/guild/broken/pulls: database is locked"
        ),
        "{err}"
    );
}

#[test]
fn refuses_a_wrong_token_with_its_fix() {
    let fake = Fake::new(data());
    let o = run(&fake, "not-the-token", &["find", "guild/tasks#48"]);
    assert_eq!(o.status.code(), Some(1));
    let err = stderr(&o);
    assert!(
        err.contains("Forgejo answered 401 to GET /api/v1/users/guild/repos: Only signed in user is allowed to call APIs."),
        "{err}"
    );
    assert!(err.contains("run forge-pull with the token the instance was given"));
    assert!(!err.contains("not-the-token"), "the token is never printed");
}

#[test]
fn refuses_a_missing_environment_with_its_fix() {
    let o = Command::new(BIN)
        .args(["find", "guild/tasks#48"])
        .env_clear()
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(1));
    let err = stderr(&o);
    assert!(
        err.contains("GUILD_FORGEJO_URL and GUILD_FORGEJO_TOKEN not set"),
        "{err}"
    );
    assert!(
        err.contains("run forge-pull inside a Guild instance"),
        "{err}"
    );
}

#[test]
fn refuses_bad_arguments_with_usage() {
    let fake = Fake::new(data());
    for args in [
        &["find", "guild/tasks"][..],
        &["find", "guild/tasks#x"],
        &["open", "guild/forge", "--head", "h", "--title", "t"],
        &["open", "guild/forge", "--title", "t", "--body-file", "f"],
        &["pull", "guild/forge"],
        &[],
    ] {
        let o = run(&fake, TOKEN, args);
        assert_eq!(o.status.code(), Some(2), "{args:?}");
        let err = stderr(&o);
        assert!(err.starts_with("usage: forge-pull open"), "{err}");
        assert!(err.contains("see forge-pull --help"), "{err}");
    }
    assert!(fake.requests().is_empty(), "a refused command asks nothing");
}

#[test]
fn help_and_version() {
    let fake = Fake::new(data());
    let o = run(&fake, TOKEN, &["--help"]);
    assert_eq!(o.status.code(), Some(0));
    let help = stdout(&o);
    assert!(help.contains(
        "forge-pull open <owner/repo> --head <branch> [--base main] --title <t> --body-file <f>"
    ));
    assert!(help.contains("forge-pull find <owner/repo>#<n>"));
    let o = run(&fake, TOKEN, &["--version"]);
    assert_eq!(
        stdout(&o),
        format!("forge-pull {}\n", env!("CARGO_PKG_VERSION"))
    );
}
