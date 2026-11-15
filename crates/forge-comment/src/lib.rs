//! forge-comment: what it decides, without touching the world. The
//! arguments it takes, the one Forgejo request each command makes (its
//! method, path and JSON body), the line it prints from Forgejo's answer,
//! and what it says when Forgejo refuses. `world` reads the environment
//! and the body file, sends the request, and prints.

pub mod world;

use serde_json::{json, Value};

pub const NAME: &str = "forge-comment";

/// Exit code of an argument forge-comment does not take, and of a
/// GUILD_FORGEJO_URL or GUILD_FORGEJO_TOKEN not set.
pub const EXIT_USAGE: u8 = 2;
/// Exit code of a run that failed: the body file, or Forgejo's answer.
pub const EXIT_FAIL: u8 = 1;

pub const USAGE: &str = "usage: forge-comment say <owner/repo>#<n> (--text <t> | --body-file <f>) [--dry-run]\n       forge-comment correct <owner/repo> <comment-id> (--text <t> | --body-file <f>) [--dry-run]";

/// What `--help` answers.
pub const HELP: &str = concat!(
    "usage: forge-comment say <owner/repo>#<n> (--text <t> | --body-file <f>) [--dry-run]\n",
    "       forge-comment correct <owner/repo> <comment-id> (--text <t> | --body-file <f>) [--dry-run]\n",
    "       forge-comment --version\n\n",
    "Says something on an issue of Guild's Forgejo in one call, and prints plain text, never JSON. It signs in as the instance's own account: GUILD_FORGEJO_URL and GUILD_FORGEJO_TOKEN from the environment, which a Guild instance is given; the caller never fetches or handles a token, and forge-comment never prints it. Reading an issue and its comments is forge-read's.\n\n",
    "commands:\n",
    "  say <owner/repo>#<n>              post a new comment on the issue (or pull) <n>: POST /api/v1/repos/<owner>/<repo>/issues/<n>/comments\n",
    "  correct <owner/repo> <comment-id> replace the body of a comment the instance's account wrote: PATCH /api/v1/repos/<owner>/<repo>/issues/comments/<comment-id>\n",
    "                                    (the comment id is the number after #issuecomment- in its URL, or the first word say printed)\n\n",
    "options, of both:\n",
    "  --text <t>        the comment's body, as written, Markdown\n",
    "  --body-file <f>   a file holding the comment's body, as written, Markdown; give --text or --body-file, not both\n",
    "  --dry-run         send nothing, and need no GUILD_FORGEJO_URL or GUILD_FORGEJO_TOKEN; print the request it would send:\n",
    "                    would send <METHOD> /api/v1/<path> {\"body\":\"…\"}\n\n",
    "Both print one line: <comment-id> <url>, the comment's id and its html_url. say posts a new comment each time it runs; correct sets the body to the same text each time, so running it again is safe.\n\n",
    "Exits 0 with the line on stdout. Exits 1 with the cause and its fix on stderr when the body file cannot be read or the body is empty, or Forgejo answers an error (its status, the issue or comment, and Forgejo's message are printed: 401 or 403 for a token that may not, or a comment another account wrote; 404 for an issue or comment that is not there; 422 for a body Forgejo refuses) or cannot be reached. Exits 2 with the usage on stderr on an argument it does not take, and naming the variable when GUILD_FORGEJO_URL or GUILD_FORGEJO_TOKEN is not set.\n",
);

/// An `owner/repo` as Forgejo names it: two names of letters, digits,
/// `.`, `_` and `-`, so it can only ever name a repo's path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repo {
    pub owner: String,
    pub name: String,
}

fn is_name(p: &str) -> bool {
    !p.is_empty()
        && p != "."
        && p != ".."
        && p.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// A whole number from 1, written in digits only.
fn number(s: &str) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse::<u64>().ok().filter(|n| *n > 0)
}

impl Repo {
    pub fn parse(s: &str) -> Option<Repo> {
        let (owner, name) = s.split_once('/')?;
        (is_name(owner) && is_name(name)).then(|| Repo {
            owner: owner.into(),
            name: name.into(),
        })
    }
}

impl std::fmt::Display for Repo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.owner, self.name)
    }
}

/// An issue named as `<owner/repo>#<n>`, the number from 1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub repo: Repo,
    pub number: u64,
}

impl Issue {
    pub fn parse(s: &str) -> Option<Issue> {
        let (repo, n) = s.split_once('#')?;
        Some(Issue {
            repo: Repo::parse(repo)?,
            number: number(n)?,
        })
    }
}

impl std::fmt::Display for Issue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}#{}", self.repo, self.number)
    }
}

/// Where the comment's body comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Body {
    Text(String),
    File(String),
}

/// What a command says on: a new comment on an issue, or a comment of a
/// repo to correct.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    Say(Issue),
    Correct { repo: Repo, id: u64 },
}

impl std::fmt::Display for Target {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Target::Say(issue) => write!(f, "issue {issue}"),
            Target::Correct { repo, id } => write!(f, "comment {id} of {repo}"),
        }
    }
}

/// A comment to post or correct.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comment {
    pub target: Target,
    pub body: Body,
    pub dry_run: bool,
}

/// What the arguments ask.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked {
    Help,
    Version,
    Comment(Comment),
}

/// The arguments, after the program's name.
pub fn parse(args: &[String]) -> Result<Asked, String> {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        return Ok(Asked::Help);
    }
    let (command, rest) = match args.split_first() {
        Some((c, rest)) => (c.as_str(), rest),
        None => return Err("a command is required: say or correct".into()),
    };
    match command {
        "--version" | "-V" if rest.is_empty() => Ok(Asked::Version),
        "say" | "correct" => parse_comment(command, rest),
        other => Err(format!(
            "no such command: {other}; the commands are say and correct"
        )),
    }
}

fn parse_comment(command: &str, args: &[String]) -> Result<Asked, String> {
    let mut positional = Vec::new();
    let (mut text, mut body_file) = (None, None);
    let mut dry_run = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let (name, inline) = match a.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n, Some(v.to_string())),
            _ => (a.as_str(), None),
        };
        let slot = match name {
            "--text" => &mut text,
            "--body-file" => &mut body_file,
            "--dry-run" if inline.is_none() => {
                dry_run = true;
                continue;
            }
            n if n.starts_with('-') => return Err(format!("{command} takes no option {n}")),
            _ => {
                positional.push(a.as_str());
                continue;
            }
        };
        let value = match inline.or_else(|| it.next().cloned()) {
            Some(v) if !v.trim().is_empty() => v,
            _ => return Err(format!("{name} takes a value, not empty")),
        };
        if slot.is_some() {
            return Err(format!("{name} is given once"));
        }
        *slot = Some(value);
    }
    let target = if command == "say" {
        match positional.as_slice() {
            [] => return Err("<owner/repo>#<n> is required, as in guild/tasks#247".into()),
            [i, rest @ ..] => {
                if let Some(extra) = rest.first() {
                    return Err(format!("unrecognized argument: {extra}"));
                }
                Target::Say(Issue::parse(i).ok_or_else(|| {
                    format!("not an <owner/repo>#<n>: {i}; give it as in guild/tasks#247")
                })?)
            }
        }
    } else {
        match positional.as_slice() {
            [] | [_] => {
                return Err("<owner/repo> <comment-id> are required, as in guild/tasks 1234".into())
            }
            [r, id, rest @ ..] => {
                if let Some(extra) = rest.first() {
                    return Err(format!("unrecognized argument: {extra}"));
                }
                let repo = Repo::parse(r)
                    .ok_or_else(|| format!("not an owner/repo: {r}; give it as in guild/tasks"))?;
                let id = number(id).ok_or_else(|| {
                    format!(
                        "not a comment id: {id}; give the number after #issuecomment- in its URL"
                    )
                })?;
                Target::Correct { repo, id }
            }
        }
    };
    let body = match (text, body_file) {
        (Some(t), None) => Body::Text(t),
        (None, Some(f)) => Body::File(f),
        (Some(_), Some(_)) => return Err("give --text or --body-file, not both".into()),
        (None, None) => {
            return Err("--text <t> or --body-file <f> is required: the comment's body".into())
        }
    };
    Ok(Asked::Comment(Comment {
        target,
        body,
        dry_run,
    }))
}

/// The one request a command sends: its method, its path under /api/v1/,
/// and its JSON body.
#[derive(Clone, Debug, PartialEq)]
pub struct Request {
    pub method: &'static str,
    pub path: String,
    pub json: Value,
}

/// The request for a target and a body: say POSTs a new comment on the
/// issue, correct PATCHes the comment of the repo.
pub fn request(target: &Target, body: &str) -> Request {
    let (method, path) = match target {
        Target::Say(i) => (
            "POST",
            format!(
                "repos/{}/{}/issues/{}/comments",
                i.repo.owner, i.repo.name, i.number
            ),
        ),
        Target::Correct { repo, id } => (
            "PATCH",
            format!("repos/{}/{}/issues/comments/{id}", repo.owner, repo.name),
        ),
    };
    Request {
        method,
        path,
        json: json!({ "body": body }),
    }
}

/// The line `--dry-run` prints: `would send <METHOD> /api/v1/<path> <json>`.
pub fn render_dry_run(r: &Request) -> String {
    format!(
        "would send {} /api/v1/{} {}\n",
        r.method,
        r.path,
        serde_json::to_string(&r.json).unwrap_or_default()
    )
}

/// The line both commands print from Forgejo's comment:
/// `<comment-id> <html_url>`.
pub fn render_comment(comment: &Value) -> String {
    let id = comment.get("id").and_then(Value::as_u64).unwrap_or(0);
    let url = comment
        .get("html_url")
        .and_then(Value::as_str)
        .unwrap_or("");
    format!("{id} {url}\n")
}

/// What to do when Forgejo answers `status` to a request on `target`.
pub fn fix(status: u16, target: &Target) -> &'static str {
    match (status, target) {
        (401, _) => "run forge-comment with the token the instance was given in GUILD_FORGEJO_TOKEN, not a token of your own",
        (403, Target::Correct { .. }) => "correct only a comment the instance's account wrote; to answer another's, say a new comment with forge-comment say",
        (403, Target::Say(_)) => "the instance's account may not comment there (a locked issue, or a scope the token lacks): ask the foreman",
        (404, Target::Say(_)) => "check the owner/repo and the issue number with forge-read issue <owner/repo> <n>",
        (404, Target::Correct { .. }) => "check the owner/repo and the comment id (the number after #issuecomment- in its URL) with forge-read issue <owner/repo> <n> --comments",
        (422, _) => "fix what Forgejo's message names in the body, then run it again",
        _ => "run it again once Forgejo answers",
    }
}

/// Forgejo's message in an error answer, on one line, or the answer itself
/// when it is not Forgejo's JSON.
pub fn error_message(body: &str) -> String {
    let m = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.get("message").and_then(Value::as_str).map(String::from))
        .unwrap_or_else(|| body.trim().to_string());
    let line = m.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.is_empty() {
        "no message".into()
    } else {
        line.chars().take(500).collect()
    }
}

/// The variables of GUILD_FORGEJO_URL and GUILD_FORGEJO_TOKEN not set, from
/// their values (empty counts as not set).
pub fn missing(url: &str, token: &str) -> Vec<&'static str> {
    let mut out = Vec::new();
    if url.trim().is_empty() {
        out.push("GUILD_FORGEJO_URL");
    }
    if token.trim().is_empty() {
        out.push("GUILD_FORGEJO_TOKEN");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    fn comment(a: &[&str]) -> Comment {
        match parse(&args(a)) {
            Ok(Asked::Comment(c)) => c,
            other => panic!("{a:?}: {other:?}"),
        }
    }

    #[test]
    fn parses_say() {
        let c = comment(&["say", "guild/tasks#247", "--text", "waits for #250"]);
        assert_eq!(
            c.target,
            Target::Say(Issue::parse("guild/tasks#247").unwrap())
        );
        assert_eq!(c.body, Body::Text("waits for #250".into()));
        assert!(!c.dry_run);
        let c = comment(&["say", "--dry-run", "--body-file=b.md", "guild/tasks#1"]);
        assert_eq!(c.body, Body::File("b.md".into()));
        assert!(c.dry_run);
    }

    #[test]
    fn parses_correct() {
        let c = comment(&["correct", "guild/tasks", "1234", "--text", "t"]);
        assert_eq!(
            c.target,
            Target::Correct {
                repo: Repo::parse("guild/tasks").unwrap(),
                id: 1234
            }
        );
    }

    #[test]
    fn refuses_what_it_does_not_take() {
        for bad in [
            &[][..],
            &["post"],
            &["say"],
            &["say", "guild/tasks#247"],
            &["say", "guild/tasks", "--text", "t"],
            &["say", "guild/tasks#0", "--text", "t"],
            &["say", "guild/tasks#+1", "--text", "t"],
            &["say", "../x#1", "--text", "t"],
            &["say", "guild/tasks#1", "x", "--text", "t"],
            &["say", "guild/tasks#1", "--text", ""],
            &["say", "guild/tasks#1", "--text", " "],
            &["say", "guild/tasks#1", "--text"],
            &["say", "guild/tasks#1", "--text", "a", "--text", "b"],
            &["say", "guild/tasks#1", "--text", "a", "--body-file", "f"],
            &["say", "guild/tasks#1", "--text", "a", "--force"],
            &["say", "guild/tasks#1", "--text", "a", "--dry-run=yes"],
            &["correct", "guild/tasks", "--text", "t"],
            &["correct", "guild/tasks", "0", "--text", "t"],
            &["correct", "guild/tasks", "12a", "--text", "t"],
            &["correct", "guild/tasks#1", "12", "--text", "t"],
            &["correct", "guild/tasks", "12", "13", "--text", "t"],
            &["correct", "guild/tasks", "12"],
        ] {
            assert!(parse(&args(bad)).is_err(), "{bad:?}");
        }
        assert_eq!(parse(&args(&["say", "--help"])), Ok(Asked::Help));
        assert_eq!(parse(&args(&["--version"])), Ok(Asked::Version));
    }

    #[test]
    fn requests_are_forgejos() {
        let say = request(
            &Target::Say(Issue::parse("guild/tasks#247").unwrap()),
            "a \"b\"\n",
        );
        assert_eq!(say.method, "POST");
        assert_eq!(say.path, "repos/guild/tasks/issues/247/comments");
        assert_eq!(say.json, json!({"body": "a \"b\"\n"}));
        assert_eq!(
            render_dry_run(&say),
            "would send POST /api/v1/repos/guild/tasks/issues/247/comments {\"body\":\"a \\\"b\\\"\\n\"}\n"
        );
        let correct = request(
            &Target::Correct {
                repo: Repo::parse("guild/tasks").unwrap(),
                id: 9,
            },
            "t",
        );
        assert_eq!(correct.method, "PATCH");
        assert_eq!(correct.path, "repos/guild/tasks/issues/comments/9");
    }

    #[test]
    fn renders_id_and_url() {
        assert_eq!(
            render_comment(
                &json!({"id": 7, "html_url": "http://f/guild/tasks/issues/1#issuecomment-7"})
            ),
            "7 http://f/guild/tasks/issues/1#issuecomment-7\n"
        );
    }

    #[test]
    fn missing_names_each_variable() {
        assert_eq!(
            missing("", " "),
            ["GUILD_FORGEJO_URL", "GUILD_FORGEJO_TOKEN"]
        );
        assert_eq!(missing("http://f", ""), ["GUILD_FORGEJO_TOKEN"]);
        assert!(missing("http://f", "t").is_empty());
    }

    #[test]
    fn error_message_is_forgejos() {
        assert_eq!(error_message(r#"{"message":"a\nb","url":"x"}"#), "a b");
        assert_eq!(error_message("  bad gateway "), "bad gateway");
        assert_eq!(error_message(""), "no message");
    }
}
