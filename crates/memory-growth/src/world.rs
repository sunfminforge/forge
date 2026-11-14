//! What memory-growth does to the world: reads its arguments, lore through
//! Forgejo and L3, read-only, and prints.

use std::collections::BTreeMap;
use std::io::Write;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::types::Value as Sql;
use rusqlite::{Connection, OpenFlags};

use crate::lore::{Lore, Unread};
use crate::{
    chain, commit_of, findings, memory, render, settled, Bars, Commit, Sizes, DAY, MAX_COMMITS,
    NAME,
};

const USAGE: &str = "usage: memory-growth [--days N] [--bytes-per-task B] [--min-bytes M]";

/// What `--help` answers.
const HELP: &str = concat!(
    "usage: memory-growth [--days N] [--bytes-per-task B] [--min-bytes M]\n\n",
    "Finds roles whose memory (core.md and skills/) grew faster than the work they did: more bytes added over the window than --bytes-per-task for each task they settled.\n\n",
    "options:\n",
    "  -h, --help            show this help message and exit\n",
    "  --days N              the window, in days back from now (14)\n",
    "  --bytes-per-task B    growth a settled task may bring before it is too much (1024)\n",
    "  --min-bytes M         growth below this is never a finding (2048)\n\n",
    "Reads lore's main through Forgejo (GUILD_FORGEJO_URL, GUILD_FORGEJO_TOKEN; GUILD_LORE_REPO, default guild/lore) and the settlements of L3 (GUILD_L3). Calls no model. Growth is counted along main's first parents, from the window's opening or from when the role came into lore within it, so a new role's first draft is no growth. The foreman settles no task of its own: it is credited with every settled task that belongs to a goal.\n\n",
    "Prints only [[finding]] tables (role, saw, evidence) on stdout and exits 0. Without a token, lore is unread and it prints nothing. Exits 1 with the cause on stderr when GUILD_FORGEJO_URL is unset, L3 is missing or Forgejo refuses its token; exits 2 on an argument it does not take.\n",
);

/// What the arguments ask: the bars, or help.
enum Asked {
    Help,
    Run(Bars),
}

/// The arguments, as Python's argparse reads them: `--opt value`,
/// `--opt=value`, and any unambiguous prefix of an option.
fn parse(args: &[String]) -> Result<Asked, String> {
    const OPTIONS: [&str; 4] = ["--help", "--days", "--bytes-per-task", "--min-bytes"];
    let mut bars = Bars {
        days: 14.0,
        bytes_per_task: 1024,
        min_bytes: 2048,
    };
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        i += 1;
        if arg == "-h" {
            return Ok(Asked::Help);
        }
        let (name, inline) = match arg.split_once('=') {
            Some((n, v)) if n.starts_with("--") => (n, Some(v.to_string())),
            _ => (arg.as_str(), None),
        };
        let matches: Vec<&str> = if name.len() > 2 && name.starts_with("--") {
            match OPTIONS.iter().find(|o| **o == name) {
                Some(o) => vec![*o],
                None => OPTIONS
                    .iter()
                    .filter(|o| o.starts_with(name))
                    .copied()
                    .collect(),
            }
        } else {
            Vec::new()
        };
        let option = match matches.as_slice() {
            [one] => *one,
            [] => return Err(format!("unrecognized arguments: {arg}")),
            many => {
                return Err(format!(
                    "ambiguous option: {name} could match {}",
                    many.join(", ")
                ))
            }
        };
        if option == "--help" {
            return Ok(Asked::Help);
        }
        let value = match inline {
            Some(v) => v,
            None => {
                let next = args
                    .get(i)
                    .filter(|v| !v.starts_with('-') || v.parse::<f64>().is_ok());
                let Some(v) = next else {
                    return Err(format!("argument {option}: expected one argument"));
                };
                i += 1;
                v.clone()
            }
        };
        match option {
            "--days" => {
                bars.days = value
                    .trim()
                    .parse()
                    .map_err(|_| format!("argument --days: invalid float value: '{value}'"))?
            }
            "--bytes-per-task" => {
                bars.bytes_per_task = value.trim().parse().map_err(|_| {
                    format!("argument --bytes-per-task: invalid int value: '{value}'")
                })?
            }
            _ => {
                bars.min_bytes = value
                    .trim()
                    .parse()
                    .map_err(|_| format!("argument --min-bytes: invalid int value: '{value}'"))?
            }
        }
    }
    Ok(Asked::Run(bars))
}

/// L3, read-only; read as it stands when the sandbox lets nothing be
/// written beside it.
fn l3() -> Result<Connection, String> {
    let path = std::env::var("GUILD_L3").ok();
    let Some(path) = path
        .as_deref()
        .filter(|p| !p.is_empty() && std::path::Path::new(p).exists())
    else {
        return Err(format!("no L3 at GUILD_L3='{}'", path.unwrap_or_default()));
    };
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI;
    let ro = Connection::open_with_flags(format!("file:{path}?mode=ro"), flags).and_then(|db| {
        db.query_row("SELECT 1 FROM sqlite_master LIMIT 1", [], |_| Ok(()))
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(()),
                e => Err(e),
            })
            .map(|()| db)
    });
    match ro {
        Ok(db) => Ok(db),
        Err(_) => Connection::open_with_flags(format!("file:{path}?immutable=1"), flags)
            .map_err(|e| format!("cannot read L3 at {path}: {e}")),
    }
}

/// A cell of L3 as Python's `str` would write it.
fn text(v: Sql) -> String {
    match v {
        Sql::Null => "None".into(),
        Sql::Integer(i) => i.to_string(),
        Sql::Real(f) => f.to_string(),
        Sql::Text(s) => s,
        Sql::Blob(b) => String::from_utf8_lossy(&b).into_owned(),
    }
}

/// L3's settlements since a time (role, task), and its flow records.
type Rows = (Vec<(String, String)>, Vec<String>);

fn read_l3(db: &Connection, since: f64) -> rusqlite::Result<Rows> {
    let mut st = db.prepare(
        "SELECT role, task FROM settlements WHERE at > ? GROUP BY role, task ORDER BY MIN(at)",
    )?;
    let rows = st.query_map([since.trunc() as i64], |r| {
        Ok((text(r.get(0)?), text(r.get(1)?)))
    })?;
    let settlements = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut st = db.prepare("SELECT record FROM flow")?;
    let flow = st
        .query_map([], |r| Ok(text(r.get(0)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok((settlements, flow))
}

/// Lore's main as it landed, newest first, along first parents.
fn history(lore: &Lore) -> Result<Vec<Commit>, Unread> {
    let mut commits = BTreeMap::new();
    let mut head: Option<String> = None;
    let mut page = 1;
    while commits.len() < MAX_COMMITS {
        let query = [
            ("sha", lore.branch.clone()),
            ("limit", "50".to_string()),
            ("page", page.to_string()),
            ("stat", "false".to_string()),
            ("verification", "false".to_string()),
            ("files", "false".to_string()),
        ];
        let (data, more) = lore.get("commits", &query)?;
        let list = data.as_array().cloned().unwrap_or_default();
        if list.is_empty() {
            break;
        }
        for c in &list {
            let Some((commit, parents)) = commit_of(c) else {
                return Err(Unread::Fail(format!(
                    "Forgejo gave a commit it could not read: {c}"
                )));
            };
            head.get_or_insert_with(|| commit.sha.clone());
            commits.insert(commit.sha.clone(), (commit, parents));
        }
        if !more {
            break;
        }
        page += 1;
    }
    Ok(chain(head.as_deref(), &commits))
}

/// Each role's memory at a commit: path, then size.
fn sizes(lore: &mut Lore, sha: &str) -> Result<BTreeMap<String, Sizes>, Unread> {
    let files = lore.tree(sha)?;
    Ok(memory(&files)
        .into_iter()
        .map(|(role, files)| {
            (
                role,
                files.into_iter().map(|(p, (size, _))| (p, size)).collect(),
            )
        })
        .collect())
}

fn fail(message: &str) -> ExitCode {
    eprintln!("{NAME}: {message}");
    ExitCode::from(1)
}

pub fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let bars = match parse(&args) {
        Ok(Asked::Help) => {
            print!("{HELP}");
            return ExitCode::SUCCESS;
        }
        Ok(Asked::Run(bars)) => bars,
        Err(e) => {
            eprintln!("{USAGE}\n{NAME}: error: {e}");
            return ExitCode::from(2);
        }
    };
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0);
    let since = now - bars.days * DAY;
    let mut lore = match Lore::from_env() {
        Ok(lore) => lore,
        Err(e) => return fail(&e),
    };
    let db = match l3() {
        Ok(db) => db,
        Err(e) => return fail(&e),
    };
    let mut run = || -> Result<String, Unread> {
        let chain = history(&lore)?;
        if chain.is_empty() {
            return Ok(String::new());
        }
        let (settlements, flow) =
            read_l3(&db, since).map_err(|e| Unread::Fail(format!("cannot read L3: {e}")))?;
        let tasks = settled(&settlements, &flow);
        let before = chain.iter().find(|c| c.time <= since);
        let window = chain.iter().rev().filter(|c| c.time > since);
        let mut states = Vec::new();
        for c in before.into_iter().chain(window) {
            states.push((c.clone(), sizes(&mut lore, &c.sha)?));
        }
        Ok(render(&findings(
            &states,
            before.is_some(),
            &tasks,
            since,
            bars,
        )))
    };
    let out = match run() {
        Ok(out) => out,
        Err(Unread::NoAccess) => return ExitCode::SUCCESS, // no token given: lore unread, nothing seen
        Err(Unread::Fail(e)) => return fail(&e),
    };
    let mut stdout = std::io::stdout().lock();
    if stdout
        .write_all(out.as_bytes())
        .and_then(|()| stdout.flush())
        .is_err()
    {
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bars(args: &[&str]) -> Result<Bars, String> {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        match parse(&args)? {
            Asked::Run(b) => Ok(b),
            Asked::Help => Err("help".into()),
        }
    }

    #[test]
    fn reads_options_as_argparse_does() {
        let b = bars(&["--days", "25", "--bytes=500", "--min-bytes", "-1"]).unwrap();
        assert_eq!((b.days, b.bytes_per_task, b.min_bytes), (25.0, 500, -1));
        assert!(bars(&["--days"])
            .unwrap_err()
            .contains("expected one argument"));
        assert!(bars(&["--days", "x"])
            .unwrap_err()
            .contains("invalid float value"));
        assert!(bars(&["extra"])
            .unwrap_err()
            .contains("unrecognized arguments"));
        assert_eq!(bars(&["--he"]).unwrap_err(), "help");
    }
}
