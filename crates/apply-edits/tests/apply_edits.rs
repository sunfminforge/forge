//! apply-edits' binary run on copies of the fixture files in
//! tests/fixtures, each copied to a temp dir of the test's own, with its
//! blocks on stdin. Each test prints what the binary printed, seen with
//! `-- --nocapture`. Every expected output is worked by hand.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_apply-edits");

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A copy of fixture `name` in a fresh directory of this test's own.
fn fixture(name: &str) -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("apply-edits")
        .join(format!("{name}-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    let to = dir.join(name);
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
        &to,
    )
    .unwrap();
    to
}

fn original(name: &str) -> Vec<u8> {
    fs::read(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .unwrap()
}

/// apply-edits run with `args`, `stdin` sent on its standard input.
fn run(args: &[&str], stdin: &str) -> Output {
    let mut c = Command::new(BIN)
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

fn edit(file: &Path, stdin: &str) -> Output {
    run(&[file.to_str().unwrap()], stdin)
}

fn out(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}

fn err(o: &Output) -> String {
    String::from_utf8(o.stderr.clone()).unwrap()
}

const SEVERAL: &str = "\
<<<<<<< SEARCH
    8080
=======
    9090
>>>>>>> REPLACE

<<<<<<< SEARCH
    let listener = TcpListener::bind((\"127.0.0.1\", port())).unwrap();
    println!(\"listening\");
=======
    let addr = (\"0.0.0.0\", port());
    let listener = TcpListener::bind(addr).unwrap();
    eprintln!(\"listening on {}\", port());
>>>>>>> REPLACE
<<<<<<< SEARCH
    println!(\"todo\");
=======
    let _ = _s;
    println!(\"done\");
>>>>>>> REPLACE
";

#[test]
fn several_multi_line_blocks_apply_in_one_call_with_numbered_regions() {
    let f = fixture("server.rs");
    let o = edit(&f, SEVERAL);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        out(&o),
        "== block 1: 1 match, line 4\n\
         4:     9090\n\
         \n\
         == block 2: 1 match, lines 8-10\n\
         8:     let addr = (\"0.0.0.0\", port());\n\
         9:     let listener = TcpListener::bind(addr).unwrap();\n\
         10:     eprintln!(\"listening on {}\", port());\n\
         \n\
         == block 3: 1 match, lines 17-18\n\
         17:     let _ = _s;\n\
         18:     println!(\"done\");\n"
    );
    assert_eq!(
        fs::read_to_string(&f).unwrap(),
        "use std::net::TcpListener;\n\
         \n\
         fn port() -> u16 {\n    9090\n}\n\
         \n\
         fn main() {\n\
         \x20   let addr = (\"0.0.0.0\", port());\n\
         \x20   let listener = TcpListener::bind(addr).unwrap();\n\
         \x20   eprintln!(\"listening on {}\", port());\n\
         \x20   for stream in listener.incoming() {\n\
         \x20       handle(stream.unwrap());\n\
         \x20   }\n\
         }\n\
         \n\
         fn handle(_s: std::net::TcpStream) {\n\
         \x20   let _ = _s;\n\
         \x20   println!(\"done\");\n\
         }\n"
    );
}

#[test]
fn a_block_not_found_exits_1_and_leaves_the_file_as_it_was() {
    let f = fixture("server.rs");
    // Block 1 would apply; block 2's indentation is wrong.
    let o = edit(
        &f,
        "<<<<<<< SEARCH\n    8080\n=======\n    9090\n>>>>>>> REPLACE\n\
         <<<<<<< SEARCH\n  fn handle(_s: std::net::TcpStream) {\n=======\nx\n>>>>>>> REPLACE\n",
    );
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(out(&o), "");
    let e = err(&o);
    assert!(
        e.contains("1 of 2 blocks did not match exactly once; nothing written"),
        "{e}"
    );
    assert!(
        e.contains("block 2 ('  fn handle(_s: std::net::TcpStream) {'): not found, 0 matches in"),
        "{e}"
    );
    assert!(e.contains("a line like its first is at line 15"), "{e}");
    assert_eq!(fs::read(&f).unwrap(), original("server.rs"));
}

#[test]
fn a_block_found_twice_exits_1_and_leaves_the_file_as_it_was() {
    let f = fixture("dup.txt");
    let o = edit(
        &f,
        "<<<<<<< SEARCH\nversion = \"0.1.0\"\n=======\nversion = \"0.2.0\"\n>>>>>>> REPLACE\n",
    );
    assert_eq!(o.status.code(), Some(1));
    let e = err(&o);
    assert!(
        e.contains("block 1 ('version = \"0.1.0\"'): 2 matches in"),
        "{e}"
    );
    assert!(e.contains("at lines 2, 5"), "{e}");
    assert!(e.contains("<<<<<<< SEARCH all"), "{e}");
    assert_eq!(fs::read(&f).unwrap(), original("dup.txt"));
}

#[test]
fn all_matches_mode_replaces_every_match() {
    let f = fixture("dup.txt");
    let o = edit(
        &f,
        "<<<<<<< SEARCH all\nversion = \"0.1.0\"\n=======\nversion = \"0.2.0\"\n>>>>>>> REPLACE\n",
    );
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        out(&o),
        "== block 1: 2 matches, lines 2, 5\n\
         2: version = \"0.2.0\"\n\
         --\n\
         5: version = \"0.2.0\"\n"
    );
    assert_eq!(
        fs::read_to_string(&f).unwrap(),
        "name = \"a\"\nversion = \"0.2.0\"\n\nname = \"b\"\nversion = \"0.2.0\"\n"
    );
}

#[test]
fn an_empty_replace_deletes_the_text() {
    let f = fixture("server.rs");
    let o = edit(
        &f,
        "<<<<<<< SEARCH\nfn port() -> u16 {\n    8080\n}\n\n=======\n>>>>>>> REPLACE\n",
    );
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        out(&o),
        "== block 1: 1 match, deleted, joined at line 3\n\
         2: \n\
         3: fn main() {\n"
    );
    let now = fs::read_to_string(&f).unwrap();
    assert!(
        now.starts_with("use std::net::TcpListener;\n\nfn main() {\n"),
        "{now}"
    );
    assert!(!now.contains("8080"));
}

#[test]
fn a_malformed_block_exits_2_and_writes_nothing() {
    let f = fixture("server.rs");
    // Block 1 is whole; block 2 never reaches its ======= line.
    let o = edit(
        &f,
        "<<<<<<< SEARCH\n    8080\n=======\n    9090\n>>>>>>> REPLACE\n\
         <<<<<<< SEARCH\nfn main() {\n>>>>>>> REPLACE\n",
    );
    assert_eq!(o.status.code(), Some(2));
    assert_eq!(out(&o), "");
    let e = err(&o);
    assert!(e.contains("malformed blocks: stdin line 8"), "{e}");
    assert!(
        e.contains("end the search text with a '=======' line"),
        "{e}"
    );
    assert_eq!(fs::read(&f).unwrap(), original("server.rs"));

    let o = edit(&f, "here is the change:\n");
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).contains("text outside a block"));
    assert_eq!(fs::read(&f).unwrap(), original("server.rs"));
}

#[test]
fn an_unreadable_file_exits_2() {
    let f = fixture("server.rs");
    let missing = f.with_file_name("nope.rs");
    let o = edit(&missing, SEVERAL);
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).contains("no such file"), "{}", err(&o));
    assert!(!missing.exists());
    let o = edit(f.parent().unwrap(), SEVERAL);
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).contains("is a directory"));
}

#[test]
fn a_last_line_without_newline_matches_and_stays_without() {
    let f = fixture("noeol.toml");
    let o = edit(
        &f,
        "<<<<<<< SEARCH\nb = 2\n=======\nb = 3\n>>>>>>> REPLACE\n",
    );
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(out(&o), "== block 1: 1 match, line 2\n2: b = 3\n");
    assert_eq!(fs::read(&f).unwrap(), b"a = 1\nb = 3");
}

#[test]
fn dry_run_prints_the_regions_and_writes_nothing() {
    let f = fixture("server.rs");
    let o = run(&["--dry-run", f.to_str().unwrap()], SEVERAL);
    assert_eq!(o.status.code(), Some(0));
    assert!(out(&o).starts_with("== block 1: 1 match, line 4\n4:     9090\n"));
    assert!(err(&o).contains("dry run"));
    assert_eq!(fs::read(&f).unwrap(), original("server.rs"));
}

#[test]
fn help_and_version_answer_and_a_usage_error_exits_2() {
    let o = run(&["--help"], "");
    assert_eq!(o.status.code(), Some(0));
    assert!(out(&o).contains("<<<<<<< SEARCH all"));
    let o = run(&["--version"], "");
    assert_eq!(
        out(&o),
        format!("apply-edits {}\n", env!("CARGO_PKG_VERSION"))
    );
    let o = run(&[], "");
    assert_eq!(o.status.code(), Some(2));
    assert!(err(&o).contains("name the file to edit"));
}
