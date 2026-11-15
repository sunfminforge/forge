//! forge-comment's binary against a stand-in Forgejo: say POSTs the body
//! as JSON to the issue's comments and correct PATCHes the comment, both
//! with the token as Authorization, and print the comment's id and URL;
//! --dry-run sends nothing and needs no environment; Forgejo's errors exit
//! 1 naming the status and the issue or comment; a missing variable exits
//! 2 naming it. Offline: nothing reaches a real forge.

mod forgejo;

use std::path::PathBuf;
use std::process::{Command, Output};

use forgejo::{Data, Fake, ME, TOKEN};
use serde_json::{json, Value};

const BIN: &str = env!("CARGO_BIN_EXE_forge-comment");

fn data() -> Data {
    Data {
        issues: vec![247, 249],
        comments: vec![
            (10, 247, ME.into(), "waits for #248".into()),
            (11, 247, "felix".into(), "Felix's own".into()),
        ],
    }
}

fn run_env(env: &[(&str, &str)], args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .env_clear()
        .envs(env.iter().copied())
        .output()
        .expect("forge-comment runs")
}

fn run(fake: &Fake, token: &str, args: &[&str]) -> Output {
    run_env(
        &[
            ("GUILD_FORGEJO_URL", &fake.url),
            ("GUILD_FORGEJO_TOKEN", token),
        ],
        args,
    )
}

fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}

fn stderr(o: &Output) -> String {
    String::from_utf8(o.stderr.clone()).unwrap()
}

fn body_file(test: &str, body: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("forge-comment-{test}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("body.md");
    std::fs::write(&f, body).unwrap();
    f
}

fn no_token_out(fake: &Fake, o: &Output) {
    for r in fake.requests() {
        assert!(!r.target.contains(TOKEN), "token in a URL: {}", r.target);
        assert!(!r.body.contains(TOKEN), "token in a body: {}", r.body);
    }
    assert!(!stdout(o).contains(TOKEN) && !stderr(o).contains(TOKEN));
}

const BODY: &str = "Split accepted; #250 waits for #249.\n\n- \"quoted\" and a tab\t.\n";

#[test]
fn say_posts_the_body_and_prints_id_and_url() {
    let fake = Fake::new(data());
    let o = run(&fake, TOKEN, &["say", "guild/tasks#247", "--text", BODY]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let url = fake.url.trim_end_matches('/');
    assert_eq!(
        stdout(&o),
        format!("12 {url}/guild/tasks/issues/247#issuecomment-12\n")
    );
    let sent = fake.requests();
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].method, "POST");
    assert_eq!(
        sent[0].target,
        "/api/v1/repos/guild/tasks/issues/247/comments"
    );
    assert_eq!(sent[0].auth.as_deref(), Some("token stand-in-token"));
    assert_eq!(sent[0].content_type.as_deref(), Some("application/json"));
    let body: Value = serde_json::from_str(&sent[0].body).unwrap();
    assert_eq!(body, json!({ "body": BODY }));
    assert_eq!(fake.comment_body(12).as_deref(), Some(BODY));
    no_token_out(&fake, &o);
}

#[test]
fn say_takes_a_body_file() {
    let fake = Fake::new(data());
    let f = body_file("say", BODY);
    let o = run(
        &fake,
        TOKEN,
        &["say", "guild/tasks#249", "--body-file", f.to_str().unwrap()],
    );
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert!(stdout(&o).ends_with("/guild/tasks/issues/249#issuecomment-12\n"));
    assert_eq!(fake.comment_body(12).as_deref(), Some(BODY));
}

#[test]
fn correct_patches_the_comment_and_prints_id_and_url() {
    let fake = Fake::new(data());
    let f = body_file("correct", "waits for #248 and #250\n");
    let args = [
        "correct",
        "guild/tasks",
        "10",
        "--body-file",
        f.to_str().unwrap(),
    ];
    let o = run(&fake, TOKEN, &args);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let url = fake.url.trim_end_matches('/');
    assert_eq!(
        stdout(&o),
        format!("10 {url}/guild/tasks/issues/247#issuecomment-10\n")
    );
    let sent = fake.requests();
    assert_eq!(sent.len(), 1, "{sent:?}");
    assert_eq!(sent[0].method, "PATCH");
    assert_eq!(
        sent[0].target,
        "/api/v1/repos/guild/tasks/issues/comments/10"
    );
    assert_eq!(sent[0].auth.as_deref(), Some("token stand-in-token"));
    let body: Value = serde_json::from_str(&sent[0].body).unwrap();
    assert_eq!(body, json!({"body": "waits for #248 and #250\n"}));
    assert_eq!(
        fake.comment_body(10).as_deref(),
        Some("waits for #248 and #250\n")
    );
    // Again: the same answer, the same body.
    let again = run(&fake, TOKEN, &args);
    assert_eq!(stdout(&again), stdout(&o));
    no_token_out(&fake, &o);
}

#[test]
fn dry_run_sends_nothing_and_needs_no_environment() {
    let fake = Fake::new(data());
    let o = run(
        &fake,
        TOKEN,
        &["say", "guild/tasks#247", "--text", "dry run", "--dry-run"],
    );
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(
        stdout(&o),
        "would send POST /api/v1/repos/guild/tasks/issues/247/comments {\"body\":\"dry run\"}\n"
    );
    assert!(stderr(&o).contains("nothing was sent"));
    let o = run_env(
        &[],
        &["correct", "guild/tasks", "10", "--dry-run", "--text", "t"],
    );
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(
        stdout(&o),
        "would send PATCH /api/v1/repos/guild/tasks/issues/comments/10 {\"body\":\"t\"}\n"
    );
    assert!(fake.requests().is_empty());
    assert_eq!(fake.comment_body(10).as_deref(), Some("waits for #248"));
}

fn fails(o: &Output, code: i32, says: &[&str]) {
    assert_eq!(o.status.code(), Some(code), "{}", stderr(o));
    assert_eq!(stdout(o), "");
    let err = stderr(o);
    for s in says {
        assert!(err.contains(s), "{s:?} not in {err}");
    }
}

#[test]
fn forgejo_errors_name_the_status_and_the_issue_or_comment() {
    let fake = Fake::new(data());
    fails(
        &run(&fake, "wrong", &["say", "guild/tasks#247", "--text", "t"]),
        1,
        &["Forgejo answered 401 to POST /api/v1/repos/guild/tasks/issues/247/comments for issue guild/tasks#247: Only signed in user", "GUILD_FORGEJO_TOKEN"],
    );
    fails(
        &run(&fake, TOKEN, &["correct", "guild/tasks", "11", "--text", "t"]),
        1,
        &["Forgejo answered 403 to PATCH /api/v1/repos/guild/tasks/issues/comments/11 for comment 11 of guild/tasks", "correct only a comment the instance's account wrote"],
    );
    fails(
        &run(&fake, TOKEN, &["say", "guild/tasks#999", "--text", "t"]),
        1,
        &[
            "Forgejo answered 404",
            "for issue guild/tasks#999: The target couldn't be found.",
            "forge-read issue",
        ],
    );
    fails(
        &run(
            &fake,
            TOKEN,
            &["correct", "guild/tasks", "99", "--text", "t"],
        ),
        1,
        &[
            "Forgejo answered 404",
            "for comment 99 of guild/tasks",
            "#issuecomment-",
        ],
    );
    fails(
        &run(&fake, TOKEN, &["say", "guild/nope#1", "--text", "t"]),
        1,
        &["Forgejo answered 404", "for issue guild/nope#1"],
    );
    assert_eq!(fake.comment_body(11).as_deref(), Some("Felix's own"));
}

#[test]
fn forgejo_422_names_the_status_and_its_message() {
    let fake = Fake::new(data());
    let long = "x".repeat(forgejo::MAX_BODY + 1);
    fails(
        &run(&fake, TOKEN, &["correct", "guild/tasks", "10", "--text", &long]),
        1,
        &["Forgejo answered 422 to PATCH /api/v1/repos/guild/tasks/issues/comments/10 for comment 10 of guild/tasks: [Body]: MaxSize", "fix what Forgejo's message names"],
    );
    assert_eq!(fake.comment_body(10).as_deref(), Some("waits for #248"));
}

#[test]
fn an_empty_body_is_refused_before_sending() {
    let fake = Fake::new(data());
    let f = body_file("blank", " \n");
    fails(
        &run(
            &fake,
            TOKEN,
            &["say", "guild/tasks#247", "--body-file", f.to_str().unwrap()],
        ),
        1,
        &["the comment's body is empty"],
    );
    assert!(fake.requests().is_empty());
}

#[test]
fn a_missing_variable_exits_2_naming_it() {
    let fake = Fake::new(data());
    fails(
        &run_env(
            &[("GUILD_FORGEJO_URL", &fake.url)],
            &["say", "guild/tasks#247", "--text", "t"],
        ),
        2,
        &["GUILD_FORGEJO_TOKEN not set"],
    );
    fails(
        &run_env(
            &[("GUILD_FORGEJO_TOKEN", TOKEN)],
            &["correct", "guild/tasks", "10", "--text", "t"],
        ),
        2,
        &["GUILD_FORGEJO_URL not set"],
    );
    fails(
        &run_env(&[], &["say", "guild/tasks#247", "--text", "t"]),
        2,
        &["GUILD_FORGEJO_URL and GUILD_FORGEJO_TOKEN not set"],
    );
    assert!(fake.requests().is_empty());
}

#[test]
fn an_unreadable_body_file_fails_with_its_fix() {
    let fake = Fake::new(data());
    fails(
        &run(
            &fake,
            TOKEN,
            &["say", "guild/tasks#247", "--body-file", "/nonexistent/b.md"],
        ),
        1,
        &[
            "the body file /nonexistent/b.md could not be read",
            "write the comment's body",
        ],
    );
    assert!(fake.requests().is_empty());
}

#[test]
fn an_argument_it_does_not_take_exits_2_with_the_usage() {
    let o = run_env(&[], &["say", "guild/tasks", "--text", "t"]);
    fails(
        &o,
        2,
        &[
            "usage: forge-comment say",
            "not an <owner/repo>#<n>: guild/tasks",
        ],
    );
    let o = run_env(
        &[],
        &["say", "guild/tasks#1", "--text", "a", "--body-file", "f"],
    );
    fails(&o, 2, &["not both"]);
}

#[test]
fn help_and_version() {
    let o = run_env(&[], &["--help"]);
    assert_eq!(o.status.code(), Some(0));
    for s in [
        "say <owner/repo>#<n>",
        "correct <owner/repo> <comment-id>",
        "--dry-run",
        "Exits 2",
    ] {
        assert!(stdout(&o).contains(s), "{s}");
    }
    let o = run_env(&[], &["--version"]);
    assert_eq!(
        stdout(&o),
        format!("forge-comment {}\n", env!("CARGO_PKG_VERSION"))
    );
}
