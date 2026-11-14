//! read-files' binary run on files written to a temp dir of the test's
//! own. Each test prints what the binary printed, seen with
//! `-- --nocapture`. Every expected output is worked by hand.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_read-files");

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A fresh directory of this test run's own.
fn scratch(name: &str) -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("read-files")
        .join(format!("{name}-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// read-files run in `dir`, so each argument is a name relative to it.
fn read_files(dir: &Path, args: &[&str]) -> Output {
    let o = Command::new(BIN)
        .args(args)
        .current_dir(dir)
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

/// a.txt, b.toml (no newline at its end) and c.rs of five lines.
fn three_files(name: &str) -> PathBuf {
    let d = scratch(name);
    fs::write(d.join("a.txt"), "alpha\nbeta\n").unwrap();
    fs::write(d.join("b.toml"), "x = 1").unwrap();
    fs::write(d.join("c.rs"), "one\ntwo\nthree\nfour\nfive\n").unwrap();
    d
}

#[test]
fn whole_files_each_print_under_their_header_with_line_numbers() {
    let d = three_files("whole");
    let o = read_files(&d, &["a.txt", "b.toml"]);
    assert_eq!(
        out(&o),
        "==> a.txt <==\n     1\talpha\n     2\tbeta\n\n==> b.toml <==\n     1\tx = 1\n"
    );
    assert_eq!(err(&o), "");
    assert_eq!(o.status.code(), Some(0));
}

#[test]
fn a_range_prints_only_its_lines_after_two_whole_files() {
    let d = three_files("range");
    let o = read_files(&d, &["a.txt", "b.toml", "c.rs:2-4"]);
    assert_eq!(
        out(&o),
        concat!(
            "==> a.txt <==\n     1\talpha\n     2\tbeta\n\n",
            "==> b.toml <==\n     1\tx = 1\n\n",
            "==> c.rs:2-4 <==\n     2\ttwo\n     3\tthree\n     4\tfour\n",
        )
    );
    assert_eq!(err(&o), "");
    assert_eq!(o.status.code(), Some(0));

    let o = read_files(&d, &["c.rs:5-5"]);
    assert_eq!(out(&o), "==> c.rs:5-5 <==\n     5\tfive\n");
    assert_eq!(o.status.code(), Some(0));
}

#[test]
fn a_missing_file_and_an_out_of_range_range_go_to_stderr_the_others_print_exit_1() {
    let d = three_files("missing");
    let o = read_files(&d, &["nope.rs", "a.txt", "c.rs:7-9", "b.toml"]);
    assert_eq!(
        out(&o),
        "==> a.txt <==\n     1\talpha\n     2\tbeta\n\n==> b.toml <==\n     1\tx = 1\n"
    );
    let e = err(&o);
    let lines: Vec<&str> = e.lines().collect();
    assert_eq!(lines.len(), 2, "{e}");
    assert!(
        lines[0].starts_with("read-files: nope.rs: cannot open:"),
        "{e}"
    );
    assert_eq!(
        lines[1],
        "read-files: c.rs:7-9: range starts past the end, it has 5 lines; read it whole as c.rs, or ask c.rs:5-5"
    );
    assert_eq!(o.status.code(), Some(1));
}

#[test]
fn a_range_ending_past_the_end_prints_what_is_there_and_says_the_rest_exit_1() {
    let d = three_files("short");
    let o = read_files(&d, &["c.rs:4-8", "a.txt"]);
    assert_eq!(
        out(&o),
        "==> c.rs:4-8 <==\n     4\tfour\n     5\tfive\n\n==> a.txt <==\n     1\talpha\n     2\tbeta\n"
    );
    assert_eq!(
        err(&o),
        "read-files: c.rs:4-8: lines 6-8 are past the end, it has 5 lines; ask c.rs:4-5\n"
    );
    assert_eq!(o.status.code(), Some(1));
    // The fix it names runs clean.
    let o = read_files(&d, &["c.rs:4-5"]);
    assert_eq!(o.status.code(), Some(0));
}

#[test]
fn a_range_it_does_not_take_names_the_one_it_does() {
    let d = three_files("bad-range");
    let o = read_files(&d, &["c.rs:0-2", "c.rs:4-2", "a.txt"]);
    assert_eq!(out(&o), "==> a.txt <==\n     1\talpha\n     2\tbeta\n");
    assert_eq!(
        err(&o),
        concat!(
            "read-files: c.rs:0-2: lines count from 1; write c.rs:1-2\n",
            "read-files: c.rs:4-2: start 4 is past end 2; write c.rs:2-4\n",
        )
    );
    assert_eq!(o.status.code(), Some(1));
    for fix in ["c.rs:1-2", "c.rs:2-4"] {
        assert_eq!(read_files(&d, &[fix]).status.code(), Some(0));
    }
}

#[test]
fn a_directory_is_said_on_stderr() {
    let d = three_files("dir");
    fs::create_dir(d.join("sub")).unwrap();
    let o = read_files(&d, &["sub", "b.toml"]);
    assert_eq!(out(&o), "==> b.toml <==\n     1\tx = 1\n");
    assert!(
        err(&o).starts_with("read-files: sub: is a directory;"),
        "{}",
        err(&o)
    );
    assert_eq!(o.status.code(), Some(1));
}

#[test]
fn a_path_with_a_colon_that_is_no_range_is_read_whole_and_dash_dash_ends_options() {
    let d = scratch("colon");
    fs::write(d.join("x:y"), "1\n").unwrap();
    fs::write(d.join("-n"), "dash\n").unwrap();
    let o = read_files(&d, &["x:y"]);
    assert_eq!(out(&o), "==> x:y <==\n     1\t1\n");
    let o = read_files(&d, &["-n"]);
    assert_eq!(out(&o), "");
    assert!(err(&o).contains("put -- before it"), "{}", err(&o));
    assert_eq!(o.status.code(), Some(2));
    let o = read_files(&d, &["--", "-n"]);
    assert_eq!(out(&o), "==> -n <==\n     1\tdash\n");
    assert_eq!(o.status.code(), Some(0));
}

#[test]
fn no_path_is_a_usage_error_and_help_says_the_exit_codes() {
    let d = scratch("usage");
    let o = read_files(&d, &[]);
    assert_eq!(out(&o), "");
    assert!(err(&o).starts_with("usage: read-files"), "{}", err(&o));
    assert_eq!(o.status.code(), Some(2));
    let o = read_files(&d, &["--help"]);
    assert!(out(&o).starts_with("usage: read-files <path>[:<start>-<end>]"));
    assert!(out(&o).contains("Exits 1 when any argument failed"));
    assert!(!out(&o).contains('{'), "a placeholder left in help");
    assert_eq!(o.status.code(), Some(0));
    let o = read_files(&d, &["--version"]);
    assert_eq!(
        out(&o),
        format!("read-files {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn it_never_changes_a_file_it_reads() {
    let d = three_files("read-only");
    let before = fs::metadata(d.join("c.rs")).unwrap();
    let o = read_files(&d, &["c.rs", "c.rs:1-2", "c.rs:4-9"]);
    assert_eq!(o.status.code(), Some(1));
    let after = fs::metadata(d.join("c.rs")).unwrap();
    assert_eq!(
        fs::read_to_string(d.join("c.rs")).unwrap(),
        "one\ntwo\nthree\nfour\nfive\n"
    );
    assert_eq!(before.modified().unwrap(), after.modified().unwrap());
    assert_eq!(before.len(), after.len());
}
