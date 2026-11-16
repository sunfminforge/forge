//! What l3-query does to the world: reads its arguments and $HOME, opens
//! the store read-only with query_only on, lists its tables or prepares
//! and runs one statement, and prints what lib decides. It never writes.

use std::io::{self, Write};
use std::process::ExitCode;

use rusqlite::types::ValueRef;
use rusqlite::{Connection, OpenFlags};

use crate::{
    check_start, header, help, parse, row, tables, Asked, Cell, Refusal, Table, DEFAULT_DB,
    EXIT_REFUSED, EXIT_USAGE, NAME, NO_ROWS, USAGE,
};

/// A failure to answer: a usage-like one (exit 2) or a refusal (exit 1).
enum Failed {
    Usage(String),
    Refused(Refusal),
}

/// The store, opened so that nothing can write it: the file read-only
/// (never created) and query_only on.
fn open(db: Option<String>) -> Result<Connection, Failed> {
    let path = match db {
        Some(p) => p,
        None => match std::env::var("HOME") {
            Ok(h) if !h.is_empty() => format!("{h}/{DEFAULT_DB}"),
            _ => {
                return Err(Failed::Usage(
                    "HOME is not set, so ~/.guild/l3.sqlite is unknown; name the store with --db <path>".into(),
                ))
            }
        },
    };
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let conn = Connection::open_with_flags(&path, flags).map_err(|e| {
        Failed::Usage(format!(
            "cannot open the store {path} read-only: {e}; check the path, or name another with --db <path>"
        ))
    })?;
    conn.pragma_update(None, "query_only", true).map_err(|e| {
        Failed::Usage(format!("cannot set query_only on {path}: {e}; check the path, or name another with --db <path>"))
    })?;
    Ok(conn)
}

fn sqlite(e: rusqlite::Error) -> Failed {
    match e {
        rusqlite::Error::MultipleStatement => Failed::Refused(Refusal::Stacked),
        e => Failed::Refused(Refusal::Sqlite(e.to_string())),
    }
}

/// Every table of the store with its columns.
fn list_tables(conn: &Connection) -> Result<String, Failed> {
    let mut names = conn
        .prepare("SELECT name FROM sqlite_schema WHERE type = 'table' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\'")
        .map_err(sqlite)?;
    let names: Vec<String> = names
        .query_map([], |r| r.get(0))
        .map_err(sqlite)?
        .collect::<Result<_, _>>()
        .map_err(sqlite)?;
    let mut cols = conn
        .prepare("SELECT name, type FROM pragma_table_info(?1) ORDER BY cid")
        .map_err(sqlite)?;
    let mut out = Vec::new();
    for name in names {
        let columns = cols
            .query_map([&name], |r| Ok((r.get(0)?, r.get(1)?)))
            .map_err(sqlite)?
            .collect::<Result<_, _>>()
            .map_err(sqlite)?;
        out.push(Table { name, columns });
    }
    Ok(tables(out))
}

fn to_cell(v: ValueRef<'_>) -> Cell {
    match v {
        ValueRef::Null => Cell::Null,
        ValueRef::Integer(n) => Cell::Integer(n),
        ValueRef::Real(r) => Cell::Real(r),
        ValueRef::Text(t) => Cell::Text(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => Cell::Blob(b.to_vec()),
    }
}

/// One statement's header and rows, run only once lib and SQLite both
/// say it starts SELECT or WITH, is one statement, and writes nothing.
fn run_sql(conn: &Connection, stmt: &str) -> Result<String, Failed> {
    check_start(stmt).map_err(Failed::Refused)?;
    let mut st = conn.prepare(stmt).map_err(sqlite)?;
    if !st.readonly() {
        return Err(Failed::Refused(Refusal::Writes));
    }
    let names: Vec<String> = st.column_names().map(|s| s.to_string()).collect();
    let width = names.len();
    let mut out = header(&names);
    let mut rows = st.query([]).map_err(sqlite)?;
    let mut any = false;
    while let Some(r) = rows.next().map_err(sqlite)? {
        let mut cells = Vec::with_capacity(width);
        for i in 0..width {
            cells.push(to_cell(r.get_ref(i).map_err(sqlite)?));
        }
        out.push_str(&row(&cells));
        any = true;
    }
    if !any {
        out.push_str(NO_ROWS);
    }
    Ok(out)
}

fn answer(asked: Asked) -> Result<String, Failed> {
    match asked {
        Asked::Help => Ok(help()),
        Asked::Version => Ok(format!("{NAME} {}\n", env!("CARGO_PKG_VERSION"))),
        Asked::Tables { db } => list_tables(&open(db)?),
        Asked::Sql { db, stmt } => run_sql(&open(db)?, &stmt),
    }
}

pub fn main() -> ExitCode {
    let mut args = Vec::new();
    for a in std::env::args_os().skip(1) {
        match a.into_string() {
            Ok(s) => args.push(s),
            Err(a) => {
                eprintln!(
                    "{USAGE}\n{NAME}: error: argument {} is not UTF-8; give it as UTF-8; see {NAME} --help",
                    a.to_string_lossy()
                );
                return ExitCode::from(EXIT_USAGE);
            }
        }
    }
    let asked = match parse(&args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{USAGE}\n{NAME}: error: {e}; see {NAME} --help");
            return ExitCode::from(EXIT_USAGE);
        }
    };
    match answer(asked) {
        Ok(text) => {
            let mut out = io::stdout().lock();
            if let Err(e) = out.write_all(text.as_bytes()).and_then(|_| out.flush()) {
                eprintln!("{NAME}: error: cannot write stdout: {e}");
                return ExitCode::from(EXIT_USAGE);
            }
            ExitCode::SUCCESS
        }
        Err(Failed::Usage(e)) => {
            eprintln!("{NAME}: error: {e}");
            ExitCode::from(EXIT_USAGE)
        }
        Err(Failed::Refused(r)) => {
            eprintln!("{}", r.line());
            ExitCode::from(EXIT_REFUSED)
        }
    }
}
