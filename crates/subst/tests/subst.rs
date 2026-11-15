//! subst's binary run in a temp dir of each test's own. Each test prints
//! what the binary printed, seen with `-- --nocapture`. Every expected
//! output is worked by hand.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_subst");

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A fresh directory holding `files` (path, content).
fn dir(files: &[(&str, &str)]) -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let d = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("subst")
        .join(format!("{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    for (p, c) in files {
        let p = d.join(p);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, c).unwrap();
    }
    d
}

/// subst run in `d` with `args`.
fn run(d: &Path, args: &[&str]) -> Output {
    let o = Command::new(BIN)
        .current_dir(d)
        .args(args)
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

fn read(d: &Path, p: &str) -> String {
    fs::read_to_string(d.join(p)).unwrap()
}

/// The names in `d`, sorted, temp files included.
fn names(d: &Path) -> Vec<String> {
    let mut v: Vec<String> = fs::read_dir(d)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[test]
fn several_files_renamed_in_one_call() {
    let d = dir(&[
        ("a.rs", "fn add(x)\n"),
        ("b.rs", "list.add(1)\nlist.adder(2)\n"),
    ]);
    let o = run(&d, &[r"\badd\(", "add_item(", "a.rs", "b.rs"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(read(&d, "a.rs"), "fn add_item(x)\n");
    assert_eq!(read(&d, "b.rs"), "list.add_item(1)\nlist.adder(2)\n");
    assert_eq!(
        out(&o),
        "== a.rs\n-- subst 1 (\\badd\\(): 1 match, line 1\n1: fn add_item(x)\n\n== b.rs\n-- subst 1 (\\badd\\(): 1 match, line 1\n1: list.add_item(1)\n"
    );
    assert_eq!(names(&d), vec!["a.rs", "b.rs"]);
}

#[test]
fn capture_group_in_the_replacement() {
    let d = dir(&[("lib.rs", "fn get_old() {}\nfn set_old() {}\n")]);
    let o = run(
        &d,
        &[r"fn (\w+)_old\(", "fn ${1}_new(", "-n", "2", "lib.rs"],
    );
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(read(&d, "lib.rs"), "fn get_new() {}\nfn set_new() {}\n");
    assert_eq!(
        out(&o),
        "== lib.rs\n-- subst 1 (fn (\\w+)_old\\(): 2 matches, lines 1-2\n1: fn get_new() {}\n2: fn set_new() {}\n"
    );
}

#[test]
fn multi_line_splice_between_two_anchors() {
    let d = dir(&[(
        "main.rs",
        "fn main() {\n    // BEGIN gen\n    one();\n    two();\n    // END gen\n    run();\n}\n",
    )]);
    let o = run(
        &d,
        &[
            r"(?s)    // BEGIN gen\n.*?    // END gen\n",
            "    generated();\n",
            "-n",
            "1",
            "main.rs",
        ],
    );
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        read(&d, "main.rs"),
        "fn main() {\n    generated();\n    run();\n}\n"
    );
    assert!(
        out(&o).ends_with("1 match, line 2\n2:     generated();\n"),
        "{}",
        out(&o)
    );
    // An empty replacement deletes the span, printed as joined.
    let d = dir(&[("m.rs", "a\n// A\nx\ny\n// B\nb\n")]);
    let o = run(&d, &[r"(?s)// A\n.*?// B\n", "", "m.rs"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(read(&d, "m.rs"), "a\nb\n");
    assert!(
        out(&o).contains("deleted, joined at line 2\n1: a\n2: b\n"),
        "{}",
        out(&o)
    );
}

#[test]
fn several_substitutions_apply_in_order() {
    let d = dir(&[("c.toml", "name = \"x\"\nversion = \"0.1.0\"\n")]);
    let o = run(
        &d,
        &[
            "-e",
            "x",
            "y",
            "-e",
            r#"(?m)^version = "0\.1\.0"$"#,
            r#"version = "0.2.0""#,
            "-n",
            "1",
            "c.toml",
        ],
    );
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(read(&d, "c.toml"), "name = \"y\"\nversion = \"0.2.0\"\n");
    assert!(out(&o).contains("-- subst 2 ((?m)^version"), "{}", out(&o));
}

#[test]
fn zero_match_in_one_file_writes_no_file() {
    let d = dir(&[("a.rs", "fn add(x)\n"), ("b.rs", "nothing here\n")]);
    let o = run(&d, &[r"\badd\(", "add_item(", "a.rs", "b.rs"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(out(&o), "");
    assert_eq!(read(&d, "a.rs"), "fn add(x)\n");
    assert_eq!(
        err(&o),
        "subst: refused: 1 substitution not matched as required; no file written, every file is as it was\n\
         subst 1 ('\\badd\\('): 0 matches in b.rs, at least 1 required; drop b.rs from the call, or see what the regex finds with grep-around '\\badd\\(' b.rs and fix the regex\n"
    );
}

#[test]
fn count_mismatch_names_the_count_and_its_fix() {
    let d = dir(&[("a.rs", "x\ny\nx\nx\n"), ("b.rs", "x\n")]);
    let o = run(&d, &["x", "z", "-n", "2", "a.rs", "b.rs"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(read(&d, "b.rs"), "x\n");
    let e = err(&o);
    assert!(
        e.starts_with("subst: refused: 2 substitutions not matched"),
        "{e}"
    );
    assert!(e.contains("subst 1 ('x'): 3 matches in a.rs (lines 1, 3, 4), 2 required by -n 2; narrow the regex until it matches 2 times, or give -n 3 if every match should change\n"), "{e}");
    assert!(e.contains("subst 1 ('x'): 1 match in b.rs (line 1), 2 required by -n 2; widen the regex until it matches 2 times, or give -n 1 if those are all\n"), "{e}");
    // The fix it names, run: -n 3 on a.rs alone applies.
    let o = run(&d, &["x", "z", "-n", "3", "a.rs"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(read(&d, "a.rs"), "z\ny\nz\nz\n");
}

#[test]
fn dry_run_prints_the_same_and_writes_nothing() {
    let d = dir(&[("a.rs", "fn add(x)\n"), ("b.rs", "list.add(1)\n")]);
    let args = [r"\badd\(", "add_item(", "a.rs", "b.rs"];
    let mut dry = vec!["--dry-run"];
    dry.extend(args);
    let o = run(&d, &dry);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(read(&d, "a.rs"), "fn add(x)\n");
    assert_eq!(read(&d, "b.rs"), "list.add(1)\n");
    assert_eq!(err(&o), "subst: dry run: nothing written\n");
    let printed = out(&o);
    let o = run(&d, &args);
    assert_eq!(out(&o), printed);
    assert_eq!(read(&d, "a.rs"), "fn add_item(x)\n");
}

#[test]
fn running_twice_refuses_the_second_and_changes_nothing() {
    let d = dir(&[("a.rs", "fn add(x)\n")]);
    assert_eq!(
        run(&d, &[r"\badd\(", "add_item(", "a.rs"]).status.code(),
        Some(0)
    );
    let o = run(&d, &[r"\badd\(", "add_item(", "a.rs"]);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(read(&d, "a.rs"), "fn add_item(x)\n");
}

#[test]
fn bad_regex_missing_group_and_unreadable_file_are_named_at_once() {
    let d = dir(&[("a.rs", "a\n")]);
    let o = run(&d, &["-e", "(", "x", "-e", "(a)", "$1b", "a.rs", "gone.rs"]);
    assert_eq!(o.status.code(), Some(2));
    let e = err(&o);
    assert!(
        e.contains("subst 1 ('('): the regex does not compile"),
        "{e}"
    );
    assert!(e.contains("write ${1}b to follow group 1 with 'b'"), "{e}");
    assert!(e.contains("cannot read gone.rs"), "{e}");
    // The fix it names, run.
    let o = run(&d, &["(a)", "${1}b", "a.rs"]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(read(&d, "a.rs"), "ab\n");
}

#[test]
fn usage_errors_exit_2_with_the_fix() {
    let d = dir(&[("f", "x\n")]);
    let o = run(&d, &["x", "y", "f", "f"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).contains("f is named twice; name each file once"));
    let o = run(&d, &["x", "y"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).contains("name the files"));
}

#[cfg(unix)]
#[test]
fn a_file_it_cannot_write_leaves_every_file_as_it_was() {
    use std::os::unix::fs::PermissionsExt;
    let d = dir(&[("a.rs", "add\n"), ("ro/b.rs", "add\n")]);
    fs::set_permissions(d.join("ro"), fs::Permissions::from_mode(0o555)).unwrap();
    let o = run(&d, &["add", "sum", "a.rs", "ro/b.rs"]);
    fs::set_permissions(d.join("ro"), fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).contains("cannot write ro/b.rs"), "{}", err(&o));
    assert_eq!(read(&d, "a.rs"), "add\n");
    assert_eq!(read(&d, "ro/b.rs"), "add\n");
    assert_eq!(names(&d), vec!["a.rs", "ro"]);
}

#[test]
fn help_and_version() {
    let d = dir(&[]);
    let o = run(&d, &["--help"]);
    assert_eq!(o.status.code(), Some(0));
    assert!(out(&o).starts_with("usage: subst"));
    assert!(out(&o).contains("apply-edits"));
    let o = run(&d, &["-V"]);
    assert_eq!(out(&o), format!("subst {}\n", env!("CARGO_PKG_VERSION")));
}
