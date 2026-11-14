//! bouncing-tasks: a detector of the trainer crew.
//!
//! A piece of work passed between narrow roles, task after task, before it
//! lands: the foreman cuts it so that each task lands on a role too narrow
//! for it, or the roles are one kind of work split too thin.
//!
//! A piece of work is a goal's tasks on one repo, in the order they were
//! first queued, up to and including the first of them that landed (then
//! the next piece starts). It bounces when its tasks change role at least
//! [`HANDOFFS`] times among at least two roles (a -> b -> a, or a -> b -> c).
//!
//! This crate's root decides: it takes the rows [`world`] read from L3 and
//! answers the findings, and touches nothing. [`world`] reads L3 and prints.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde_json::Value;

pub mod world;

/// Hand-offs a piece of work must make to bounce.
pub const HANDOFFS: usize = 2;
/// Roles whose rows say nothing of who worked a task: the warden reviews.
pub const NOT_WORKERS: [&str; 1] = ["warden"];

/// A goal as a flow record names it: a number, or whatever else it holds.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Goal {
    Number(i64),
    Other(String),
}

impl Goal {
    fn of(v: &Value) -> Goal {
        match v {
            Value::Number(n) if n.as_i64().is_some() => Goal::Number(n.as_i64().unwrap_or(0)),
            Value::String(s) => Goal::Other(s.clone()),
            other => Goal::Other(other.to_string()),
        }
    }
}

impl std::fmt::Display for Goal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Goal::Number(n) => write!(f, "{n}"),
            Goal::Other(s) => f.write_str(s),
        }
    }
}

/// A task as L3's flow tells it, and the role that worked it.
#[derive(Clone, Debug, PartialEq)]
pub struct Task {
    pub task: i64,
    /// When it was first seen in flow: the order of a piece's tasks.
    pub first: i64,
    pub goal: Option<Goal>,
    pub repo: Option<String>,
    pub landed: bool,
    pub role: Option<String>,
}

/// A row of `flow`: task, at, record, in the order of `at, id`.
pub type FlowRow = (i64, i64, String);

/// Python's truthiness of a JSON value.
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64() != Some(0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// The tasks of flow, each with its goal, repo and landing, from its rows
/// in order; then the role each was worked by, from `workers`: the
/// (task, role) rows of settlements and steps.
pub fn tasks(flow: &[FlowRow], workers: &[(String, String)]) -> BTreeMap<i64, Task> {
    let mut tasks: BTreeMap<i64, Task> = BTreeMap::new();
    for (task, at, record) in flow {
        let Ok(Value::Object(r)) = serde_json::from_str::<Value>(record) else {
            continue;
        };
        let t = tasks.entry(*task).or_insert(Task {
            task: *task,
            first: *at,
            goal: None,
            repo: None,
            landed: false,
            role: None,
        });
        if let Some(goal) = r.get("goal").filter(|g| !g.is_null()) {
            t.goal = Some(Goal::of(goal));
        }
        if let Some(repo) = r.get("repo").filter(|v| truthy(v)) {
            t.repo = Some(match repo {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            });
        }
        if r.get("phase").and_then(Value::as_str) == Some("landed") && !t.landed {
            // A record's own `at` when it has one, the row's otherwise; a
            // null `at` is no landing.
            t.landed = r.get("at").is_none_or(|a| !a.is_null());
        }
    }
    let mut votes: HashMap<i64, HashMap<&str, usize>> = HashMap::new();
    for (task, role) in workers {
        if NOT_WORKERS.contains(&role.as_str()) {
            continue;
        }
        let num = task.rsplit('#').next().unwrap_or("");
        if num.is_empty() || !num.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let Ok(num) = num.parse::<i64>() else {
            continue;
        };
        *votes.entry(num).or_default().entry(role).or_default() += 1;
    }
    for (num, t) in tasks.iter_mut() {
        t.role = votes.get(num).and_then(|c| {
            c.iter()
                .min_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)))
                .map(|(r, _)| r.to_string())
        });
    }
    tasks
}

/// A piece of work: its goal, repo, tasks in order, and whether it landed.
pub struct Piece<'a> {
    pub goal: &'a Goal,
    pub repo: &'a str,
    pub tasks: Vec<&'a Task>,
    pub landed: bool,
}

/// Each piece of work, by goal and repo, then in the order of its tasks.
pub fn pieces(tasks: &BTreeMap<i64, Task>) -> Vec<Piece<'_>> {
    let mut by: BTreeMap<(&Goal, &str), Vec<&Task>> = BTreeMap::new();
    for t in tasks.values() {
        let (Some(goal), Some(repo), Some(_)) = (&t.goal, &t.repo, &t.role) else {
            continue;
        };
        by.entry((goal, repo.as_str())).or_default().push(t);
    }
    let mut out = Vec::new();
    for ((goal, repo), mut ts) in by {
        ts.sort_by_key(|t| (t.first, t.task));
        let mut piece = Vec::new();
        for t in ts {
            piece.push(t);
            if t.landed {
                out.push(Piece {
                    goal,
                    repo,
                    tasks: std::mem::take(&mut piece),
                    landed: true,
                });
            }
        }
        if !piece.is_empty() {
            out.push(Piece {
                goal,
                repo,
                tasks: piece,
                landed: false,
            });
        }
    }
    out
}

/// A finding: the role, what was seen, and the evidence for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub role: String,
    pub saw: String,
    pub evidence: Vec<String>,
}

/// One finding per role and saw: each role a bouncing piece passed through.
pub fn findings(tasks: &BTreeMap<i64, Task>) -> Vec<Finding> {
    let mut out: Vec<Finding> = Vec::new();
    for piece in pieces(tasks) {
        let roles: Vec<&str> = piece
            .tasks
            .iter()
            .filter_map(|t| t.role.as_deref())
            .collect();
        let handoffs = roles.windows(2).filter(|w| w[0] != w[1]).count();
        let distinct: BTreeSet<&str> = roles.iter().copied().collect();
        if handoffs < HANDOFFS || distinct.len() < 2 {
            continue;
        }
        let names: Vec<&str> = distinct.iter().copied().collect();
        let saw = format!(
            "a piece of work passed between narrow roles ({}), task after task, before it landed",
            names.join(", ")
        );
        let path: Vec<String> = piece
            .tasks
            .iter()
            .map(|t| format!("#{} {}", t.task, t.role.as_deref().unwrap_or("")))
            .collect();
        let evidence = format!(
            "goal guild/tasks#{}, repo {}: {}; {} hand-offs, {}",
            piece.goal,
            piece.repo,
            path.join(" -> "),
            handoffs,
            if piece.landed {
                "then it landed"
            } else {
                "not landed yet"
            }
        );
        for role in names {
            // Pieces alike, with the same role and saw, join their evidence.
            match out.iter_mut().find(|f| f.role == role && f.saw == saw) {
                Some(f) => f.evidence.push(evidence.clone()),
                None => out.push(Finding {
                    role: role.to_string(),
                    saw: saw.clone(),
                    evidence: vec![evidence.clone()],
                }),
            }
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

/// A string as Python's `repr` writes it, for what stderr says.
pub fn repr(s: Option<&str>) -> String {
    let Some(s) = s else { return "None".into() };
    let q = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::from(q);
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == q => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push(q);
    out
}

/// What `--help` answers.
pub const HELP: &str = concat!(
    "usage: bouncing-tasks\n\n",
    "A detector of the trainer crew: finds a piece of work passed between narrow roles, task after task, before it lands: roles too narrow for the foreman's cut, or one kind of work split too thin.\n\n",
    "Reads L3 only (GUILD_L3), read-only: flow for each task's goal, repo and landing; settlements and steps for the role that worked it, the warden's reviews aside. Calls no model. A piece of work is a goal's tasks on one repo in the order they were queued, up to and including the first that landed; it bounces when its tasks change role at least twice among at least two roles.\n\n",
    "Prints only [[finding]] tables (role, saw, evidence) on stdout, one per role the work passed through, all with the same saw, and exits 0. With no L3 to read it says so on stderr and finds nothing. It takes no arguments: any but --help is refused with exit 2.\n",
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repr_quotes_as_repr_does() {
        assert_eq!(repr(None), "None");
        assert_eq!(repr(Some("/a/b")), "'/a/b'");
        assert_eq!(repr(Some("it's")), "\"it's\"");
    }

    #[test]
    fn the_most_votes_then_the_first_name_is_the_role() {
        let flow = vec![(1, 10, r#"{"goal":1,"repo":"r"}"#.to_string())];
        let workers = vec![
            ("guild/tasks#1".to_string(), "b".to_string()),
            ("guild/tasks#1".to_string(), "a".to_string()),
            ("guild/tasks#1".to_string(), "warden".to_string()),
            ("guild/tasks#1".to_string(), "warden".to_string()),
        ];
        assert_eq!(tasks(&flow, &workers)[&1].role.as_deref(), Some("a"));
    }
}
