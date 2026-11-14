//! forge-pull: what it decides, without touching the world. The arguments
//! it takes, the pull request body it sends, which pulls say they close a
//! task, a pull's review state, and the plain text it prints from what
//! Forgejo answered. `world` reads Forgejo, writes the one pull, and prints.

pub mod world;

use serde_json::{json, Value};

pub const NAME: &str = "forge-pull";

/// The most items Forgejo gives on one page (its MAX_RESPONSE_ITEMS
/// default), asked for on every page.
pub const PAGE_LIMIT: usize = 50;

/// No reading goes past this many pages of one list: a server that pages
/// wrongly cannot keep forge-pull reading for ever.
pub const MAX_PAGES: usize = 200;

/// The base branch `open` asks for when `--base` is not given.
pub const DEFAULT_BASE: &str = "main";

/// Exit code of an argument forge-pull does not take.
pub const EXIT_USAGE: u8 = 2;
/// Exit code of a run that failed: the environment, Forgejo's answer, the
/// body file, or `find` with no pull saying it closes the task.
pub const EXIT_FAIL: u8 = 1;

pub const USAGE: &str = "usage: forge-pull open <owner/repo> --head <branch> [--base main] --title <t> --body-file <f> [--dry-run]\n       forge-pull find <owner/repo>#<n>";

/// What `--help` answers.
pub const HELP: &str = concat!(
    "usage: forge-pull open <owner/repo> --head <branch> [--base main] --title <t> --body-file <f> [--dry-run]\n",
    "       forge-pull find <owner/repo>#<n>\n",
    "       forge-pull --version\n\n",
    "Opens a pull request on Guild's Forgejo, or finds the pulls that say they close a task, in one call, and prints plain text, never JSON. It signs in as the instance's own account: GUILD_FORGEJO_URL and GUILD_FORGEJO_TOKEN from the environment, which a Guild instance is given; the caller never fetches or handles a token, and forge-pull never prints it. open is its only write; find only reads.\n\n",
    "commands:\n",
    "  open <owner/repo>         open a pull request from a pushed branch; prints one line: <owner/repo>#<number> <url>\n",
    "    --head <branch>         the branch to merge, already pushed to <owner/repo> (or <user>:<branch> from a fork)\n",
    "    --base <branch>         the branch to merge into (main)\n",
    "    --title <t>             the pull's title, one line\n",
    "    --body-file <f>         a file holding the pull's body as written, Markdown; put the line Closes <owner/repo>#<n> in it to close a task on merge\n",
    "    --dry-run               send nothing; print the request open would send, or the open pull it would answer\n",
    "  find <owner/repo>#<n>     every pull of <owner>'s repos whose body says Closes <owner/repo>#<n>; prints one line per pull:\n",
    "                            <owner/repo>#<number> <state> head=<branch> sha=<sha> review=<review>\n\n",
    "open first asks whether a pull from --head into --base is already open; if it is, open sends nothing, prints that pull's line and says on stderr that it was already open, so running it again is safe. An open pull's title and body are not changed.\n\n",
    "find reads every repo of the owner and every pull of each, open, closed and merged, and keeps those whose body says Closes <owner/repo>#<n> (any case, the number whole: #4 is not #40). <state> is open, closed or merged. <review> is changes-requested when a reviewer's latest verdict asks for changes, else approved when one approved, else commented when there are only comments, else none; a dismissed review does not count. Lines are by repo, then number.\n\n",
    "Exits 0 with the answer on stdout. Exits 1 with the cause and its fix on stderr when GUILD_FORGEJO_URL or GUILD_FORGEJO_TOKEN is unset, the body file cannot be read, Forgejo answers an error (its status and message are printed) or cannot be reached, or find finds no pull saying it closes the task; exits 2 with the usage on stderr on an argument it does not take.\n",
);

/// An `owner/repo` as Forgejo names it: two names of letters, digits,
/// `.`, `_` and `-`, so it can only ever name a repo's path.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
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
pub struct Task {
    pub repo: Repo,
    pub number: u64,
}

impl Task {
    pub fn parse(s: &str) -> Option<Task> {
        let (repo, number) = s.split_once('#')?;
        let repo = Repo::parse(repo)?;
        if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let number = number.parse::<u64>().ok().filter(|n| *n > 0)?;
        Some(Task { repo, number })
    }
}

impl std::fmt::Display for Task {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}#{}", self.repo, self.number)
    }
}

/// A pull request to open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Open {
    pub repo: Repo,
    pub head: String,
    pub base: String,
    pub title: String,
    pub body_file: String,
    pub dry_run: bool,
}

/// What the arguments ask.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked {
    Help,
    Version,
    Open(Open),
    Find(Task),
}

/// The arguments, after the program's name.
pub fn parse(args: &[String]) -> Result<Asked, String> {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        return Ok(Asked::Help);
    }
    let (command, rest) = match args.split_first() {
        Some((c, rest)) => (c.as_str(), rest),
        None => return Err("a command is required: open or find".into()),
    };
    match command {
        "--version" | "-V" if rest.is_empty() => Ok(Asked::Version),
        "open" => parse_open(rest),
        "find" => parse_find(rest),
        other => Err(format!(
            "no such command: {other}; the commands are open and find"
        )),
    }
}

/// A branch name as git allows it, near enough: no space or control
/// character, no `..`, not starting with `-`.
fn is_branch(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('-')
        && !s.contains("..")
        && !s
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || "~^?*[\\".contains(c))
}

fn parse_open(args: &[String]) -> Result<Asked, String> {
    let mut positional = Vec::new();
    let (mut head, mut base, mut title, mut body_file) = (None, None, None, None);
    let mut dry_run = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let (name, inline) = match a.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n, Some(v.to_string())),
            _ => (a.as_str(), None),
        };
        let slot = match name {
            "--head" => &mut head,
            "--base" => &mut base,
            "--title" => &mut title,
            "--body-file" => &mut body_file,
            "--dry-run" if inline.is_none() => {
                dry_run = true;
                continue;
            }
            n if n.starts_with('-') => return Err(format!("open takes no option {n}")),
            _ => {
                positional.push(a);
                continue;
            }
        };
        let value = match inline.or_else(|| it.next().cloned()) {
            Some(v) if !v.trim().is_empty() => v,
            _ => return Err(format!("{name} takes a value")),
        };
        if slot.is_some() {
            return Err(format!("{name} is given once"));
        }
        *slot = Some(value);
    }
    let repo = match positional.first() {
        None => return Err("<owner/repo> is required, as in guild/forge".into()),
        Some(r) => Repo::parse(r)
            .ok_or_else(|| format!("not an owner/repo: {r}; give it as in guild/forge"))?,
    };
    if let Some(extra) = positional.get(1) {
        return Err(format!("unrecognized argument: {extra}"));
    }
    let head = head.ok_or("--head <branch> is required: the pushed branch to merge")?;
    let head_branch = head.split_once(':').map_or(head.as_str(), |(_, b)| b);
    if !is_branch(head_branch) {
        return Err(format!("not a branch name: --head {head}"));
    }
    let base = base.unwrap_or_else(|| DEFAULT_BASE.to_string());
    if !is_branch(&base) || base.contains(':') {
        return Err(format!("not a branch name: --base {base}"));
    }
    let title = title.ok_or("--title <t> is required")?;
    if title.contains(['\r', '\n']) {
        return Err("--title is one line".into());
    }
    let body_file = body_file.ok_or("--body-file <f> is required: the file holding the body")?;
    Ok(Asked::Open(Open {
        repo,
        head,
        base,
        title,
        body_file,
        dry_run,
    }))
}

fn parse_find(args: &[String]) -> Result<Asked, String> {
    match args {
        [] => Err("<owner/repo>#<n> is required, as in guild/tasks#48".into()),
        [a] if a.starts_with('-') => Err(format!("find takes no option {a}")),
        [t] => Task::parse(t)
            .map(Asked::Find)
            .ok_or_else(|| format!("not an <owner/repo>#<n>: {t}; give it as in guild/tasks#48")),
        [_, extra, ..] => Err(format!("unrecognized argument: {extra}")),
    }
}

/// The JSON body of Forgejo's create-pull call.
pub fn open_request(open: &Open, body: &str) -> Value {
    json!({
        "head": open.head,
        "base": open.base,
        "title": open.title,
        "body": body,
    })
}

/// Whether to ask for the page after one just read: it was full, it
/// brought something not seen before, Forgejo's total (when it said one)
/// is not reached, and the page cap is not reached.
pub fn more(
    page: usize,
    page_len: usize,
    new_items: usize,
    seen: usize,
    total: Option<usize>,
) -> bool {
    page < MAX_PAGES && page_len >= PAGE_LIMIT && new_items > 0 && total.is_none_or(|t| seen < t)
}

/// Whether a body says it closes the task: `Closes <owner/repo>#<n>`, the
/// keyword in any case and starting a word, the number whole, so
/// `#4` is not said by `#40` and `#40` is not said by `#4`.
pub fn closes(body: &str, task: &Task) -> bool {
    let hay = body.to_ascii_lowercase();
    let needle = format!("closes {task}").to_ascii_lowercase();
    let mut from = 0;
    while let Some(at) = hay[from..].find(&needle) {
        let start = from + at;
        let end = start + needle.len();
        let before_ok = hay[..start]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric() && c != '_');
        let after_ok = hay[end..]
            .chars()
            .next()
            .is_none_or(|c| !c.is_alphanumeric() && c != '_');
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

/// A pull's state: Forgejo's open or closed, and merged for a merged pull.
pub fn pull_state(pull: &Value) -> &str {
    if pull.get("merged").and_then(Value::as_bool).unwrap_or(false) {
        "merged"
    } else {
        text(pull, "state")
    }
}

/// A pull's review state from its reviews, oldest first: each reviewer's
/// latest verdict (approved or changes asked) counts, a dismissed review
/// never; changes-requested over approved over commented over none.
pub fn review_state(reviews: &[Value]) -> &'static str {
    let mut verdicts: Vec<(&str, &str)> = Vec::new();
    let mut commented = false;
    for r in reviews {
        if r.get("dismissed").and_then(Value::as_bool).unwrap_or(false) {
            continue;
        }
        let who = r.get("user").map(|u| text(u, "login")).unwrap_or("");
        match text(r, "state") {
            s @ ("APPROVED" | "REQUEST_CHANGES") => {
                verdicts.retain(|(w, _)| *w != who);
                verdicts.push((who, s));
            }
            "COMMENT" => commented = true,
            _ => {}
        }
    }
    if verdicts.iter().any(|(_, s)| *s == "REQUEST_CHANGES") {
        "changes-requested"
    } else if !verdicts.is_empty() {
        "approved"
    } else if commented {
        "commented"
    } else {
        "none"
    }
}

/// The line `open` prints: `<owner/repo>#<number> <url>`.
pub fn render_opened(repo: &Repo, pull: &Value) -> String {
    let number = pull.get("number").and_then(Value::as_u64).unwrap_or(0);
    format!("{repo}#{number} {}\n", text(pull, "html_url"))
}

/// One line of `find`:
/// `<owner/repo>#<number> <state> head=<branch> sha=<sha> review=<review>`.
pub fn render_found(repo: &Repo, pull: &Value, review: &str) -> String {
    let number = pull.get("number").and_then(Value::as_u64).unwrap_or(0);
    let head = pull.get("head");
    let field = |k: &str| head.map(|h| text(h, k)).unwrap_or("");
    format!(
        "{repo}#{number} {} head={} sha={} review={review}\n",
        pull_state(pull),
        field("ref"),
        field("sha"),
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    fn task(s: &str) -> Task {
        Task::parse(s).unwrap()
    }

    #[test]
    fn parses_open() {
        let Ok(Asked::Open(o)) = parse(&args(&[
            "open",
            "guild/forge",
            "--head",
            "guild/tasks-226",
            "--title=A title",
            "--body-file",
            "b.md",
        ])) else {
            panic!()
        };
        assert_eq!(
            (o.head.as_str(), o.base.as_str(), o.title.as_str()),
            ("guild/tasks-226", "main", "A title")
        );
        assert!(!o.dry_run);
        for bad in [
            &["open"][..],
            &["open", "guild/forge", "--title", "t", "--body-file", "f"],
            &["open", "guild/forge", "--head", "h", "--body-file", "f"],
            &["open", "guild/forge", "--head", "h", "--title", "t"],
            &[
                "open",
                "guild/forge",
                "--head",
                "a b",
                "--title",
                "t",
                "--body-file",
                "f",
            ],
            &[
                "open",
                "guild/forge",
                "--head",
                "h",
                "--head",
                "i",
                "--title",
                "t",
                "--body-file",
                "f",
            ],
            &[
                "open",
                "guild/forge",
                "--head",
                "h",
                "--title",
                "a\nb",
                "--body-file",
                "f",
            ],
            &[
                "open",
                "guild/forge",
                "--head",
                "h",
                "--title",
                "t",
                "--body-file",
                "f",
                "--x",
            ],
            &[
                "open",
                "../x",
                "--head",
                "h",
                "--title",
                "t",
                "--body-file",
                "f",
            ],
        ] {
            assert!(parse(&args(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn parses_find() {
        assert_eq!(
            parse(&args(&["find", "guild/tasks#48"])),
            Ok(Asked::Find(task("guild/tasks#48")))
        );
        for bad in [
            "guild/tasks",
            "guild/tasks#",
            "guild/tasks#0",
            "guild/tasks#+4",
            "#4",
            "a/b/c#1",
        ] {
            assert!(parse(&args(&["find", bad])).is_err(), "{bad}");
        }
        assert!(parse(&args(&["find"])).is_err());
        assert!(parse(&args(&["find", "guild/tasks#1", "x"])).is_err());
    }

    #[test]
    fn closes_matches_the_whole_number() {
        let t = task("guild/tasks#48");
        assert!(closes("Closes guild/tasks#48\n\nmore", &t));
        assert!(closes("text. closes guild/tasks#48.", &t));
        assert!(closes("CLOSES Guild/Tasks#48", &t));
        assert!(!closes("Closes guild/tasks#480", &t));
        assert!(!closes("Closes guild/tasks#4", &t));
        assert!(!closes("Encloses guild/tasks#48", &t));
        assert!(!closes("Closes guild/tasks-x#48", &t));
        assert!(!closes("Refs guild/tasks#48", &t));
        assert!(closes("Closes guild/tasks#480, Closes guild/tasks#48", &t));
        assert!(closes("Closes guild/tasks#4", &task("guild/tasks#4")));
    }

    #[test]
    fn review_state_by_latest_verdict() {
        let r = |who: &str, s: &str, dismissed: bool| json!({"user": {"login": who}, "state": s, "dismissed": dismissed});
        assert_eq!(review_state(&[]), "none");
        assert_eq!(review_state(&[r("a", "COMMENT", false)]), "commented");
        assert_eq!(review_state(&[r("a", "APPROVED", false)]), "approved");
        assert_eq!(
            review_state(&[r("a", "REQUEST_CHANGES", false), r("a", "APPROVED", false)]),
            "approved"
        );
        assert_eq!(
            review_state(&[r("a", "APPROVED", false), r("b", "REQUEST_CHANGES", false)]),
            "changes-requested"
        );
        assert_eq!(
            review_state(&[r("b", "REQUEST_CHANGES", true), r("a", "APPROVED", false)]),
            "approved"
        );
        assert_eq!(review_state(&[r("a", "PENDING", false)]), "none");
    }

    #[test]
    fn error_message_is_forgejos() {
        assert_eq!(error_message(r#"{"message":"a\nb","url":"x"}"#), "a b");
        assert_eq!(error_message("  bad gateway "), "bad gateway");
        assert_eq!(error_message(""), "no message");
    }
}
