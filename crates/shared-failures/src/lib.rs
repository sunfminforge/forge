//! shared-failures: a detector of the trainer crew. One failure, several roles.
//!
//! The same failure or send-back reason in the tasks of several roles is a
//! gap in the level above them (a shared skill, a broader role, or the
//! foreman's way of splitting), not a patch in each.
//!
//! Reasons it reads, each on a task whose `role/<role>` label names its role:
//! - a send-back: the reason the scheduler quotes in its `sent-back` comment;
//! - a failure Guild saw the cause of: `<!--guild:failed cause=<cause> ...-->`;
//! - a failure the steward put back in the queue, with its reason
//!   (`<!--guild:requeue-->`).
//!
//! Two reasons are the same when their words, once tokens that only name
//! this case (hashes, numbers, paths, quoted output) are dropped, overlap at
//! least [`SIMILAR`] (Jaccard); a cause of a failed mark is the same only as
//! the same cause.
//!
//! This crate's root decides, from the issues and comments [`world`] read;
//! [`world`] reads Forgejo and prints.

use std::collections::BTreeSet;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;

pub mod py;
pub mod world;

pub const ORG: &str = "guild";
pub const REPO: &str = "tasks";
/// The share of their words two reasons have in common to be the same.
pub const SIMILAR: f64 = 0.5;
/// Fewer words than this say too little to be the same as another.
pub const MIN_WORDS: usize = 4;
/// Items a page of a Forgejo list holds.
pub const PAGE: usize = 50;

const SENT_BACK: &str = "<!--guild:sent-back ";
const FAILED: &str = "<!--guild:failed ";
const REQUEUE: &str = "<!--guild:requeue-->";

const STOP: &str = "a an the and or of to in on at by for with from as is are was were be
    been it its this that these those not no but if then than so do does did
    i we you he she they them their our my me us which who what when where
    how there here all any each every one into out up down over under";

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap_or_else(|e| panic!("{pattern}: {e}"))
}

static CAUSE: LazyLock<Regex> = LazyLock::new(|| re(r"\bcause=(\S+?)(?:\s|-->)"));
static CODE: LazyLock<Regex> = LazyLock::new(|| re(r"`[^`]*`"));
static QUOTED: LazyLock<Regex> = LazyLock::new(|| re(r#"'[^']*'|"[^"]*""#));
static PATH: LazyLock<Regex> = LazyLock::new(|| re(r"\S*[/\\#]\S*"));
static HASH: LazyLock<Regex> = LazyLock::new(|| re(r"\b[0-9a-f]{7,40}\b"));
static DIGITS: LazyLock<Regex> = LazyLock::new(|| re(r"\d+"));
static WORD: LazyLock<Regex> = LazyLock::new(|| re(r"[a-z][a-z'-]+"));

/// The kind of a reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    SendBack,
    Failure,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::SendBack => "send-back",
            Kind::Failure => "failure",
        }
    }
}

/// A reason a comment says: its kind, its text, and whether it is the same
/// only as the same text (a failure's cause).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reason {
    pub kind: Kind,
    pub text: String,
    pub exact: bool,
}

/// The reasons a comment's body says, if any.
pub fn reasons(body: &str) -> Vec<Reason> {
    let mut out = Vec::new();
    if body.starts_with(SENT_BACK) {
        let quoted: Vec<&str> = py::splitlines(body)
            .into_iter()
            .filter_map(|l| l.strip_prefix('>'))
            .map(py::strip)
            .collect();
        let text = py::strip(&quoted.join(" ")).to_string();
        if !text.is_empty() {
            out.push(Reason {
                kind: Kind::SendBack,
                text,
                exact: false,
            });
        }
    } else if body.starts_with(FAILED) {
        if let Some(m) = CAUSE.captures(body) {
            out.push(Reason {
                kind: Kind::Failure,
                text: format!("cause={}", &m[1]),
                exact: true,
            });
        }
    } else if let Some(rest) = body.strip_prefix(REQUEUE) {
        let text = py::strip(rest);
        if !text.is_empty() {
            out.push(Reason {
                kind: Kind::Failure,
                text: text.to_string(),
                exact: false,
            });
        }
    }
    out
}

/// The words of a reason that say what it is, less those naming its case.
pub fn words(text: &str) -> BTreeSet<String> {
    let text = text.to_lowercase();
    let text = CODE.replace_all(&text, " "); // code: names this case
    let text = QUOTED.replace_all(&text, " "); // quoted output
    let text = PATH.replace_all(&text, " "); // paths, refs
    let text = HASH.replace_all(&text, " "); // hashes
    let text = DIGITS.replace_all(&text, " ");
    let stop: BTreeSet<&str> = STOP.split_whitespace().collect();
    WORD.find_iter(&text)
        .map(|m| m.as_str())
        .filter(|w| !stop.contains(w) && w.chars().count() > 2)
        .map(str::to_string)
        .collect()
}

/// A reason as seen on a task of a role.
#[derive(Clone, Debug)]
pub struct Seen {
    pub reason: Reason,
    pub role: String,
    pub task: i64,
    pub at: String,
    pub words: BTreeSet<String>,
}

/// Whether two reasons are the same.
pub fn same(a: &Seen, b: &Seen) -> bool {
    if a.reason.exact || b.reason.exact {
        return a.reason.exact && b.reason.exact && a.reason.text == b.reason.text;
    }
    let (wa, wb) = (&a.words, &b.words);
    if wa.len() < MIN_WORDS || wb.len() < MIN_WORDS {
        return false;
    }
    let shared = wa.intersection(wb).count();
    let all = wa.union(wb).count();
    shared as f64 / all as f64 >= SIMILAR
}

/// The groups of reasons that are the same, each in the order seen, the
/// groups in the order of their first.
pub fn clusters(seen: &[Seen]) -> Vec<Vec<&Seen>> {
    let mut parent: Vec<usize> = (0..seen.len()).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for i in 0..seen.len() {
        for j in i + 1..seen.len() {
            if seen[i].reason.kind == seen[j].reason.kind && same(&seen[i], &seen[j]) {
                let (ri, rj) = (find(&mut parent, i), find(&mut parent, j));
                parent[ri] = rj;
            }
        }
    }
    let mut groups: Vec<(usize, Vec<&Seen>)> = Vec::new();
    for (i, s) in seen.iter().enumerate() {
        let root = find(&mut parent, i);
        match groups.iter_mut().find(|(r, _)| *r == root) {
            Some((_, g)) => g.push(s),
            None => groups.push((root, vec![s])),
        }
    }
    groups.into_iter().map(|(_, g)| g).collect()
}

/// A text on one line, cut to `n` characters.
pub fn short(text: &str, n: usize) -> String {
    let text = py::squeeze(text);
    if text.chars().count() <= n {
        return text;
    }
    format!("{}…", py::rstrip(py::take(&text, n - 1)))
}

/// Python's truthiness of a JSON value.
fn truthy(v: Option<&Value>) -> bool {
    match v {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64() != Some(0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
    }
}

/// The role a task issue's `role/<role>` label names.
pub fn role_of(issue: &Value) -> Option<String> {
    let labels = issue.get("labels").and_then(Value::as_array)?;
    labels
        .iter()
        .filter_map(|l| l.get("name").and_then(Value::as_str))
        .find_map(|n| n.strip_prefix("role/"))
        .filter(|r| !r.is_empty())
        .map(str::to_string)
}

/// The number of the issue a comment is on.
pub fn issue_number(comment: &Value) -> Option<i64> {
    let url = comment
        .get("issue_url")
        .and_then(Value::as_str)
        .unwrap_or("");
    let tail = url.trim_end_matches('/').rsplit('/').next().unwrap_or("");
    if tail.is_empty() || !tail.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    tail.parse().ok()
}

/// A finding: the role, what was seen, and the evidence for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub role: String,
    pub saw: String,
    pub evidence: Vec<String>,
}

/// One finding per role whose tasks share a reason with another role's.
pub fn findings(issues: &[Value], comments: &[Value]) -> Vec<Finding> {
    let mut roles: Vec<(i64, String)> = Vec::new();
    for issue in issues {
        if truthy(issue.get("pull_request")) {
            continue;
        }
        if let (Some(role), Some(n)) = (role_of(issue), issue.get("number").and_then(Value::as_i64))
        {
            roles.retain(|(m, _)| *m != n);
            roles.push((n, role));
        }
    }
    let created = |c: &Value| {
        c.get("created_at")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let mut sorted: Vec<&Value> = comments.iter().collect();
    sorted.sort_by_key(|c| (created(c), c.get("id").and_then(Value::as_i64).unwrap_or(0)));
    let mut seen = Vec::new();
    for c in sorted {
        let Some(task) = issue_number(c) else {
            continue;
        };
        let Some((_, role)) = roles.iter().find(|(n, _)| *n == task) else {
            continue;
        };
        for reason in reasons(c.get("body").and_then(Value::as_str).unwrap_or("")) {
            let words = if reason.exact {
                BTreeSet::new()
            } else {
                words(&reason.text)
            };
            seen.push(Seen {
                reason,
                role: role.clone(),
                task,
                at: created(c),
                words,
            });
        }
    }
    let mut out = Vec::new();
    for group in clusters(&seen) {
        let group_roles: BTreeSet<&str> = group.iter().map(|s| s.role.as_str()).collect();
        if group_roles.len() < 2 {
            continue;
        }
        let names: Vec<&str> = group_roles.into_iter().collect();
        let first = group[0]; // earliest: stable as the group grows
        let saw = format!(
            "the same {} reason came back in the tasks of several roles ({}): {}",
            first.reason.kind.name(),
            names.join(", "),
            short(&first.reason.text, 160)
        );
        let evidence: Vec<String> = group
            .iter()
            .map(|s| {
                format!(
                    "{ORG}/{REPO}#{} ({}, {} at {}): {}",
                    s.task,
                    s.role,
                    s.reason.kind.name(),
                    s.at,
                    short(&s.reason.text, 240)
                )
            })
            .collect();
        for role in names {
            out.push(Finding {
                role: role.to_string(),
                saw: saw.clone(),
                evidence: evidence.clone(),
            });
        }
    }
    out
}

/// A string as JSON writes it, which TOML reads as a basic string.
fn quote(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_default()
}

/// Each finding as a `[[finding]]` table, blank lines between.
pub fn render(found: &[Finding]) -> String {
    found
        .iter()
        .map(|f| {
            let evidence: String = f
                .evidence
                .iter()
                .map(|e| format!("  {},\n", quote(e)))
                .collect();
            format!(
                "[[finding]]\nrole = {}\nsaw = {}\nevidence = [\n{evidence}]\n",
                quote(&f.role),
                quote(&f.saw)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// What `--help` answers.
pub const HELP: &str = concat!(
    "usage: shared-failures\n\n",
    "A detector of the trainer crew: finds one failure or send-back reason shared by the tasks of several roles, a gap in the level above them rather than a patch in each.\n\n",
    "Reads Forgejo only (GUILD_FORGEJO_URL, GUILD_FORGEJO_TOKEN): the task issues of guild/tasks and their comments, read-only. Calls no model. The reasons it reads, each on a task whose role/<role> label names its role: a send-back's reason, a failure Guild saw the cause of, a failure the steward put back in the queue. Two reasons are the same when their words, less the tokens that only name the case (hashes, numbers, paths, quoted output), overlap at least half; a failure's cause only as the same cause.\n\n",
    "Prints only [[finding]] tables (role, saw, evidence) on stdout, one per role whose tasks share a reason with another role's, all with the same saw, and exits 0. Forgejo it cannot read is said on stderr, and it finds nothing. It takes no arguments: any but --help is refused with exit 2.\n",
);
