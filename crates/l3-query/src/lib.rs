//! l3-query: what it decides, without touching the world. The arguments
//! it takes, whether a statement may run, what each refusal says with its
//! fix, and how tables, a header and rows print. `world` opens the store
//! read-only, prepares and runs, and prints what this decides.

pub mod world;

pub const NAME: &str = "l3-query";

/// Exit code of a refused statement: one that writes, more than one, one
/// that is not SELECT or WITH, or one SQLite cannot prepare or run.
pub const EXIT_REFUSED: u8 = 1;

/// Exit code of a usage error or a store that cannot be opened.
pub const EXIT_USAGE: u8 = 2;

/// The store read when no --db is given, under $HOME.
pub const DEFAULT_DB: &str = ".guild/l3.sqlite";

pub const USAGE: &str =
    "usage: l3-query [--db <path>] tables\n       l3-query [--db <path>] sql \"<SELECT>\"";

/// What `--help` answers, before its exit codes are put in by `help`.
const HELP: &str = concat!(
    "usage: l3-query [--db <path>] tables\n",
    "       l3-query [--db <path>] sql \"<SELECT>\"\n",
    "       l3-query --version\n\n",
    "Reads Guild's L3 store (~/.guild/l3.sqlite) in one call, read-only, instead of tries of sqlite3. Run 'l3-query tables' first to see what there is to select, then 'l3-query sql \"SELECT …\"'. The store is opened read-only with query_only on: l3-query never writes it.\n\n",
    "commands:\n",
    "  tables          one line per table, sorted by name, '<table>: <column> <TYPE>, <column> <TYPE>, …', the columns in their order; a column declared without a type prints '<column> (untyped)'. SQLite's own sqlite_* tables are left out. A store with no table prints '(no tables)'.\n",
    "  sql \"<stmt>\"    runs one statement, starting SELECT or WITH, quoted as one argument. Prints a header line of its column names, then one line per row, the values tab-separated. A statement with no rows prints the header then '(no rows)'.\n\n",
    "How a value prints: NULL as NULL; an integer or text as it is; a real with its decimal point (2.0, 0.5, 1e300); a blob as x'<hex>'. In a column name or text, a backslash prints \\\\, a tab \\t, a newline \\n and a carriage return \\r, so every row stays one line.\n\n",
    "Refused, with exit {EXIT_REFUSED}, the reason and its fix on stderr and nothing run: an empty statement; one that does not start SELECT or WITH (INSERT, UPDATE, DELETE, REPLACE, DROP, CREATE, ALTER, ATTACH, DETACH, PRAGMA, VACUUM, EXPLAIN…), comments before it skipped; a WITH that writes (WITH … DELETE); more than one statement ('SELECT 1; SELECT 2'); and one SQLite cannot prepare or run (no such table, a syntax error), its message given.\n\n",
    "options:\n",
    "  --db <path>     the store to read, in place of ~/.guild/l3.sqlite; it must exist\n",
    "  -h, --help      print this help\n",
    "  -V, --version   print l3-query's version\n\n",
    "Exits 0 with the answer on stdout. Exits {EXIT_REFUSED} on a refused statement. Exits {EXIT_USAGE}, with the usage on stderr and nothing on stdout, on no command or one it does not take, a missing --db value, a sql without its statement, or a store it cannot open.\n",
);

/// What `--help` answers, its exit codes as the constants say.
pub fn help() -> String {
    HELP.replace("{EXIT_REFUSED}", &EXIT_REFUSED.to_string())
        .replace("{EXIT_USAGE}", &EXIT_USAGE.to_string())
}

/// What the arguments ask. `db` is the --db given, if any.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked {
    Help,
    Version,
    Tables { db: Option<String> },
    Sql { db: Option<String>, stmt: String },
}

/// The arguments, after the program's name. --db may come before or
/// after the command.
pub fn parse(args: &[String]) -> Result<Asked, String> {
    let mut db = None;
    let mut words: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        match a {
            "-h" | "--help" => return Ok(Asked::Help),
            "-V" | "--version" => return Ok(Asked::Version),
            "--db" => {
                let Some(p) = args.get(i + 1) else {
                    return Err("--db needs a path: --db <path>".into());
                };
                db = Some(p.clone());
                i += 1;
            }
            _ if a.starts_with("--db=") => db = Some(a["--db=".len()..].to_string()),
            // A statement is the one word after `sql`, whatever it starts with.
            _ if a.starts_with('-') && words.first() != Some(&"sql") => {
                return Err(format!("no such option: {a}"))
            }
            _ => words.push(a),
        }
        i += 1;
    }
    match words.as_slice() {
        [] => Err("name a command: tables or sql \"<SELECT>\"".into()),
        ["tables"] => Ok(Asked::Tables { db }),
        ["tables", ..] => Err("tables takes no argument".into()),
        ["sql"] => Err("sql needs its statement: sql \"SELECT …\"".into()),
        ["sql", stmt] => Ok(Asked::Sql {
            db,
            stmt: (*stmt).to_string(),
        }),
        ["sql", ..] => {
            Err("sql takes one statement: quote it as one argument, sql \"SELECT …\"".into())
        }
        [other, ..] => Err(format!(
            "no such command: {other}; the commands are tables and sql"
        )),
    }
}

/// Why a statement is not run. Each says its reason and its fix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Nothing but whitespace and comments.
    Empty,
    /// Starts with a keyword other than SELECT or WITH.
    NotSelect(String),
    /// SQLite says the statement writes (a WITH … DELETE, say).
    Writes,
    /// More than one statement.
    Stacked,
    /// SQLite could not prepare or run it.
    Sqlite(String),
}

impl Refusal {
    /// The line stderr prints: the reason, then its fix.
    pub fn line(&self) -> String {
        let (why, fix) = match self {
            Refusal::Empty => (
                "the statement is empty".to_string(),
                "give one SELECT, sql \"SELECT …\"".to_string(),
            ),
            Refusal::NotSelect(k) => (
                format!(
                    "refused: a statement starting {k}; only SELECT or WITH runs, and nothing is written"
                ),
                "read with a SELECT; to see the tables and their columns run l3-query tables"
                    .to_string(),
            ),
            Refusal::Writes => (
                "refused: the statement writes; only reading runs".to_string(),
                "drop the INSERT, UPDATE or DELETE from it and select what you need".to_string(),
            ),
            Refusal::Stacked => (
                "refused: more than one statement; one runs per call".to_string(),
                "run each SELECT in its own l3-query sql call, without ';' between".to_string(),
            ),
            Refusal::Sqlite(m) => (
                format!("refused by SQLite: {m}"),
                "fix the statement; run l3-query tables for the tables and their columns"
                    .to_string(),
            ),
        };
        format!("{NAME}: error: {why}; {fix}")
    }
}

/// The statement's first keyword, upper-cased, after whitespace and
/// `--` or `/* */` comments; None when nothing else is there.
pub fn first_keyword(sql: &str) -> Option<String> {
    let mut rest = sql;
    loop {
        rest = rest.trim_start();
        if let Some(r) = rest.strip_prefix("--") {
            rest = r.find('\n').map_or("", |n| &r[n + 1..]);
        } else if let Some(r) = rest.strip_prefix("/*") {
            rest = r.find("*/").map_or("", |n| &r[n + 2..]);
        } else {
            break;
        }
    }
    let word: String = rest
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if word.is_empty() {
        // Something not a keyword, a `(` or `;` say: name what it is.
        rest.chars().next().map(|c| c.to_string())
    } else {
        Some(word.to_ascii_uppercase())
    }
}

/// Whether the statement may be prepared: it starts SELECT or WITH.
/// Writing, stacking and SQLite's own errors are known only once it is
/// prepared, and are `world`'s to report with the variants above.
pub fn check_start(sql: &str) -> Result<(), Refusal> {
    match first_keyword(sql) {
        None => Err(Refusal::Empty),
        Some(k) if k == "SELECT" || k == "WITH" => Ok(()),
        Some(k) if k == ";" => Err(Refusal::Empty),
        Some(k) => Err(Refusal::NotSelect(k)),
    }
}

/// One value of a row, as SQLite gives it.
#[derive(Clone, Debug, PartialEq)]
pub enum Cell {
    Null,
    Integer(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

/// Text with backslash, tab, newline and carriage return escaped, so it
/// stays on its line and in its column.
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out
}

/// How one value prints.
pub fn cell(c: &Cell) -> String {
    match c {
        Cell::Null => "NULL".into(),
        Cell::Integer(n) => n.to_string(),
        Cell::Real(r) => format!("{r:?}"),
        Cell::Text(t) => escape(t),
        Cell::Blob(b) => {
            let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
            format!("x'{hex}'")
        }
    }
}

/// The header line of column names, every one tab-separated.
pub fn header(names: &[String]) -> String {
    let names: Vec<String> = names.iter().map(|n| escape(n)).collect();
    format!("{}\n", names.join("\t"))
}

/// One row's line.
pub fn row(cells: &[Cell]) -> String {
    let cells: Vec<String> = cells.iter().map(cell).collect();
    format!("{}\n", cells.join("\t"))
}

/// What follows the header when no row came.
pub const NO_ROWS: &str = "(no rows)\n";

/// One table: its name and its columns, each a name and its declared
/// type (empty when declared without one), in their order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Table {
    pub name: String,
    pub columns: Vec<(String, String)>,
}

/// What `tables` prints: one line per table, sorted by name.
pub fn tables(mut ts: Vec<Table>) -> String {
    if ts.is_empty() {
        return "(no tables)\n".into();
    }
    ts.sort_by(|a, b| a.name.cmp(&b.name));
    let mut out = String::new();
    for t in ts {
        let cols: Vec<String> = t
            .columns
            .iter()
            .map(|(n, ty)| {
                if ty.is_empty() {
                    format!("{} (untyped)", escape(n))
                } else {
                    format!("{} {}", escape(n), escape(ty))
                }
            })
            .collect();
        out.push_str(&format!("{}: {}\n", escape(&t.name), cols.join(", ")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parse_takes_db_before_or_after() {
        assert_eq!(
            parse(&args(&["--db", "x.sqlite", "tables"])),
            Ok(Asked::Tables {
                db: Some("x.sqlite".into())
            })
        );
        assert_eq!(
            parse(&args(&["sql", "SELECT 1", "--db=y"])),
            Ok(Asked::Sql {
                db: Some("y".into()),
                stmt: "SELECT 1".into()
            })
        );
        assert!(parse(&args(&["sql", "SELECT", "1"])).is_err());
        assert!(parse(&args(&["--db"])).is_err());
        assert!(parse(&args(&[])).is_err());
    }

    #[test]
    fn first_keyword_skips_comments() {
        assert_eq!(
            first_keyword("  -- a note\n/* more */ select 1"),
            Some("SELECT".into())
        );
        assert_eq!(first_keyword(" /* only */ "), None);
        assert_eq!(first_keyword("(SELECT 1)"), Some("(".into()));
        assert_eq!(
            check_start("pragma user_version = 3"),
            Err(Refusal::NotSelect("PRAGMA".into()))
        );
        assert_eq!(check_start(" ; "), Err(Refusal::Empty));
    }

    #[test]
    fn cells_print_on_one_line() {
        assert_eq!(cell(&Cell::Real(2.0)), "2.0");
        assert_eq!(cell(&Cell::Text("a\tb\nc\\".into())), "a\\tb\\nc\\\\");
        assert_eq!(cell(&Cell::Blob(vec![0, 255])), "x'00ff'");
        assert_eq!(cell(&Cell::Null), "NULL");
    }
}
