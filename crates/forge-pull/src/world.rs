//! What forge-pull does to the world: reads its arguments, the environment
//! and the body file, asks Forgejo, and prints. The one module that builds
//! a Forgejo request; its only write is `open`'s POST of one pull.

use std::collections::HashSet;
use std::io::{Read, Write};
use std::process::ExitCode;
use std::time::Duration;

use serde_json::Value;

use crate::{
    closes, error_message, more, open_request, parse, render_found, render_opened, review_state,
    Asked, Open, Repo, Task, EXIT_FAIL, EXIT_USAGE, HELP, NAME, PAGE_LIMIT, USAGE,
};

/// Why a run failed: said on stderr with its fix, exit 1.
struct Fail(String);

/// Forgejo as the environment names it, signed in as the instance.
struct Forgejo {
    base: String,
    token: String,
    agent: ureq::Agent,
}

/// A query value or path segment, percent-encoded; `keep` names the
/// characters left as they are.
fn quote(s: &str, keep: &[u8]) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~' => {
                (b as char).to_string()
            }
            b if keep.contains(&b) => (b as char).to_string(),
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

/// Forgejo's answer: its status and JSON, and its X-Total-Count.
struct Answer {
    status: u16,
    json: Value,
    total: Option<usize>,
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
                "{} not set: run forge-pull inside a Guild instance, which is given its Forgejo's URL and its own account's token in GUILD_FORGEJO_URL and GUILD_FORGEJO_TOKEN",
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

    /// One call to an API path. A status in `ok` is an answer; any other
    /// fails with Forgejo's status and message.
    fn call(
        &self,
        method: &str,
        path: &str,
        query: &[(&str, String)],
        body: Option<&Value>,
        ok: &[u16],
    ) -> Result<Answer, Fail> {
        let mut url = format!("{}/api/v1/{path}", self.base);
        if !query.is_empty() {
            let q: Vec<String> = query
                .iter()
                .map(|(k, v)| format!("{}={}", quote(k, &[]), quote(v, &[])))
                .collect();
            url = format!("{url}?{}", q.join("&"));
        }
        let shown = format!("{method} /api/v1/{path}");
        let req = self
            .agent
            .request(method, &url)
            .set("Accept", "application/json")
            .set("Authorization", &format!("token {}", self.token));
        let sent = match body {
            Some(b) => req
                .set("Content-Type", "application/json")
                .send_string(&b.to_string()),
            None => req.call(),
        };
        let resp = match sent {
            Ok(resp) => resp,
            Err(ureq::Error::Status(code, resp)) if ok.contains(&code) => resp,
            Err(ureq::Error::Status(code, resp)) => {
                let message = error_message(&resp.into_string().unwrap_or_default());
                let fix = match code {
                    401 | 403 => "run forge-pull with the token the instance was given, not a token of your own; if Forgejo names a scope, the instance's account lacks it: ask the foreman",
                    404 => "check the owner/repo (and the branch or number), and that the account of GUILD_FORGEJO_TOKEN can see it",
                    409 | 422 => "fix what Forgejo's message names (a branch not pushed, a head equal to base, a pull already open), then run it again",
                    _ => "run it again once Forgejo answers",
                };
                return Err(Fail(format!(
                    "Forgejo answered {code} to {shown}: {message}: {fix}"
                )));
            }
            Err(ureq::Error::Transport(t)) => {
                return Err(Fail(format!(
                    "Forgejo could not be reached for {shown}: {}: check GUILD_FORGEJO_URL, then run it again",
                    cause(&t)
                )))
            }
        };
        let status = resp.status();
        let total = resp
            .header("X-Total-Count")
            .and_then(|v| v.trim().parse().ok());
        let mut raw = Vec::new();
        let read = resp
            .into_reader()
            .read_to_end(&mut raw)
            .map_err(|e| e.to_string());
        let json = if (200..300).contains(&status) {
            match read.and_then(|_| serde_json::from_slice(&raw).map_err(|e| e.to_string())) {
                Ok(v) => v,
                Err(e) => {
                    return Err(Fail(format!(
                        "Forgejo's answer to {shown} could not be read: {e}: run it again once Forgejo answers"
                    )))
                }
            }
        } else {
            Value::Null
        };
        Ok(Answer {
            status,
            json,
            total,
        })
    }

    /// Every item of a paged list, each page read until `more` says stop;
    /// an item seen on an earlier page is not taken twice.
    fn all(&self, path: &str, query: &[(&str, String)]) -> Result<Vec<Value>, Fail> {
        let mut items = Vec::new();
        let mut seen = HashSet::new();
        let mut page = 1;
        loop {
            let mut q = query.to_vec();
            q.push(("page", page.to_string()));
            q.push(("limit", PAGE_LIMIT.to_string()));
            let answer = self.call("GET", path, &q, None, &[])?;
            let Some(list) = answer.json.as_array() else {
                return Err(Fail(format!(
                    "Forgejo answered GET /api/v1/{path} with something not a list: run it again once Forgejo answers"
                )));
            };
            let before = items.len();
            for item in list {
                let id = item.get("id").and_then(Value::as_u64);
                if id.is_none_or(|i| seen.insert(i)) {
                    items.push(item.clone());
                }
            }
            let new = items.len() - before;
            if !more(page, list.len(), new, items.len(), answer.total) {
                return Ok(items);
            }
            page += 1;
        }
    }
}

fn repo_path(repo: &Repo) -> String {
    format!(
        "repos/{}/{}",
        quote(&repo.owner, &[]),
        quote(&repo.name, &[])
    )
}

fn open(f: &Forgejo, o: &Open) -> Result<String, Fail> {
    let body = std::fs::read_to_string(&o.body_file).map_err(|e| {
        Fail(format!(
            "the body file {} could not be read: {e}: write the pull's body to that file, then run it again",
            o.body_file
        ))
    })?;
    let path = repo_path(&o.repo);
    // A pull already open from head into base is the answer; nothing is sent.
    let existing = f.call(
        "GET",
        &format!(
            "{path}/pulls/{}/{}",
            quote(&o.base, b"/"),
            quote(&o.head, b"/:")
        ),
        &[],
        None,
        &[404],
    )?;
    if existing.status == 200 && existing.json.get("state").and_then(Value::as_str) == Some("open")
    {
        eprintln!(
            "{NAME}: a pull from {} into {} is already open; nothing was sent",
            o.head, o.base
        );
        return Ok(render_opened(&o.repo, &existing.json));
    }
    let request = open_request(o, &body);
    if o.dry_run {
        return Ok(format!(
            "would send POST /api/v1/{path}/pulls {}\n",
            serde_json::to_string(&request).unwrap_or_default()
        ));
    }
    let made = f.call("POST", &format!("{path}/pulls"), &[], Some(&request), &[])?;
    Ok(render_opened(&o.repo, &made.json))
}

fn find(f: &Forgejo, task: &Task) -> Result<String, Fail> {
    let owner = &task.repo.owner;
    let repos = f.all(&format!("users/{}/repos", quote(owner, &[])), &[])?;
    let mut found: Vec<(Repo, Value)> = Vec::new();
    for r in &repos {
        if r.get("has_pull_requests").and_then(Value::as_bool) == Some(false) {
            continue;
        }
        let name = r.get("name").and_then(Value::as_str).unwrap_or("");
        let Some(repo) = Repo::parse(&format!("{owner}/{name}")) else {
            continue;
        };
        let pulls = f.all(
            &format!("{}/pulls", repo_path(&repo)),
            &[("state", "all".into())],
        )?;
        for p in pulls {
            let body = p.get("body").and_then(Value::as_str).unwrap_or("");
            if closes(body, task) {
                found.push((repo.clone(), p));
            }
        }
    }
    if found.is_empty() {
        return Err(Fail(format!(
            "none: no pull of {owner}'s {} repos says Closes {task}: open one with forge-pull open, its body holding the line Closes {task}",
            repos.len()
        )));
    }
    let number = |p: &Value| p.get("number").and_then(Value::as_u64).unwrap_or(0);
    found.sort_by(|(ra, pa), (rb, pb)| ra.cmp(rb).then(number(pa).cmp(&number(pb))));
    let mut out = String::new();
    for (repo, p) in &found {
        let reviews = f.all(
            &format!("{}/pulls/{}/reviews", repo_path(repo), number(p)),
            &[],
        )?;
        out.push_str(&render_found(repo, p, review_state(&reviews)));
    }
    Ok(out)
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
            Asked::Open(o) => open(&f, o),
            Asked::Find(t) => find(&f, t),
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
