//! forge-read's binary against a stand-in Forgejo: an issue with and
//! without its comments (more than one page of them), a list by label and
//! by state, the pulls of a repo from its pulls endpoint with head, base
//! and sha (more than one page of them) by state, label and head branch, a
//! not-found issue, and each refusal with its fix. Offline:
//! nothing reaches a real forge.

mod forgejo;

use std::process::{Command, Output};

use forgejo::{Data, Fake, TOKEN};
use serde_json::{json, Value};

const BIN: &str = env!("CARGO_BIN_EXE_forge-read");

fn label(name: &str) -> Value {
    json!({"id": name.len(), "name": name, "color": "ffffff"})
}

fn item(number: u64, state: &str, labels: &[&str], title: &str, pull: Option<bool>) -> Value {
    json!({
        "id": 1000 + number,
        "number": number,
        "title": title,
        "state": state,
        "labels": labels.iter().map(|l| label(l)).collect::<Vec<_>>(),
        "user": {"login": "felix"},
        "created_at": format!("2026-11-{:02}T10:00:00Z", number % 28 + 1),
        "body": format!("Body of #{number}.\n\nSecond paragraph."),
        "pull_request": pull.map(|m| json!({"merged": m})),
    })
}

fn pull(number: u64, state: &str, merged: bool, labels: &[&str], head: &str, title: &str) -> Value {
    json!({
        "id": 7000 + number,
        "number": number,
        "title": title,
        "state": state,
        "merged": merged,
        "labels": labels.iter().map(|l| label(l)).collect::<Vec<_>>(),
        "user": {"login": "felix"},
        "head": {"ref": head, "sha": format!("{number:04}abcdef0123456789abcdef0123456789abcd")},
        "base": {"ref": "main"},
    })
}

/// Bulk pulls #1..=#60 and three by hand: more than one page of 50, served
/// oldest first so newest first is forge-read's own doing.
fn pulls() -> Vec<Value> {
    let mut out: Vec<Value> = (1..=60)
        .map(|n| {
            pull(
                n,
                "open",
                false,
                &["bulk"],
                &format!("guild/tasks-{n}"),
                &format!("Bulk pull {n}"),
            )
        })
        .collect();
    out.push(pull(
        88,
        "closed",
        false,
        &["pr"],
        "guild/tasks-88",
        "A pull, closed unmerged",
    ));
    out.push(pull(
        89,
        "closed",
        true,
        &["pr", "tool-request"],
        "guild/tasks-226",
        "forge-pull: open a pull",
    ));
    out.push(pull(
        90,
        "open",
        false,
        &["pr"],
        "guild/tasks-226",
        "A pull, open, again from tasks-226",
    ));
    out
}

/// 120 comments on #64: three pages of Forgejo's 50.
const COMMENTS: usize = 120;

fn data() -> Data {
    let mut issues = vec![
        item(90, "open", &["pr"], "A pull, open", Some(false)),
        item(
            89,
            "closed",
            &["pr", "tool-request"],
            "A pull, merged",
            Some(true),
        ),
        item(
            88,
            "closed",
            &["pr"],
            "A pull, closed unmerged",
            Some(false),
        ),
        item(
            64,
            "open",
            &["goal", "tool-request"],
            "tool-request: read Forgejo",
            None,
        ),
        item(63, "closed", &["task"], "A closed task", None),
        item(
            61,
            "closed",
            &["goal", "tool-request"],
            "tool-request: read a pull",
            None,
        ),
        item(1, "open", &["task"], "A first issue", None),
    ];
    // Enough open plain tasks that a listing spans more than one page.
    for n in 2..=60 {
        issues.push(item(
            1000 + n,
            "open",
            &["task", "bulk"],
            &format!("Bulk {n}"),
            None,
        ));
    }
    let comments = (1..=COMMENTS)
        .map(|i| {
            json!({"id": 5000 + i, "body": format!("Comment {i}."),
                   "user": {"login": if i % 2 == 0 { "steward" } else { "warden" }},
                   "created_at": format!("2026-11-14T{:02}:{:02}:00Z", i / 60, i % 60)})
        })
        .collect();
    Data {
        issues,
        comments: vec![(64, comments), (1, vec![])],
        pulls: pulls(),
    }
}

fn run(fake: &Fake, token: &str, args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .env_clear()
        .env("GUILD_FORGEJO_URL", &fake.url)
        .env("GUILD_FORGEJO_TOKEN", token)
        .output()
        .expect("forge-read runs")
}

fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}

fn stderr(o: &Output) -> String {
    String::from_utf8(o.stderr.clone()).unwrap()
}

fn only_gets(fake: &Fake) {
    let requests = fake.requests();
    assert!(!requests.is_empty());
    for r in &requests {
        assert!(r.starts_with("GET /api/v1/"), "not a GET: {r}");
        assert!(!r.contains(TOKEN), "token in a URL: {r}");
    }
}

#[test]
fn issue_prints_its_fields_and_body() {
    let fake = Fake::new(data());
    let o = run(&fake, TOKEN, &["issue", "guild/tasks", "64"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(
        stdout(&o),
        "number: 64\ntitle: tool-request: read Forgejo\nstate: open\nlabels: goal, tool-request\nauthor: felix\ncreated: 2026-11-09T10:00:00Z\n\nBody of #64.\n\nSecond paragraph.\n"
    );
    assert!(!stdout(&o).contains("comment"));
    assert_eq!(
        fake.requests().len(),
        1,
        "no comments read without --comments"
    );
    only_gets(&fake);
}

#[test]
fn issue_comments_paging_reads_every_page() {
    let fake = Fake::new(data());
    let o = run(&fake, TOKEN, &["issue", "guild/tasks", "64", "--comments"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let out = stdout(&o);
    assert!(out.starts_with("number: 64\n"));
    assert!(out.contains(&format!("\ncomments: {COMMENTS}\n")));
    assert_eq!(out.matches("\n--- comment ").count(), COMMENTS);
    assert!(out.contains("\n--- comment 1 of 120 by warden at 2026-11-14T00:01:00Z\nComment 1.\n"));
    assert!(
        out.contains("\n--- comment 51 of 120 by warden at 2026-11-14T00:51:00Z\nComment 51.\n")
    );
    assert!(out
        .ends_with("\n--- comment 120 of 120 by steward at 2026-11-14T02:00:00Z\nComment 120.\n"));
    let pages: Vec<String> = fake
        .requests()
        .into_iter()
        .filter(|r| r.contains("/comments"))
        .collect();
    assert_eq!(pages.len(), 3, "three pages of 50: {pages:?}");
    only_gets(&fake);
}

#[test]
fn issue_comments_none() {
    let fake = Fake::new(data());
    let o = run(&fake, TOKEN, &["issue", "guild/tasks", "1", "--comments"]);
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert!(stdout(&o).ends_with("\ncomments: 0\n"));
}

#[test]
fn issue_not_found() {
    let fake = Fake::new(data());
    let o = run(&fake, TOKEN, &["issue", "guild/tasks", "999"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(stdout(&o), "");
    let err = stderr(&o);
    assert!(err.contains("guild/tasks#999 not found"), "{err}");
    assert!(err.contains("check the owner/repo and number"), "{err}");
    let o = run(&fake, TOKEN, &["list", "guild/nope", "issues"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(stderr(&o).contains("guild/nope not found"));
}

#[test]
fn list_by_label() {
    let fake = Fake::new(data());
    let o = run(
        &fake,
        TOKEN,
        &[
            "list",
            "guild/tasks",
            "issues",
            "--label",
            "tool-request",
            "--state",
            "all",
        ],
    );
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(
        stdout(&o),
        "#64 open [goal,tool-request] tool-request: read Forgejo\n#61 closed [goal,tool-request] tool-request: read a pull\n"
    );
    // Two labels: each is required, though Forgejo matches either.
    let o = run(
        &fake,
        TOKEN,
        &[
            "list",
            "guild/tasks",
            "pulls",
            "--label",
            "pr",
            "--label=tool-request",
            "--state=all",
        ],
    );
    assert_eq!(
        stdout(&o),
        "#89 merged guild/tasks-226 -> main 0089abcdef01 [pr,tool-request] forge-pull: open a pull\n"
    );
    only_gets(&fake);
}

#[test]
fn list_by_state() {
    let fake = Fake::new(data());
    let o = run(
        &fake,
        TOKEN,
        &["list", "guild/tasks", "issues", "--state", "closed"],
    );
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(
        stdout(&o),
        "#63 closed [task] A closed task\n#61 closed [goal,tool-request] tool-request: read a pull\n"
    );
    // Open is the default; 61 open issues span two pages.
    let o = run(&fake, TOKEN, &["list", "guild/tasks", "issues"]);
    let out = stdout(&o);
    assert_eq!(out.lines().count(), 61);
    assert!(out.lines().all(|l| l.split(' ').nth(1) == Some("open")));
    assert!(out.ends_with("#1060 open [task,bulk] Bulk 60\n"));
    let o = run(
        &fake,
        TOKEN,
        &["list", "guild/tasks", "pulls", "--state", "all"],
    );
    assert!(stdout(&o).starts_with(
        "#90 open guild/tasks-226 -> main 0090abcdef01 [pr] A pull, open, again from tasks-226\n#89 merged guild/tasks-226 -> main 0089abcdef01 [pr,tool-request] forge-pull: open a pull\n#88 closed guild/tasks-88 -> main 0088abcdef01 [pr] A pull, closed unmerged\n"
    ));
    only_gets(&fake);
}

#[test]
fn refuses_a_wrong_token_with_its_fix() {
    let fake = Fake::new(data());
    let o = run(&fake, "not-the-token", &["issue", "guild/tasks", "64"]);
    assert_eq!(o.status.code(), Some(1));
    let err = stderr(&o);
    assert!(
        err.contains("Forgejo refused the token in GUILD_FORGEJO_TOKEN (401)"),
        "{err}"
    );
    assert!(err.contains("run forge-read with the token the instance was given"));
    assert!(!err.contains("not-the-token"), "the token is never printed");
}

#[test]
fn refuses_a_missing_environment_with_its_fix() {
    let o = Command::new(BIN)
        .args(["issue", "guild/tasks", "64"])
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
        err.contains("run forge-read inside a Guild instance"),
        "{err}"
    );
}

#[test]
fn refuses_bad_arguments_with_usage() {
    let fake = Fake::new(data());
    for args in [
        &["issue", "guild/tasks"][..],
        &["issue", "guild/tasks", "x"],
        &["list", "guild/tasks", "commits"],
        &["list", "guild/tasks", "issues", "--state", "merged"],
        &["pull", "guild/forge", "1"],
        &[],
    ] {
        let o = run(&fake, TOKEN, args);
        assert_eq!(o.status.code(), Some(2), "{args:?}");
        let err = stderr(&o);
        assert!(err.starts_with("usage: forge-read issue"), "{err}");
        assert!(err.contains("see forge-read --help"), "{err}");
    }
    assert!(
        fake.requests().is_empty(),
        "a refused command reads nothing"
    );
}

#[test]
fn help_and_version() {
    let fake = Fake::new(data());
    let o = run(&fake, TOKEN, &["--help"]);
    assert_eq!(o.status.code(), Some(0));
    let help = stdout(&o);
    assert!(help.contains("forge-read issue <owner/repo> <n> [--comments]"));
    assert!(help.contains("--state open|closed|all"));
    let o = run(&fake, TOKEN, &["--version"]);
    assert_eq!(
        stdout(&o),
        format!("forge-read {}\n", env!("CARGO_PKG_VERSION"))
    );
}

const SHA89: &str = "0089abcdef01";

#[test]
fn list_pulls_prints_head_base_sha_newest_first_across_pages() {
    let fake = Fake::new(data());
    let o = run(
        &fake,
        TOKEN,
        &["list", "guild/tasks", "pulls", "--state", "all"],
    );
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    let out = stdout(&o);
    let lines: Vec<&str> = out.lines().collect();
    assert_eq!(lines.len(), 63, "every page read: {out}");
    assert_eq!(
        &lines[..4],
        [
            "#90 open guild/tasks-226 -> main 0090abcdef01 [pr] A pull, open, again from tasks-226",
            &format!("#89 merged guild/tasks-226 -> main {SHA89} [pr,tool-request] forge-pull: open a pull"),
            "#88 closed guild/tasks-88 -> main 0088abcdef01 [pr] A pull, closed unmerged",
            "#60 open guild/tasks-60 -> main 0060abcdef01 [bulk] Bulk pull 60",
        ]
    );
    assert_eq!(
        lines[62],
        "#1 open guild/tasks-1 -> main 0001abcdef01 [bulk] Bulk pull 1"
    );
    let reads: Vec<String> = fake.requests();
    assert!(
        reads
            .iter()
            .all(|r| r.starts_with("GET /api/v1/repos/guild/tasks/pulls?")),
        "the pulls endpoint, not issue search: {reads:?}"
    );
    assert_eq!(reads.len(), 2, "two pages of 50: {reads:?}");
    only_gets(&fake);
}

#[test]
fn filters_narrow_list_pulls_by_state_label_and_head() {
    let fake = Fake::new(data());
    let lines = |args: &[&str]| {
        let o = run(&fake, TOKEN, args);
        assert_eq!(o.status.code(), Some(0), "{args:?}: {}", stderr(&o));
        stdout(&o)
    };
    // Open is the default: the two closed pulls are not listed.
    let open = lines(&["list", "guild/tasks", "pulls"]);
    assert_eq!(open.lines().count(), 61);
    assert!(open.lines().all(|l| l.split(' ').nth(1) == Some("open")));
    // Closed takes in the merged pull.
    assert_eq!(
        lines(&["list", "guild/tasks", "pulls", "--state", "closed"]),
        format!("#89 merged guild/tasks-226 -> main {SHA89} [pr,tool-request] forge-pull: open a pull\n#88 closed guild/tasks-88 -> main 0088abcdef01 [pr] A pull, closed unmerged\n")
    );
    // Each label is required.
    assert_eq!(
        lines(&["list", "guild/tasks", "pulls", "--state=all", "--label", "pr", "--label", "tool-request"]),
        format!("#89 merged guild/tasks-226 -> main {SHA89} [pr,tool-request] forge-pull: open a pull\n")
    );
    // The head branch, whole: guild/tasks-2 is not guild/tasks-226.
    assert_eq!(
        lines(&["list", "guild/tasks", "pulls", "--state", "all", "--head", "guild/tasks-226"]),
        format!("#90 open guild/tasks-226 -> main 0090abcdef01 [pr] A pull, open, again from tasks-226\n#89 merged guild/tasks-226 -> main {SHA89} [pr,tool-request] forge-pull: open a pull\n")
    );
    assert_eq!(
        lines(&["list", "guild/tasks", "pulls", "--state=closed", "--head=guild/tasks-226"]),
        format!("#89 merged guild/tasks-226 -> main {SHA89} [pr,tool-request] forge-pull: open a pull\n")
    );
    assert_eq!(
        lines(&["list", "guild/tasks", "pulls", "--head", "guild/tasks-2"]),
        "#2 open guild/tasks-2 -> main 0002abcdef01 [bulk] Bulk pull 2\n"
    );
    assert_eq!(
        lines(&["list", "guild/tasks", "pulls", "--head", "nobody"]),
        ""
    );
    only_gets(&fake);
}

#[test]
fn filters_refuse_head_twice_or_without_a_value_with_usage() {
    let fake = Fake::new(data());
    for args in [
        &["list", "guild/tasks", "pulls", "--head", "a", "--head", "b"][..],
        &["list", "guild/tasks", "pulls", "--head"],
        &["list", "guild/tasks", "pulls", "--head="],
        &["list", "guild/tasks", "issues", "--head", "a"],
    ] {
        let o = run(&fake, TOKEN, args);
        assert_eq!(o.status.code(), Some(2), "{args:?}");
        assert_eq!(stdout(&o), "");
        let err = stderr(&o);
        assert!(err.starts_with("usage: forge-read issue"), "{err}");
        assert!(err.contains("[--head <branch>]"), "{err}");
        assert!(err.contains("--head"), "{err}");
        assert!(err.contains("see forge-read --help"), "{err}");
    }
    let err = stderr(&run(
        &fake,
        TOKEN,
        &["list", "guild/tasks", "pulls", "--head", "a", "--head", "b"],
    ));
    assert!(err.contains("--head is given once"), "{err}");
    let err = stderr(&run(
        &fake,
        TOKEN,
        &["list", "guild/tasks", "pulls", "--head"],
    ));
    assert!(err.contains("--head takes a value"), "{err}");
    assert!(
        fake.requests().is_empty(),
        "a refused command reads nothing"
    );
}

#[test]
fn filters_leave_list_issues_as_before() {
    let fake = Fake::new(data());
    let o = run(
        &fake,
        TOKEN,
        &["list", "guild/tasks", "issues", "--state", "closed"],
    );
    assert_eq!(o.status.code(), Some(0), "{}", stderr(&o));
    assert_eq!(
        stdout(&o),
        "#63 closed [task] A closed task\n#61 closed [goal,tool-request] tool-request: read a pull\n"
    );
    assert!(fake.requests().iter().all(
        |r| r.starts_with("GET /api/v1/repos/guild/tasks/issues?") && r.contains("type=issues")
    ));
}
