//! apply-patch's binary run in a temp dir of each test's own, its patch on
//! stdin. Each test prints what the binary printed, seen with
//! `-- --nocapture`. Every expected output is worked by hand.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_apply-patch");

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A fresh directory holding `files` (path, content).
fn dir(files: &[(&str, &str)]) -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let d = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("apply-patch")
        .join(format!("{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    for (p, c) in files {
        fs::write(d.join(p), c).unwrap();
    }
    d
}

/// apply-patch run in `d` with `args`, `stdin` sent on its standard input.
fn run(d: &Path, args: &[&str], stdin: &str) -> Output {
    let mut c = Command::new(BIN)
        .current_dir(d)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
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

const OLD: &str = "fn main() {\n    let port = 8080;\n    serve(port);\n}\n";

const ALL_THREE: &str = "\
*** Begin Patch
*** Add File: src/new.rs
+pub fn hello() -> &'static str {
+    \"hello\"
+}
*** Update File: main.rs
<<<<<<< SEARCH
    let port = 8080;
=======
    let port = 9090;
    let host = \"::\";
>>>>>>> REPLACE
*** Delete File: gone.txt
*** End Patch
";

#[test]
fn add_update_and_delete_apply_in_one_call() {
    let d = dir(&[("main.rs", OLD), ("gone.txt", "a\nb\n")]);
    let o = run(&d, &[], ALL_THREE);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        out(&o),
        "== add src/new.rs: 3 lines\n1: pub fn hello() -> &'static str {\n2:     \"hello\"\n3: }\n\n\
         == update main.rs: 1 block\n== block 1: 1 match, lines 2-3\n2:     let port = 9090;\n3:     let host = \"::\";\n\n\
         == delete gone.txt: 2 lines removed\n"
    );
    assert_eq!(
        read(&d, "src/new.rs"),
        "pub fn hello() -> &'static str {\n    \"hello\"\n}\n"
    );
    assert_eq!(
        read(&d, "main.rs"),
        "fn main() {\n    let port = 9090;\n    let host = \"::\";\n    serve(port);\n}\n"
    );
    assert!(!d.join("gone.txt").exists());
    assert_eq!(names(&d), ["main.rs", "src"]);
}

#[test]
fn add_writes_a_new_file_making_its_directories() {
    let d = dir(&[]);
    let o = run(
        &d,
        &[],
        "*** Begin Patch\n*** Add File: a/b/c.txt\n+one\n+\n+three\n*** End Patch\n",
    );
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        out(&o),
        "== add a/b/c.txt: 3 lines\n1: one\n2: \n3: three\n"
    );
    assert_eq!(read(&d, "a/b/c.txt"), "one\n\nthree\n");
}

#[test]
fn update_applies_several_blocks_to_several_files() {
    let d = dir(&[("a.toml", "x = 1\ny = 2\nz = 3\n"), ("b.txt", "k\nk\n")]);
    let o = run(
        &d,
        &[],
        "*** Begin Patch\n\
         *** Update File: a.toml\n\
         <<<<<<< SEARCH\nx = 1\n=======\nx = 10\n>>>>>>> REPLACE\n\n\
         <<<<<<< SEARCH\nz = 3\n=======\n>>>>>>> REPLACE\n\
         *** Update File: b.txt\n\
         <<<<<<< SEARCH all\nk\n=======\nK\n>>>>>>> REPLACE\n\
         *** End Patch\n",
    );
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        out(&o),
        "== update a.toml: 2 blocks\n== block 1: 1 match, line 1\n1: x = 10\n\
         == block 2: 1 match, deleted, joined at line 3\n1: x = 10\n2: y = 2\n\n\
         == update b.txt: 1 block\n== block 1: 2 matches, lines 1-2\n1: K\n2: K\n"
    );
    assert_eq!(read(&d, "a.toml"), "x = 10\ny = 2\n");
    assert_eq!(read(&d, "b.txt"), "K\nK\n");
}

#[test]
fn delete_removes_the_file() {
    let d = dir(&[("gone.txt", "bye"), ("kept.txt", "hi\n")]);
    let o = run(
        &d,
        &[],
        "*** Begin Patch\n*** Delete File: gone.txt\n*** End Patch\n",
    );
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(out(&o), "== delete gone.txt: 1 line removed\n");
    assert_eq!(names(&d), ["kept.txt"]);
}

/// A patch of three changes, one of which fails as `bad` says; nothing
/// is written and the failing change is named.
fn all_or_none(files: &[(&str, &str)], bad: &str, says: &[&str]) {
    let d = dir(files);
    let before: Vec<(String, String)> = names(&d)
        .into_iter()
        .map(|n| {
            let c = read(&d, &n);
            (n, c)
        })
        .collect();
    let patch = format!(
        "*** Begin Patch\n*** Add File: src/new.rs\n+x\n*** Update File: main.rs\n<<<<<<< SEARCH\n    let port = 8080;\n=======\n    let port = 1;\n>>>>>>> REPLACE\n{bad}*** End Patch\n"
    );
    let o = run(&d, &[], &patch);
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(out(&o), "");
    let e = err(&o);
    assert!(e.contains("nothing written, every file is as it was"));
    for s in says {
        assert!(e.contains(s), "stderr lacks {s:?}");
    }
    let after: Vec<(String, String)> = names(&d)
        .into_iter()
        .map(|n| {
            let c = read(&d, &n);
            (n, c)
        })
        .collect();
    assert_eq!(after, before, "a file was written");
}

#[test]
fn all_or_none_a_search_matching_nowhere_writes_nothing() {
    all_or_none(
        &[("main.rs", OLD), ("other.txt", "a\nb\n")],
        "*** Update File: other.txt\n<<<<<<< SEARCH\nzzz\n=======\nq\n>>>>>>> REPLACE\n",
        &[
            "1 of 3 changes cannot be made",
            "update other.txt (patch line 10): 1 of 1 block did not match exactly once",
            "block 1 ('zzz'): not found, 0 matches in other.txt",
            "peek other.txt",
        ],
    );
}

#[test]
fn all_or_none_a_search_matching_twice_writes_nothing() {
    all_or_none(
        &[("main.rs", OLD), ("other.txt", "a\nb\na\n")],
        "*** Update File: other.txt\n<<<<<<< SEARCH\na\n=======\nq\n>>>>>>> REPLACE\n",
        &[
            "block 1 ('a'): 2 matches in other.txt, at lines 1, 3",
            "'<<<<<<< SEARCH all'",
        ],
    );
}

#[test]
fn all_or_none_an_add_over_an_existing_file_writes_nothing() {
    all_or_none(
        &[("main.rs", OLD), ("there.txt", "keep\n")],
        "*** Add File: there.txt\n+new\n",
        &["add there.txt (patch line 10): there.txt exists already; to change it use '*** Update File: there.txt'"],
    );
}

#[test]
fn all_or_none_an_update_of_a_missing_file_writes_nothing() {
    all_or_none(
        &[("main.rs", OLD)],
        "*** Update File: lib/nope.rs\n<<<<<<< SEARCH\na\n=======\nb\n>>>>>>> REPLACE\n",
        &["update lib/nope.rs (patch line 10): no such file: lib/nope.rs; check the path with peek lib, or create it with '*** Add File: lib/nope.rs'"],
    );
}

#[test]
fn all_or_none_a_delete_of_a_missing_file_writes_nothing() {
    all_or_none(
        &[("main.rs", OLD)],
        "*** Delete File: nope.txt\n",
        &["delete nope.txt (patch line 10): no such file: nope.txt; check the path with peek ., or drop its '*** Delete File: ' section"],
    );
}

#[test]
fn all_or_none_names_every_failing_change_at_once() {
    let d = dir(&[("main.rs", OLD)]);
    let o = run(
        &d,
        &[],
        "*** Begin Patch\n*** Add File: main.rs\n+x\n*** Delete File: a\n*** Delete File: b\n*** End Patch\n",
    );
    assert_eq!(o.status.code(), Some(1));
    let e = err(&o);
    assert!(e.starts_with("apply-patch: refused: 3 of 3 changes cannot be made"));
    assert!(e.contains("add main.rs (patch line 2)"));
    assert!(e.contains("delete a (patch line 4)"));
    assert!(e.contains("delete b (patch line 5)"));
    assert_eq!(read(&d, "main.rs"), OLD);
}

#[cfg(unix)]
#[test]
fn all_or_none_a_write_that_fails_puts_back_what_was_done() {
    use std::os::unix::fs::PermissionsExt;
    let d = dir(&[("main.rs", OLD)]);
    fs::create_dir(d.join("locked")).unwrap();
    fs::write(d.join("locked/f.txt"), "a\n").unwrap();
    fs::set_permissions(d.join("locked"), fs::Permissions::from_mode(0o555)).unwrap();
    let o = run(
        &d,
        &[],
        "*** Begin Patch\n*** Add File: made/new.txt\n+x\n*** Update File: main.rs\n<<<<<<< SEARCH\n    let port = 8080;\n=======\n    let port = 1;\n>>>>>>> REPLACE\n*** Update File: locked/f.txt\n<<<<<<< SEARCH\na\n=======\nb\n>>>>>>> REPLACE\n*** End Patch\n",
    );
    fs::set_permissions(d.join("locked"), fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).contains("cannot write locked/f.txt"));
    assert!(err(&o)
        .contains("nothing written, every file is as it was; make it and its directory writable"));
    assert_eq!(read(&d, "main.rs"), OLD);
    assert_eq!(read(&d, "locked/f.txt"), "a\n");
    assert_eq!(names(&d), ["locked", "main.rs"]);
    assert_eq!(names(&d.join("locked")), ["f.txt"]);
}

#[test]
fn a_malformed_patch_exits_2_and_writes_nothing() {
    let d = dir(&[("main.rs", OLD)]);
    let o = run(
        &d,
        &[],
        "*** Begin Patch\n*** Add File: new.txt\n+x\n*** Update File: main.rs\n<<<<<<< SEARCH\nfn main() {\n>>>>>>> REPLACE\n*** End Patch\n",
    );
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o)
        .contains("malformed patch: stdin line 7: '>>>>>>> REPLACE' in block 1's search text"));
    assert!(err(&o).contains("(in the update of main.rs); nothing written"));
    assert_eq!(names(&d), ["main.rs"]);
    let o = run(&d, &[], "*** Begin Patch\n*** Add File: new.txt\n+x\n");
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).contains("cut short; end it with a '*** End Patch' line"));
    assert_eq!(names(&d), ["main.rs"]);
}

#[test]
fn dry_run_prints_what_it_would_do_and_writes_nothing() {
    let d = dir(&[("main.rs", OLD), ("gone.txt", "a\nb\n")]);
    let o = run(&d, &["--dry-run"], ALL_THREE);
    assert_eq!(o.status.code(), Some(0));
    assert!(out(&o).starts_with("== add src/new.rs: 3 lines\n"));
    assert!(err(&o).contains("dry run: nothing written"));
    assert_eq!(names(&d), ["gone.txt", "main.rs"]);
    assert_eq!(read(&d, "main.rs"), OLD);
}

#[test]
fn a_file_argument_is_refused_with_the_fix() {
    let d = dir(&[]);
    let o = run(&d, &["main.rs"], "");
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).contains("name each file in the patch on stdin"));
}

#[test]
fn help_shows_the_patch_format_and_exits_0() {
    let d = dir(&[]);
    let o = run(&d, &["--help"], "");
    assert_eq!(o.status.code(), Some(0));
    let h = out(&o);
    assert!(h.starts_with("usage: apply-patch [--dry-run] < patch\n"));
    assert!(h.contains("*** Add File: <path>"));
    assert!(h.contains("Exits 1 when any change cannot be made"));
}

#[test]
fn the_help_example_applies() {
    let d = dir(&[("old.txt", "a\n")]);
    let o = run(
        &d,
        &[],
        "*** Begin Patch\n*** Add File: new.txt\n+hello\n*** Update File: old.txt\n<<<<<<< SEARCH\na\n=======\nb\n>>>>>>> REPLACE\n*** End Patch\n",
    );
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(read(&d, "new.txt"), "hello\n");
    assert_eq!(read(&d, "old.txt"), "b\n");
}
