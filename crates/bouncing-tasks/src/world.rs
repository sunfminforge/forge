//! What bouncing-tasks does to the world: reads L3, read-only, and prints.

use std::io::Write;
use std::process::ExitCode;

use rusqlite::types::Value as Sql;
use rusqlite::{Connection, OpenFlags};

use crate::{findings, render, repr, tasks, FlowRow, HELP};

const NAME: &str = "bouncing-tasks";

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

/// The rows of L3 the detector decides on: flow, and who worked each task.
type Rows = (Vec<FlowRow>, Vec<(String, String)>);

fn read(l3: &str) -> rusqlite::Result<Rows> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_URI;
    let db = Connection::open_with_flags(format!("file:{l3}?mode=ro"), flags)?;
    let mut st = db.prepare("SELECT task, at, record FROM flow ORDER BY at, id")?;
    let flow = st
        .query_map([], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?, text(r.get(2)?)))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(st);
    let mut workers = Vec::new();
    for table in ["settlements", "steps"] {
        let mut st = db.prepare(&format!("SELECT task, role FROM {table}"))?;
        let rows = st.query_map([], |r| Ok((text(r.get(0)?), text(r.get(1)?))))?;
        for row in rows {
            workers.push(row?);
        }
    }
    Ok((flow, workers))
}

pub fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{HELP}");
        return ExitCode::SUCCESS;
    }
    if let Some(a) = args.first() {
        eprintln!("{NAME}: takes no arguments, not {a:?}: run it as `{NAME}`");
        return ExitCode::from(2);
    }
    let l3 = std::env::var("GUILD_L3").ok();
    let Some(path) = l3
        .as_deref()
        .filter(|p| !p.is_empty() && std::path::Path::new(p).exists())
    else {
        eprintln!("{NAME}: no L3 at {}: nothing read", repr(l3.as_deref()));
        return ExitCode::SUCCESS;
    };
    let (flow, workers) = match read(path) {
        Ok(rows) => rows,
        Err(e) => {
            eprintln!("{NAME}: cannot read L3 at {path}: {e}: nothing read");
            return ExitCode::SUCCESS;
        }
    };
    let out = render(&findings(&tasks(&flow, &workers)));
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
