//! The lore repo, read through Forgejo's API only: the one module that
//! builds a Forgejo request. The same in each detector that reads lore.

use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::time::Duration;

use serde_json::Value;

/// Why lore could not be read.
#[derive(Debug)]
pub enum Unread {
    /// No Forgejo token was given (as under `guild detector try`): lore
    /// cannot be read, and nothing is seen.
    NoAccess,
    /// Forgejo refused or could not be reached: said on stderr, exit 1.
    Fail(String),
}

/// Every file of a tree: path, then size and blob sha.
pub type Files = BTreeMap<String, (i64, String)>;

pub struct Lore {
    pub base: String,
    pub token: String,
    pub repo: String,
    pub branch: String,
    agent: ureq::Agent,
    trees: HashMap<String, Files>,
}

/// `urllib.parse.quote_plus` of a query value.
fn quote(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~' => {
                (b as char).to_string()
            }
            b' ' => "+".into(),
            b => format!("%{b:02X}"),
        })
        .collect()
}

/// What went wrong reaching Forgejo, without the URL ureq puts first.
pub fn cause(t: &ureq::Transport) -> String {
    let mut out = t.kind().to_string();
    if let Some(m) = t.message() {
        out = format!("{out}: {m}");
    }
    if let Some(source) = std::error::Error::source(t) {
        out = format!("{out}: {source}");
    }
    out
}

impl Lore {
    /// Lore as the environment names it: GUILD_FORGEJO_URL (required),
    /// GUILD_FORGEJO_TOKEN, GUILD_LORE_REPO, GUILD_LORE_BRANCH.
    pub fn from_env() -> Result<Lore, String> {
        let var = |k: &str| std::env::var(k).unwrap_or_default();
        let base = var("GUILD_FORGEJO_URL").trim_end_matches('/').to_string();
        if base.is_empty() {
            return Err("GUILD_FORGEJO_URL is not set".into());
        }
        let or = |k: &str, d: &str| std::env::var(k).unwrap_or_else(|_| d.to_string());
        Ok(Lore {
            base,
            token: var("GUILD_FORGEJO_TOKEN"),
            repo: or("GUILD_LORE_REPO", "guild/lore"),
            branch: or("GUILD_LORE_BRANCH", "main"),
            // No proxy: a detector reaches nothing beyond the Hub.
            agent: ureq::AgentBuilder::new()
                .timeout(Duration::from_secs(30))
                .build(),
            trees: HashMap::new(),
        })
    }

    /// The JSON at `path` of the repo, and whether Forgejo says it has more.
    pub fn get(&self, path: &str, query: &[(&str, String)]) -> Result<(Value, bool), Unread> {
        let mut url = format!("{}/api/v1/repos/{}/{path}", self.base, self.repo);
        if !query.is_empty() {
            let q: Vec<String> = query
                .iter()
                .map(|(k, v)| format!("{}={}", quote(k), quote(v)))
                .collect();
            url = format!("{url}?{}", q.join("&"));
        }
        let mut req = self.agent.get(&url).set("Accept", "application/json");
        if !self.token.is_empty() {
            req = req.set("Authorization", &format!("token {}", self.token));
        }
        let resp = match req.call() {
            Ok(resp) => resp,
            Err(ureq::Error::Status(code, _)) => {
                if (code == 401 || code == 403) && self.token.is_empty() {
                    return Err(Unread::NoAccess);
                }
                return Err(Unread::Fail(format!("Forgejo answered {code} for {url}")));
            }
            Err(ureq::Error::Transport(t)) => {
                return Err(Unread::Fail(format!(
                    "Forgejo could not be read at {url}: {}",
                    cause(&t)
                )))
            }
        };
        let more = resp
            .header("X-HasMore")
            .is_some_and(|v| v.eq_ignore_ascii_case("true"));
        let mut body = Vec::new();
        let read = resp
            .into_reader()
            .read_to_end(&mut body)
            .map_err(|e| e.to_string());
        let value = read.and_then(|_| serde_json::from_slice(&body).map_err(|e| e.to_string()));
        match value {
            Ok(v) => Ok((v, more)),
            Err(e) => Err(Unread::Fail(format!(
                "Forgejo could not be read at {url}: {e}"
            ))),
        }
    }

    /// Every file at `git_ref`, each page of the tree read.
    pub fn tree(&mut self, git_ref: &str) -> Result<Files, Unread> {
        if let Some(files) = self.trees.get(git_ref) {
            return Ok(files.clone());
        }
        let mut files = Files::new();
        let mut page = 1;
        loop {
            let query = [
                ("recursive", "true".to_string()),
                ("per_page", "1000".to_string()),
                ("page", page.to_string()),
            ];
            let (data, _) = self.get(&format!("git/trees/{git_ref}"), &query)?;
            let entries = data
                .get("tree")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            for e in &entries {
                if e.get("type").and_then(Value::as_str) == Some("blob") {
                    let path = e
                        .get("path")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let size = e.get("size").and_then(Value::as_i64).unwrap_or(0);
                    let sha = e
                        .get("sha")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    files.insert(path, (size, sha));
                }
            }
            let truncated = data
                .get("truncated")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if !truncated || entries.is_empty() {
                break;
            }
            page += 1;
        }
        self.trees.insert(git_ref.to_string(), files.clone());
        Ok(files)
    }
}
