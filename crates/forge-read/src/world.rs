//! What forge-read does to the world: reads its arguments and the
//! environment, GETs from Forgejo, and prints. The one module that builds a
//! Forgejo request; it only ever sends GET.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::process::ExitCode;
use std::time::Duration;

use serde_json::Value;

use crate::{
    carries, heads, id, more, newest_first, parse, render_comments, render_issue, render_line,
    render_pull_line, Asked, Kind, Repo, State, EXIT_FAIL, EXIT_USAGE, HELP, NAME, PAGE_LIMIT,
    USAGE,
};

/// Why a read failed: said on stderr with its fix, exit 1.
struct Fail(String);

/// Forgejo as the environment names it, signed in as the instance.
struct Forgejo {
    base: String,
    token: String,
    agent: ureq::Agent,
}

/// A query value, percent-encoded.
fn quote(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~' => {
                (b as char).to_string()
            }
            b => format!("%{b:02X}"),
        })
        .collect()
}

/// What went wrong reaching Forgejo, without the URL ureq puts first.
fn cause(t: &ureq::Transport) -> String {
    let mut out = t.kind().to_string();
    if let Some(m) = t.message() {
        out = format!("{out}: {m}");
    }
    if let Some(source) = std::error::Error::source(t) {
        out = format!("{out}: {source}");
    }
    out
}

impl Forgejo {
    fn from_env() -> Result<Forgejo, Fail> {
        let var = |k: &str| std::env::var(k).unwrap_or_default();
        let base = var("GUILD_FORGEJO_URL").trim_end_matches('/').to_string();
        let token = var("GUILD_FORGEJO_TOKEN");
        let mut missing = Vec::new();
        if base.is_empty() {
            missing.push("GUILD_FORGEJO_URL");
        }
        if token.is_empty() {
            missing.push("GUILD_FORGEJO_TOKEN");
        }
        if !missing.is_empty() {
            return Err(Fail(format!(
                "{} not set: run forge-read inside a Guild instance, which is given its Forgejo's URL and its own account's token in GUILD_FORGEJO_URL and GUILD_FORGEJO_TOKEN",
                missing.join(" and ")
            )));
        }
        Ok(Forgejo {
            base,
            token,
            agent: ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(30))
                .build(),
        })
    }

    /// The JSON at an API path, and Forgejo's X-Total-Count. `what` names
    /// what is read, for a 404.
    fn get(
        &self,
        path: &str,
        query: &[(&str, String)],
        what: &str,
    ) -> Result<(Value, Option<usize>), Fail> {
        let mut url = format!("{}/api/v1/{path}", self.base);
        if !query.is_empty() {
            let q: Vec<String> = query
                .iter()
                .map(|(k, v)| format!("{}={}", quote(k), quote(v)))
                .collect();
            url = format!("{url}?{}", q.join("&"));
        }
        let req = self
            .agent
            .get(&url)
            .set("Accept", "application/json")
            .set("Authorization", &format!("token {}", self.token));
        let resp = match req.call() {
            Ok(resp) => resp,
            Err(ureq::Error::Status(404, _)) => {
                return Err(Fail(format!(
                    "{what} not found: check the owner/repo and number, and that the account of GUILD_FORGEJO_TOKEN can read it"
                )))
            }
            Err(ureq::Error::Status(code @ (401 | 403), _)) => {
                return Err(Fail(format!(
                    "Forgejo refused the token in GUILD_FORGEJO_TOKEN ({code}) for {url}: run forge-read with the token the instance was given, not a token of your own"
                )))
            }
            Err(ureq::Error::Status(code, _)) => {
                return Err(Fail(format!(
                    "Forgejo answered {code} for {url}: run it again once Forgejo answers"
                )))
            }
            Err(ureq::Error::Transport(t)) => {
                return Err(Fail(format!(
                    "Forgejo could not be reached at {url}: {}: check GUILD_FORGEJO_URL, then run it again",
                    cause(&t)
                )))
            }
        };
        let total = resp
            .header("X-Total-Count")
            .and_then(|v| v.trim().parse().ok());
        let mut body = Vec::new();
        let read = resp
            .into_reader()
            .read_to_end(&mut body)
            .map_err(|e| e.to_string());
        match read.and_then(|_| serde_json::from_slice(&body).map_err(|e| e.to_string())) {
            Ok(v) => Ok((v, total)),
            Err(e) => Err(Fail(format!(
                "Forgejo's answer at {url} could not be read: {e}"
            ))),
        }
    }

    /// Every item of a paged list, each page read until `more` says stop;
    /// an item seen on an earlier page is not taken twice.
    fn all(&self, path: &str, query: &[(&str, String)], what: &str) -> Result<Vec<Value>, Fail> {
        let mut items = Vec::new();
        let mut seen = HashSet::new();
        let mut page = 1;
        loop {
            let mut q = query.to_vec();
            q.push(("page", page.to_string()));
            q.push(("limit", PAGE_LIMIT.to_string()));
            let (data, total) = self.get(path, &q, what)?;
            let Some(list) = data.as_array() else {
                return Err(Fail(format!("Forgejo gave {what} that is not a list")));
            };
            let before = items.len();
            for item in list {
                if id(item).is_none_or(|i| seen.insert(i)) {
                    items.push(item.clone());
                }
            }
            let new = items.len() - before;
            if !more(page, list.len(), new, items.len(), total) {
                return Ok(items);
            }
            page += 1;
        }
    }
}

fn issue(f: &Forgejo, repo: &Repo, number: u64, comments: bool) -> Result<String, Fail> {
    let what = format!("{repo}#{number}");
    let path = format!("repos/{}/{}/issues/{number}", repo.owner, repo.name);
    let (data, _) = f.get(&path, &[], &what)?;
    let mut out = render_issue(&data);
    if comments {
        let list = f.all(
            &format!("{path}/comments"),
            &[],
            &format!("{what}'s comments"),
        )?;
        out.push_str(&render_comments(&list));
    }
    Ok(out)
}

fn list(
    f: &Forgejo,
    repo: &Repo,
    kind: Kind,
    labels: &[String],
    state: State,
    head: Option<&str>,
) -> Result<String, Fail> {
    if kind == Kind::Pulls {
        return pulls(f, repo, labels, state, head);
    }
    let mut query = vec![
        ("state", state.query().to_string()),
        ("type", kind.query().to_string()),
    ];
    if !labels.is_empty() {
        query.push(("labels", labels.join(",")));
    }
    let path = format!("repos/{}/{}/issues", repo.owner, repo.name);
    let items = f.all(&path, &query, &format!("{repo}"))?;
    // Forgejo matches any of the labels; forge-read keeps those with each.
    Ok(items
        .iter()
        .filter(|i| carries(i, labels))
        .map(render_line)
        .collect())
}

/// Every pull of a repo from its pulls endpoint, which has each pull's
/// head and base. It takes labels only by id, so forge-read sends none and
/// keeps the pulls carrying each label, and the head it keeps likewise.
fn pulls(
    f: &Forgejo,
    repo: &Repo,
    labels: &[String],
    state: State,
    head: Option<&str>,
) -> Result<String, Fail> {
    let query = vec![("state", state.query().to_string())];
    let path = format!("repos/{}/{}/pulls", repo.owner, repo.name);
    let mut items = f.all(&path, &query, &format!("{repo}"))?;
    newest_first(&mut items);
    Ok(items
        .iter()
        .filter(|p| carries(p, labels) && heads(p, head))
        .map(render_pull_line)
        .collect())
}

pub fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let asked = match parse(&args) {
        Ok(Asked::Help) => {
            print!("{HELP}");
            return ExitCode::SUCCESS;
        }
        Ok(Asked::Version) => {
            println!("{NAME} {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Ok(asked) => asked,
        Err(e) => {
            eprintln!("{USAGE}\n{NAME}: error: {e}; see {NAME} --help");
            return ExitCode::from(EXIT_USAGE);
        }
    };
    let run = || -> Result<String, Fail> {
        let f = Forgejo::from_env()?;
        match &asked {
            Asked::Issue {
                repo,
                number,
                comments,
            } => issue(&f, repo, *number, *comments),
            Asked::List {
                repo,
                kind,
                labels,
                state,
                head,
            } => list(&f, repo, *kind, labels, *state, head.as_deref()),
            Asked::Help | Asked::Version => Ok(String::new()),
        }
    };
    let out = match run() {
        Ok(out) => out,
        Err(Fail(e)) => {
            eprintln!("{NAME}: {e}");
            return ExitCode::from(EXIT_FAIL);
        }
    };
    let mut stdout = std::io::stdout().lock();
    if stdout
        .write_all(out.as_bytes())
        .and_then(|()| stdout.flush())
        .is_err()
    {
        return ExitCode::from(EXIT_FAIL);
    }
    ExitCode::SUCCESS
}
