//! tree-grep's binary run on fixture trees, each written by the test into
//! a fresh directory under the system's temp dir (not under target/,
//! whose parent .gitignore would hide it; and a file meant to be ignored
//! could not be committed as a fixture). Each test runs the binary in the
//! tree and asserts its stdout and exit code; each prints what the binary
//! printed, seen with `-- --nocapture`. Expected outputs are worked by
//! hand from the fixture.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_tree-grep");

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A fresh tree holding `files`, each a path and its content; a path
/// ending in `/` is an empty directory.
fn tree(files: &[(&str, &str)]) -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let dir = std::env::temp_dir()
        .join("tree-grep-tests")
        .join(format!("{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    for (p, c) in files {
        let p = dir.join(p);
        if p.to_string_lossy().ends_with('/') {
            fs::create_dir_all(&p).unwrap();
            continue;
        }
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, c).unwrap();
    }
    dir
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

/// A crate shaped as forge's are, under `crates/<name>`.
fn a_crate(name: &str) -> Vec<(String, String)> {
    vec![
        (
            format!("crates/{name}/tool.toml"),
            format!("name = \"{name}\"\nversion = \"0.1.0\"\n"),
        ),
        (
            format!("crates/{name}/Cargo.toml"),
            format!("[package]\nname = \"{name}\"\n"),
        ),
        (
            format!("crates/{name}/src/lib.rs"),
            "pub fn x() {}\n".into(),
        ),
        (
            format!("crates/{name}/src/main.rs"),
            "fn main() {}\n".into(),
        ),
        (
            format!("crates/{name}/tests/t.rs"),
            "#[test]\nfn t() {}\n".into(),
        ),
    ]
}

fn owned(v: &[(String, String)]) -> Vec<(&str, &str)> {
    v.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect()
}

#[test]
fn tree_then_matches_numbered_with_directories_trailing_slash() {
    let files = a_crate("peek");
    let dir = tree(&owned(&files));
    let o = run(&dir, &["name = \"peek\"", "crates/peek"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        stdout(&o),
        "== tree crates/peek\n\
         Cargo.toml\n\
         src/\n  lib.rs\n  main.rs\n\
         tests/\n  t.rs\n\
         tool.toml\n\
         == crates/peek/Cargo.toml\n2: name = \"peek\"\n\
         == crates/peek/tool.toml\n1: name = \"peek\"\n"
    );
}

#[test]
fn several_directories_in_one_call_each_tree_then_its_matches() {
    let mut files = a_crate("alpha");
    files.extend(a_crate("beta"));
    let dir = tree(&owned(&files));
    let o = run(
        &dir,
        &[
            "^name",
            "crates/beta",
            "crates/alpha",
            "--glob",
            "tool.toml",
        ],
    );
    assert_eq!(o.status.code(), Some(0));
    let tree_of = "Cargo.toml\nsrc/\n  lib.rs\n  main.rs\ntests/\n  t.rs\ntool.toml\n";
    assert_eq!(
        stdout(&o),
        format!(
            "== tree crates/beta\n{tree_of}== crates/beta/tool.toml\n1: name = \"beta\"\n\
             == tree crates/alpha\n{tree_of}== crates/alpha/tool.toml\n1: name = \"alpha\"\n"
        )
    );
}

#[test]
fn depth_limits_the_tree_not_the_search() {
    let dir = tree(&[
        ("a/b/c/deep.txt", "needle\n"),
        ("a/top.txt", "hay\n"),
        ("empty/", ""),
    ]);
    // Default depth 2: a/, its b/ and top.txt; c/ is depth 3.
    let o = run(&dir, &["needle"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        stdout(&o),
        "== tree .\na/\n  b/\n  top.txt\nempty/\n== a/b/c/deep.txt\n1: needle\n"
    );

    let o = run(&dir, &["needle", ".", "--depth", "1"]);
    assert_eq!(
        stdout(&o),
        "== tree .\na/\nempty/\n== a/b/c/deep.txt\n1: needle\n"
    );

    let o = run(&dir, &["needle", "--depth=4"]);
    assert_eq!(
        stdout(&o),
        "== tree .\na/\n  b/\n    c/\n      deep.txt\n  top.txt\nempty/\n\
         == a/b/c/deep.txt\n1: needle\n"
    );
}

#[test]
fn gitignore_target_git_hidden_and_binary_skipped_in_tree_and_search() {
    let dir = tree(&[
        (".gitignore", "ignored/\n*.log\n"),
        ("kept.rs", "needle\n"),
        ("ignored/x.rs", "needle\n"),
        ("run.log", "needle\n"),
        ("target/debug/y.rs", "needle\n"),
        ("sub/target/z.rs", "needle\n"),
        ("sub/s.rs", "needle\n"),
        (".git/HEAD", "needle\n"),
        (".hidden/h.rs", "needle\n"),
        ("blob.bin", "needle\0binary\n"),
    ]);
    let o = run(&dir, &["needle"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        stdout(&o),
        "== tree .\nkept.rs\nsub/\n  s.rs\n== kept.rs\n1: needle\n== sub/s.rs\n1: needle\n"
    );
}

#[test]
fn context_prints_regions_as_grep_around_does() {
    let text: String = (1..=12)
        .map(|k| {
            if k == 3 || k == 5 || k == 11 {
                format!("HIT {k}\n")
            } else {
                format!("line {k}\n")
            }
        })
        .collect();
    let dir = tree(&[("f.txt", &text)]);
    // 3±1 = 2..4 and 5±1 = 4..6 overlap; 11±1 = 10..12 apart.
    let o = run(&dir, &["HIT", "--context", "1"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        stdout(&o),
        "== tree .\nf.txt\n== f.txt\n 2: line 2\n>3: HIT 3\n 4: line 4\n>5: HIT 5\n 6: line 6\n\
         --\n 10: line 10\n>11: HIT 11\n 12: line 12\n"
    );
    // Default context 0: the matching lines alone, unmarked.
    let o = run(&dir, &["HIT"]);
    assert_eq!(
        stdout(&o),
        "== tree .\nf.txt\n== f.txt\n3: HIT 3\n5: HIT 5\n11: HIT 11\n"
    );
}

#[test]
fn glob_narrows_the_files_searched_not_the_tree() {
    let dir = tree(&[
        ("a.rs", "needle\n"),
        ("b.toml", "needle\n"),
        ("sub/c.rs", "needle\n"),
    ]);
    let o = run(&dir, &["needle", "--glob", "*.rs"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        stdout(&o),
        "== tree .\na.rs\nb.toml\nsub/\n  c.rs\n== a.rs\n1: needle\n== sub/c.rs\n1: needle\n"
    );
    let o = run(&dir, &["needle", "-g", "!sub/**", "-g", "!*.toml"]);
    assert_eq!(
        stdout(&o),
        "== tree .\na.rs\nb.toml\nsub/\n  c.rs\n== a.rs\n1: needle\n"
    );
}

#[test]
fn fixed_takes_the_pattern_as_text() {
    let dir = tree(&[("a.rs", "x\nfn main() {}\n")]);
    let o = run(&dir, &["--fixed", "fn main("]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(stdout(&o), "== tree .\na.rs\n== a.rs\n2: fn main() {}\n");
}

#[test]
fn no_match_exits_1_with_the_tree_printed() {
    let dir = tree(&[("a.rs", "fn main() {}\n")]);
    let o = run(&dir, &["absent_word"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(stdout(&o), "== tree .\na.rs\n");
}

#[test]
fn a_missing_directory_errors_under_its_header_others_print_exit_2() {
    let dir = tree(&[("here/a.rs", "needle\n"), ("file.txt", "needle\n")]);
    let o = run(&dir, &["needle", "gone", "here", "file.txt"]);
    assert_eq!(o.status.code(), Some(2));
    let out = stdout(&o);
    assert!(
        out.starts_with("== tree gone\nerror: gone does not exist; fix: "),
        "{out}"
    );
    assert!(
        out.contains("== tree here\na.rs\n== here/a.rs\n1: needle\n== tree file.txt\nerror: file.txt is not a directory"),
        "{out}"
    );
    // Each error line names its fix.
    assert_eq!(out.matches("; fix: ").count(), 2, "{out}");
}

#[test]
fn a_bad_pattern_or_glob_exits_2_printing_nothing() {
    let dir = tree(&[("a.rs", "fn main() {}\n")]);
    let o = run(&dir, &["fn main(", "--glob", "a{"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(o.stdout.is_empty());
    let e = stderr(&o);
    assert!(e.starts_with("usage: tree-grep"), "{e}");
    assert!(e.contains("not a valid regular expression"), "{e}");
    assert!(e.contains("--fixed"), "{e}");
    assert!(e.contains("--glob a{ is not a glob"), "{e}");
    assert_eq!(e.matches("fix:").count(), 2, "{e}");
}

#[test]
fn a_bad_argument_exits_2_naming_every_fault() {
    let dir = tree(&[]);
    for (args, faults) in [
        (&["--bogus", "x"][..], 1),
        (&[][..], 1),
        (&["x", "--context", "many", "--depth", "-1"][..], 2),
        (&["x", "--depth"][..], 1),
    ] {
        let o = run(&dir, args);
        assert_eq!(o.status.code(), Some(2), "{args:?}");
        assert!(o.stdout.is_empty(), "{args:?}");
        let e = stderr(&o);
        assert!(e.starts_with("usage: tree-grep"), "{args:?}");
        assert_eq!(e.matches("fix:").count(), faults, "{args:?}: {e}");
    }
}

#[test]
fn help_and_version() {
    let dir = tree(&[]);
    let o = run(&dir, &["--help"]);
    assert_eq!(o.status.code(), Some(0));
    let h = stdout(&o);
    assert!(h.starts_with("usage: tree-grep <pattern>"), "{h}");
    assert!(h.contains("Exits 0 when a line matched"), "{h}");
    assert!(h.contains("default 2"), "{h}");
    assert!(h.contains("default 0"), "{h}");
    assert!(!h.contains('{'), "every placeholder filled: {h}");
    let o = run(&dir, &["--version"]);
    assert_eq!(stdout(&o), "tree-grep 0.1.0\n");
}
