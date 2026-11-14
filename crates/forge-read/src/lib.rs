//! forge-read: what it decides, without touching the world. The arguments
//! it takes, the pages it asks Forgejo for, and the plain text it prints
//! from what Forgejo answered. `world` reads Forgejo and prints.

pub mod world;

use serde_json::Value;

pub const NAME: &str = "forge-read";

/// The most items Forgejo gives on one page (its MAX_RESPONSE_ITEMS
/// default), asked for on every page.
pub const PAGE_LIMIT: usize = 50;

/// No reading goes past this many pages: 50 × 200 items is far more than
/// any repo of Guild's holds, and a server that pages wrongly cannot keep
/// forge-read reading for ever.
pub const MAX_PAGES: usize = 200;

/// Exit code of an argument forge-read does not take.
pub const EXIT_USAGE: u8 = 2;
/// Exit code of a read that failed: the environment, Forgejo, or not found.
pub const EXIT_FAIL: u8 = 1;

pub const USAGE: &str = "usage: forge-read issue <owner/repo> <n> [--comments]\n       forge-read list <owner/repo> issues|pulls [--label <name>]... [--state open|closed|all]";

/// What `--help` answers.
pub const HELP: &str = concat!(
    "usage: forge-read issue <owner/repo> <n> [--comments]\n",
    "       forge-read list <owner/repo> issues|pulls [--label <name>]... [--state open|closed|all]\n",
    "       forge-read --version\n\n",
    "Reads Guild's Forgejo in one call, read-only (GET only), and prints plain text, never JSON. It signs in as the instance's own account: GUILD_FORGEJO_URL and GUILD_FORGEJO_TOKEN from the environment, which a Guild instance is given; the caller never fetches or handles a token, and forge-read never prints it.\n\n",
    "commands:\n",
    "  issue <owner/repo> <n>    one issue (or pull, read as an issue): its number, title, state, labels, author, time and body\n",
    "    --comments              then every comment, oldest first, each with its author and time; every page is read, not only the first\n",
    "  list <owner/repo> issues|pulls\n",
    "                            one line per item, newest first: #<number> <state> [<labels>] <title>\n",
    "    --label <name>          only items carrying this label; give it again to require each of several\n",
    "    --state open|closed|all which items by state (open); a merged pull's state is printed merged\n\n",
    "issue prints, each on its own line: number: <n>, title: <title>, state: <state>, labels: <a, b>, author: <login>, created: <time>, then a blank line and the body as written. With --comments: a blank line, comments: <count>, and for each comment a blank line, --- comment <i> of <count> by <login> at <time>, and its body.\n\n",
    "Exits 0 with the answer on stdout. Exits 1 with the cause and its fix on stderr when GUILD_FORGEJO_URL or GUILD_FORGEJO_TOKEN is unset, Forgejo refuses the token or cannot be reached, or the repo or issue is not found; exits 2 with the usage on stderr on an argument it does not take.\n",
);

/// An `owner/repo` as Forgejo names it: two names of letters, digits,
/// `.`, `_` and `-`, so it can only ever name a repo's path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repo {
    pub owner: String,
    pub name: String,
}

impl Repo {
    pub fn parse(s: &str) -> Option<Repo> {
        let (owner, name) = s.split_once('/')?;
        let ok = |p: &str| {
            !p.is_empty()
                && p != "."
                && p != ".."
                && p.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        };
        (ok(owner) && ok(name)).then(|| Repo {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Issues,
    Pulls,
}

impl Kind {
    /// The `type` Forgejo's issue search takes.
    pub fn query(self) -> &'static str {
        match self {
            Kind::Issues => "issues",
            Kind::Pulls => "pulls",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Open,
    Closed,
    All,
}

impl State {
    pub fn query(self) -> &'static str {
        match self {
            State::Open => "open",
            State::Closed => "closed",
            State::All => "all",
        }
    }
}

/// What the arguments ask.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked {
    Help,
    Version,
    Issue {
        repo: Repo,
        number: u64,
        comments: bool,
    },
    List {
        repo: Repo,
        kind: Kind,
        labels: Vec<String>,
        state: State,
    },
}

/// The arguments, after the program's name.
pub fn parse(args: &[String]) -> Result<Asked, String> {
    if args.iter().any(|a| a == "-h" || a == "--help") {
        return Ok(Asked::Help);
    }
    let (command, rest) = match args.split_first() {
        Some((c, rest)) => (c.as_str(), rest),
        None => return Err("a command is required: issue or list".into()),
    };
    match command {
        "--version" | "-V" if rest.is_empty() => Ok(Asked::Version),
        "issue" => parse_issue(rest),
        "list" => parse_list(rest),
        other => Err(format!(
            "no such command: {other}; the commands are issue and list"
        )),
    }
}

fn repo_of(arg: Option<&String>) -> Result<Repo, String> {
    let Some(arg) = arg else {
        return Err("<owner/repo> is required, as in guild/tasks".into());
    };
    Repo::parse(arg).ok_or_else(|| format!("not an owner/repo: {arg}; give it as in guild/tasks"))
}

fn parse_issue(args: &[String]) -> Result<Asked, String> {
    let mut positional = Vec::new();
    let mut comments = false;
    for a in args {
        match a.as_str() {
            "--comments" => comments = true,
            a if a.starts_with('-') => return Err(format!("issue takes no option {a}")),
            _ => positional.push(a),
        }
    }
    let repo = repo_of(positional.first().copied())?;
    let number = match positional.get(1) {
        None => return Err("the issue's number <n> is required, as in 64".into()),
        Some(n) => n
            .trim_start_matches('#')
            .parse::<u64>()
            .ok()
            .filter(|n| *n > 0)
            .ok_or_else(|| format!("not an issue number: {n}; give it as in 64"))?,
    };
    if let Some(extra) = positional.get(2) {
        return Err(format!("unrecognized argument: {extra}"));
    }
    Ok(Asked::Issue {
        repo,
        number,
        comments,
    })
}

fn parse_list(args: &[String]) -> Result<Asked, String> {
    let mut positional = Vec::new();
    let mut labels = Vec::new();
    let mut state = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let (name, inline) = match a.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n, Some(v.to_string())),
            _ => (a.as_str(), None),
        };
        match name {
            "--label" | "--state" => {
                let value = match inline.or_else(|| it.next().cloned()) {
                    Some(v) if !v.is_empty() => v,
                    _ => return Err(format!("{name} takes a value")),
                };
                if name == "--label" {
                    labels.push(value);
                } else {
                    if state.is_some() {
                        return Err("--state is given once".into());
                    }
                    state = Some(match value.as_str() {
                        "open" => State::Open,
                        "closed" => State::Closed,
                        "all" => State::All,
                        v => return Err(format!("--state takes open, closed or all, not {v}")),
                    });
                }
            }
            n if n.starts_with('-') => return Err(format!("list takes no option {n}")),
            _ => positional.push(a),
        }
    }
    let repo = repo_of(positional.first().copied())?;
    let kind = match positional.get(1).map(|s| s.as_str()) {
        Some("issues") => Kind::Issues,
        Some("pulls") => Kind::Pulls,
        Some(other) => return Err(format!("list reads issues or pulls, not {other}")),
        None => return Err("list reads issues or pulls: name one".into()),
    };
    if let Some(extra) = positional.get(2) {
        return Err(format!("unrecognized argument: {extra}"));
    }
    Ok(Asked::List {
        repo,
        kind,
        labels,
        state: state.unwrap_or(State::Open),
    })
}

/// Whether to ask for the page after one just read: it was full, it
/// brought something not seen before, Forgejo's total (when it said one)
/// is not reached, and the page cap is not reached. A server that ignores
/// paging and answers everything at once stops here after its first page.
pub fn more(
    page: usize,
    page_len: usize,
    new_items: usize,
    seen: usize,
    total: Option<usize>,
) -> bool {
    page < MAX_PAGES && page_len >= PAGE_LIMIT && new_items > 0 && total.is_none_or(|t| seen < t)
}

fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or("")
}

fn login(v: &Value) -> &str {
    let user = v.get("user").map(|u| text(u, "login")).unwrap_or("");
    if user.is_empty() {
        // A comment migrated from elsewhere keeps its author here.
        text(v, "original_author")
    } else {
        user
    }
}

fn label_names(issue: &Value) -> Vec<&str> {
    issue
        .get("labels")
        .and_then(Value::as_array)
        .map(|ls| ls.iter().map(|l| text(l, "name")).collect())
        .unwrap_or_default()
}

/// An item's state: Forgejo's open or closed, and merged for a merged pull.
fn state(issue: &Value) -> &str {
    let merged = issue
        .get("pull_request")
        .and_then(|p| p.get("merged"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if merged {
        "merged"
    } else {
        text(issue, "state")
    }
}

/// Whether an item carries every one of the labels.
pub fn carries(issue: &Value, labels: &[String]) -> bool {
    let names = label_names(issue);
    labels.iter().all(|l| names.contains(&l.as_str()))
}

/// The item's id, to tell a page's new items from ones seen before.
pub fn id(item: &Value) -> Option<u64> {
    item.get("id").and_then(Value::as_u64)
}

/// One line of a title or label, so nothing it holds breaks a line.
fn one_line(s: &str) -> String {
    s.split(['\r', '\n']).collect::<Vec<_>>().join(" ")
}

/// An issue as `issue` prints it, without its comments.
pub fn render_issue(issue: &Value) -> String {
    let number = issue.get("number").and_then(Value::as_u64).unwrap_or(0);
    let labels: Vec<String> = label_names(issue).into_iter().map(one_line).collect();
    let mut out = format!(
        "number: {number}\ntitle: {}\nstate: {}\nlabels: {}\nauthor: {}\ncreated: {}\n",
        one_line(text(issue, "title")),
        state(issue),
        labels.join(", "),
        login(issue),
        text(issue, "created_at"),
    );
    let body = text(issue, "body");
    out.push('\n');
    out.push_str(body);
    if !body.ends_with('\n') {
        out.push('\n');
    }
    out
}

/// Every comment, oldest first, as `issue --comments` prints them.
pub fn render_comments(comments: &[Value]) -> String {
    let n = comments.len();
    let mut out = format!("\ncomments: {n}\n");
    for (i, c) in comments.iter().enumerate() {
        out.push_str(&format!(
            "\n--- comment {} of {n} by {} at {}\n",
            i + 1,
            login(c),
            text(c, "created_at")
        ));
        let body = text(c, "body");
        out.push_str(body);
        if !body.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

/// One line of `list`: `#<number> <state> [<labels>] <title>`.
pub fn render_line(issue: &Value) -> String {
    let number = issue.get("number").and_then(Value::as_u64).unwrap_or(0);
    let labels: Vec<String> = label_names(issue).into_iter().map(one_line).collect();
    format!(
        "#{number} {} [{}] {}\n",
        state(issue),
        labels.join(","),
        one_line(text(issue, "title"))
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_issue() {
        assert_eq!(
            parse(&args(&["issue", "guild/tasks", "64", "--comments"])),
            Ok(Asked::Issue {
                repo: Repo::parse("guild/tasks").unwrap(),
                number: 64,
                comments: true
            })
        );
        assert!(parse(&args(&["issue", "guild/tasks"])).is_err());
        assert!(parse(&args(&["issue", "guild/tasks", "0"])).is_err());
        assert!(parse(&args(&["issue", "guild/tasks", "x"])).is_err());
        assert!(parse(&args(&["issue", "guild/tasks", "1", "--x"])).is_err());
        assert!(parse(&args(&["issue", "../etc", "1"])).is_err());
        assert!(parse(&args(&["issue", "a/b/c", "1"])).is_err());
    }

    #[test]
    fn parses_list() {
        assert_eq!(
            parse(&args(&[
                "list",
                "guild/tasks",
                "issues",
                "--label",
                "a",
                "--label=b",
                "--state",
                "all"
            ])),
            Ok(Asked::List {
                repo: Repo::parse("guild/tasks").unwrap(),
                kind: Kind::Issues,
                labels: vec!["a".into(), "b".into()],
                state: State::All
            })
        );
        let Ok(Asked::List { state, kind, .. }) = parse(&args(&["list", "g/f", "pulls"])) else {
            panic!()
        };
        assert_eq!((state, kind), (State::Open, Kind::Pulls));
        assert!(parse(&args(&["list", "g/f", "commits"])).is_err());
        assert!(parse(&args(&["list", "g/f", "issues", "--state", "merged"])).is_err());
        assert!(parse(&args(&["list", "g/f", "issues", "--label"])).is_err());
        assert!(parse(&args(&["nope"])).is_err());
        assert_eq!(parse(&args(&["list", "--help"])), Ok(Asked::Help));
    }

    #[test]
    fn pages_until_short_seen_or_total() {
        assert!(more(1, PAGE_LIMIT, PAGE_LIMIT, 50, Some(120)));
        assert!(!more(3, 20, 20, 120, Some(120)));
        assert!(!more(1, 254, 254, 254, Some(254)));
        assert!(!more(2, 254, 0, 254, None));
        assert!(more(1, PAGE_LIMIT, PAGE_LIMIT, 50, None));
        assert!(!more(MAX_PAGES, PAGE_LIMIT, PAGE_LIMIT, 50, None));
    }

    #[test]
    fn renders_plain_text() {
        let issue = json!({"number": 7, "title": "a\nb", "state": "closed",
            "labels": [{"name": "x"}, {"name": "y"}], "user": {"login": "u"},
            "created_at": "t", "body": "hi", "pull_request": {"merged": true}});
        assert_eq!(render_line(&issue), "#7 merged [x,y] a b\n");
        assert!(render_issue(&issue)
            .starts_with("number: 7\ntitle: a b\nstate: merged\nlabels: x, y\nauthor: u\n"));
        assert!(carries(&issue, &["x".into(), "y".into()]));
        assert!(!carries(&issue, &["x".into(), "z".into()]));
    }
}
