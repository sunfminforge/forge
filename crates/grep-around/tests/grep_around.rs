//! grep-around's binary run on fixture trees, each written by the test
//! into a fresh directory under the system's temp dir (not under target/,
//! whose parent .gitignore would hide it; and a file meant to be ignored
//! could not be committed as a fixture). Each test runs the binary in the
//! tree and asserts its stdout and exit code; each prints what the binary
//! printed, seen with `-- --nocapture`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_grep-around");

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A fresh tree holding `files`, each a path and its content.
fn tree(files: &[(&str, &str)]) -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir()
        .join("grep-around-tests")
        .join(format!("{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    for (p, c) in files {
        let p = dir.join(p);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, c).unwrap();
    }
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// `n` lines, `line 1` to `line n`, with `hits` lines replaced by
/// `HIT <n>`.
fn numbered(n: usize, hits: &[usize]) -> String {
    (1..=n)
        .map(|k| {
            if hits.contains(&k) {
                format!("HIT {k}\n")
            } else {
                format!("line {k}\n")
            }
        })
        .collect()
}

fn run(dir: &Path, args: &[&str]) -> Output {
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

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

#[test]
fn multi_file_match_prints_each_file_under_its_header() {
    let dir = tree(&[
        ("src/a.rs", "use x;\nfn main() {\n}\n"),
        ("src/b.rs", "// none here\n"),
        ("tools/c.rs", "fn helper() {}\nfn main() {}\n"),
    ]);
    let o = run(&dir, &["fn main", "--context", "1"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        stdout(&o),
        "== src/a.rs\n 1: use x;\n>2: fn main() {\n 3: }\n\
         == tools/c.rs\n 1: fn helper() {}\n>2: fn main() {}\n"
    );
}

#[test]
fn overlapping_regions_merge_and_apart_ones_are_separated() {
    // Hits at 5 and 8 with context 2: 3..7 and 6..10 overlap, one region.
    // Hit at 20: 18..22, apart, after the separator.
    let dir = tree(&[("f.txt", &numbered(25, &[5, 8, 20]))]);
    let o = run(&dir, &["HIT", "f.txt", "-C", "2"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        stdout(&o),
        "== f.txt\n 3: line 3\n 4: line 4\n>5: HIT 5\n 6: line 6\n 7: line 7\n>8: HIT 8\n 9: line 9\n 10: line 10\n\
         --\n 18: line 18\n 19: line 19\n>20: HIT 20\n 21: line 21\n 22: line 22\n"
    );
}

#[test]
fn default_context_is_ten_lines() {
    let dir = tree(&[("f.txt", &numbered(40, &[20]))]);
    let o = run(&dir, &["HIT"]);
    assert_eq!(o.status.code(), Some(0));
    let out = stdout(&o);
    let numbered_lines: Vec<&str> = out.lines().skip(1).collect();
    assert_eq!(numbered_lines.len(), 21, "{out}");
    assert_eq!(numbered_lines[0], " 10: line 10");
    assert_eq!(numbered_lines[10], ">20: HIT 20");
    assert_eq!(numbered_lines[20], " 30: line 30");
}

#[test]
fn glob_keeps_only_the_files_it_matches_and_bang_skips() {
    let dir = tree(&[
        ("a.rs", "needle\n"),
        ("b.toml", "needle\n"),
        ("sub/c.rs", "needle\n"),
    ]);
    let o = run(&dir, &["needle", "--glob", "*.rs", "--context", "0"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(stdout(&o), "== a.rs\n>1: needle\n== sub/c.rs\n>1: needle\n");

    let o = run(
        &dir,
        &["needle", "-g", "!sub/**", "-g", "!*.toml", "-C", "0"],
    );
    assert_eq!(stdout(&o), "== a.rs\n>1: needle\n");
}

#[test]
fn gitignore_target_hidden_and_binary_files_are_skipped() {
    let dir = tree(&[
        (".gitignore", "ignored/\n*.log\n"),
        ("kept.rs", "needle\n"),
        ("ignored/x.rs", "needle\n"),
        ("run.log", "needle\n"),
        ("target/debug/y.rs", "needle\n"),
        ("crate/target/z.rs", "needle\n"),
        (".hidden/h.rs", "needle\n"),
        ("blob.bin", "needle\0binary\n"),
    ]);
    // Not a git repository: .gitignore holds all the same.
    assert!(!dir.join(".git").exists());
    let o = run(&dir, &["needle", "-C", "0"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(stdout(&o), "== kept.rs\n>1: needle\n");
}

#[test]
fn a_file_range_prints_those_lines_numbered() {
    let dir = tree(&[("src/f.rs", &numbered(10, &[]))]);
    let o = run(&dir, &["src/f.rs:3-5"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(stdout(&o), "== src/f.rs\n3: line 3\n4: line 4\n5: line 5\n");

    // Past the end stops at the last line.
    let o = run(&dir, &["src/f.rs:9-99"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(stdout(&o), "== src/f.rs\n9: line 9\n10: line 10\n");
}

#[test]
fn a_bad_range_exits_2_naming_its_fix() {
    let dir = tree(&[("f.rs", &numbered(10, &[]))]);
    for (arg, says) in [
        ("f.rs:11-12", "f.rs has 10 lines"),
        ("f.rs:5-3", "5 is after 3"),
        ("f.rs:0-3", "numbered from 1"),
        ("nope.rs:1-3", "cannot read nope.rs"),
    ] {
        let o = run(&dir, &[arg]);
        assert_eq!(o.status.code(), Some(2), "{arg}");
        assert!(o.stdout.is_empty());
        assert!(stderr(&o).contains(says), "{arg}");
        assert!(stderr(&o).contains("fix:"), "{arg}");
    }
}

#[test]
fn a_range_shaped_pattern_with_a_path_is_searched() {
    let dir = tree(&[("f.txt", "see x:1-2 here\n")]);
    let o = run(&dir, &["x:1-2", ".", "-C", "0"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(stdout(&o), "== f.txt\n>1: see x:1-2 here\n");
}

#[test]
fn no_match_exits_1_printing_nothing() {
    let dir = tree(&[("a.rs", "fn main() {}\n")]);
    let o = run(&dir, &["absent_word"]);
    assert_eq!(o.status.code(), Some(1));
    assert!(o.stdout.is_empty());
}

#[test]
fn a_bad_pattern_exits_2_naming_its_fix() {
    let dir = tree(&[("a.rs", "fn main() {}\n")]);
    let o = run(&dir, &["fn main("]);
    assert_eq!(o.status.code(), Some(2));
    assert!(o.stdout.is_empty());
    let e = stderr(&o);
    assert!(e.contains("not a valid regular expression"), "{e}");
    assert!(e.contains("--fixed"), "{e}");
}

#[test]
fn fixed_matches_the_pattern_as_written() {
    let dir = tree(&[("a.rs", "x\nfn main() {}\n")]);
    let o = run(&dir, &["--fixed", "fn main(", "-C", "0"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(stdout(&o), "== a.rs\n>2: fn main() {}\n");
}

#[test]
fn bad_paths_glob_and_pattern_are_named_at_once_and_exit_2() {
    let dir = tree(&[("a.rs", "x\n")]);
    let o = run(&dir, &["(", "a.rs", "missing", "gone", "--glob", "a{"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(o.stdout.is_empty());
    let e = stderr(&o);
    assert!(e.contains("not a valid regular expression"), "{e}");
    assert!(e.contains("path missing does not exist"), "{e}");
    assert!(e.contains("path gone does not exist"), "{e}");
    assert!(e.contains("--glob a{ is not a glob"), "{e}");
    assert_eq!(e.matches("fix:").count(), 4, "{e}");
}

#[test]
fn a_bad_argument_exits_2_with_the_usage() {
    let dir = tree(&[]);
    for args in [
        &["--bogus", "x"][..],
        &[][..],
        &["x", "--context", "many"][..],
    ] {
        let o = run(&dir, args);
        assert_eq!(o.status.code(), Some(2), "{args:?}");
        assert!(stderr(&o).starts_with("usage: grep-around"), "{args:?}");
    }
}

#[test]
fn help_and_version() {
    let dir = tree(&[]);
    let o = run(&dir, &["--help"]);
    assert_eq!(o.status.code(), Some(0));
    let h = stdout(&o);
    assert!(h.starts_with("usage: grep-around <pattern>"));
    assert!(h.contains("Exits 0 when a line matched"));
    assert!(h.contains("default 10"));
    assert!(!h.contains('{'), "every placeholder filled: {h}");
    let o = run(&dir, &["--version"]);
    assert_eq!(stdout(&o), "grep-around 0.1.0\n");
}
