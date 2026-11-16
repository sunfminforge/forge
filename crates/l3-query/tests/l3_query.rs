//! l3-query's binary run on a fixture store made in a temp dir of the
//! test's own, never the live ~/.guild/l3.sqlite. Every expected output
//! is worked by hand from the fixture below.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use rusqlite::Connection;

const BIN: &str = env!("CARGO_BIN_EXE_l3-query");

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A fresh directory of this test run's own.
fn scratch(name: &str) -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("l3-query")
        .join(format!("{name}-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A store of two tables: tasks (4 columns, one untyped, 2 rows) and
/// steps (3 columns, no rows), plus an index, which is no table.
fn fixture(name: &str) -> PathBuf {
    let db = scratch(name).join("l3.sqlite");
    let c = Connection::open(&db).unwrap();
    c.execute_batch(
        "CREATE TABLE tasks (id INTEGER PRIMARY KEY, title TEXT NOT NULL, score REAL, note);
         CREATE TABLE steps (task_id INTEGER, kind TEXT, at TEXT);
         CREATE INDEX steps_task ON steps (task_id);
         INSERT INTO tasks VALUES (1, 'first', 2.0, NULL);
         INSERT INTO tasks VALUES (2, 'two	words
here', 0.5, x'00ff');",
    )
    .unwrap();
    drop(c);
    db
}

fn run(db: &Path, args: &[&str]) -> Output {
    let o = Command::new(BIN)
        .arg("--db")
        .arg(db)
        .args(args)
        .env_remove("HOME")
        .output()
        .unwrap();
    println!(
        "--- stdout:\n{}--- stderr:\n{}--- exit: {:?}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr),
        o.status.code()
    );
    o
}

fn out(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}

fn err(o: &Output) -> String {
    String::from_utf8(o.stderr.clone()).unwrap()
}

#[test]
fn tables_lists_each_table_with_its_columns() {
    let db = fixture("tables");
    let o = run(&db, &["tables"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        out(&o),
        "steps: task_id INTEGER, kind TEXT, at TEXT\n\
         tasks: id INTEGER, title TEXT, score REAL, note (untyped)\n"
    );
}

#[test]
fn tables_of_an_empty_store_says_so() {
    let db = scratch("tables-empty").join("e.sqlite");
    Connection::open(&db)
        .unwrap()
        .execute_batch("CREATE TABLE t (a); DROP TABLE t;")
        .unwrap();
    let o = run(&db, &["tables"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(out(&o), "(no tables)\n");
}

#[test]
fn tables_of_a_missing_store_names_the_fix() {
    let db = scratch("tables-missing").join("absent.sqlite");
    let o = run(&db, &["tables"]);
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(out(&o), "");
    assert!(err(&o).contains("--db <path>"), "{}", err(&o));
    assert!(!db.exists(), "a missing store must not be created");
}

#[test]
fn select_prints_a_header_and_a_line_per_row() {
    let db = fixture("select");
    let o = run(
        &db,
        &[
            "sql",
            "SELECT id, title, score, note FROM tasks ORDER BY id",
        ],
    );
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        out(&o),
        "id\ttitle\tscore\tnote\n\
         1\tfirst\t2.0\tNULL\n\
         2\ttwo\\twords\\nhere\t0.5\tx'00ff'\n"
    );
}

#[test]
fn select_with_a_cte_and_comment_runs() {
    let db = fixture("select-with");
    let o = run(
        &db,
        &[
            "sql",
            "-- how many\nWITH n AS (SELECT count(*) AS c FROM tasks) SELECT c FROM n;",
        ],
    );
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(out(&o), "c\n2\n");
}

#[test]
fn select_with_no_rows_says_so() {
    let db = fixture("select-empty");
    let o = run(&db, &["sql", "SELECT task_id, kind FROM steps"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(out(&o), "task_id\tkind\n(no rows)\n");
}

#[test]
fn select_of_a_missing_table_is_refused_by_sqlite() {
    let db = fixture("select-missing");
    let o = run(&db, &["sql", "SELECT * FROM absent"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(out(&o), "");
    assert!(err(&o).contains("no such table: absent"), "{}", err(&o));
    assert!(err(&o).contains("run l3-query tables"), "{}", err(&o));
}

/// Each statement that would write, or is not one SELECT, with the words
/// its refusal must carry.
const WRITES: &[(&str, &str)] = &[
    (
        "INSERT INTO tasks VALUES (3, 'x', 1.0, NULL)",
        "a statement starting INSERT",
    ),
    (
        "UPDATE tasks SET title = 'y'",
        "a statement starting UPDATE",
    ),
    ("DELETE FROM tasks", "a statement starting DELETE"),
    ("DROP TABLE tasks", "a statement starting DROP"),
    ("CREATE TABLE more (a)", "a statement starting CREATE"),
    (
        "ATTACH DATABASE 'other.sqlite' AS o",
        "a statement starting ATTACH",
    ),
    ("PRAGMA user_version = 7", "a statement starting PRAGMA"),
    ("pragma query_only = 0", "a statement starting PRAGMA"),
    (
        "/* hidden */ delete from tasks",
        "a statement starting DELETE",
    ),
    (
        "WITH d AS (SELECT 1) DELETE FROM tasks",
        "the statement writes",
    ),
    ("SELECT 1; DROP TABLE tasks", "more than one statement"),
    ("SELECT 1; SELECT 2", "more than one statement"),
    ("   ", "the statement is empty"),
];

#[test]
fn refuse_every_write_and_stacked_statement_leaving_the_fixture_unchanged() {
    let db = fixture("refuse");
    let before = fs::read(&db).unwrap();
    for (stmt, why) in WRITES {
        let o = run(&db, &["sql", stmt]);
        assert_eq!(o.status.code(), Some(1), "{stmt}");
        assert_eq!(out(&o), "", "{stmt}");
        let e = err(&o);
        assert!(e.contains(why), "{stmt}: {e}");
        // Every refusal carries its fix after the reason.
        assert!(e.contains("; "), "{stmt}: {e}");
        assert_eq!(fs::read(&db).unwrap(), before, "{stmt} changed the store");
    }
    let dir = db.parent().unwrap();
    assert!(!dir.join("other.sqlite").exists(), "ATTACH made a file");
    let left: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    assert_eq!(
        left,
        vec!["l3.sqlite".to_string()],
        "only the store is there"
    );
}

#[test]
fn refuse_names_the_fix_for_a_write() {
    let db = fixture("refuse-fix");
    let o = run(&db, &["sql", "DELETE FROM tasks"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        err(&o),
        "l3-query: error: refused: a statement starting DELETE; only SELECT or WITH runs, and nothing is written; read with a SELECT; to see the tables and their columns run l3-query tables\n"
    );
}

#[test]
fn usage_errors_exit_2_with_nothing_on_stdout() {
    let db = fixture("usage");
    for args in [
        &[][..],
        &["sql"][..],
        &["sql", "SELECT", "1"][..],
        &["drop"][..],
    ] {
        let o = run(&db, args);
        assert_eq!(o.status.code(), Some(2), "{args:?}");
        assert_eq!(out(&o), "", "{args:?}");
        assert!(err(&o).starts_with("usage: l3-query"), "{args:?}");
    }
}

#[test]
fn help_names_both_commands_and_the_exit_codes() {
    let o = Command::new(BIN).arg("--help").output().unwrap();
    assert_eq!(o.status.code(), Some(0));
    let h = out(&o);
    assert!(h.contains("l3-query [--db <path>] tables"));
    assert!(h.contains("sql \"<stmt>\""));
    assert!(h.contains("Exits 1 on a refused statement"));
    assert!(h.contains("Exits 2,"));
    assert!(!h.contains('{'), "a placeholder left: {h}");
}

#[test]
fn default_store_is_under_home() {
    let home = scratch("home");
    fs::create_dir_all(home.join(".guild")).unwrap();
    let db = fixture("home-src");
    fs::copy(&db, home.join(".guild/l3.sqlite")).unwrap();
    let o = Command::new(BIN)
        .args(["sql", "SELECT count(*) AS n FROM tasks"])
        .env("HOME", &home)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", err(&o));
    assert_eq!(out(&o), "n\n2\n");
}
