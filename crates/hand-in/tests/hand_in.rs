//! hand-in's binary run against a stand-in guild, a shell script alone on
//! PATH, never the real Hub. The stand-in writes each call it gets (its
//! arguments, and the lesson's stdin, read by a shell builtin since PATH
//! holds only it) to `calls`, answers as a test asks,
//! and prints what it was given. Each test prints what the binary printed,
//! seen with `-- --nocapture`. Every expected output is worked by hand.

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_hand-in");

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A fresh directory with a `bin/` holding a stand-in guild. `refuse` is
/// the subcommand (`task check`, `lesson`, `task used`) it refuses, if any.
fn scratch(refuse: Option<&str>) -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let d = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("hand-in")
        .join(format!("{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("bin")).unwrap();
    let calls = d.join("calls");
    let refuse_case = match refuse {
        Some(r) => format!("  \"{r}\"*) echo \"refused: {r} said no\" >&2; exit 1;;\n"),
        None => String::new(),
    };
    let script = format!(
        "#!/bin/sh\n\
         echo call: \"$*\" >> '{c}'\n\
         if [ \"$1\" = lesson ]; then echo stdin: >> '{c}'; while IFS= read -r l; do echo \"$l\" >> '{c}'; done; fi\n\
         case \"$*\" in\n{refuse_case}esac\n\
         echo \"kept: $*\"\n",
        c = calls.display()
    );
    let p = d.join("bin/guild");
    fs::write(&p, script).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    d
}

/// hand-in run with `d/bin` as its only PATH, `stdin` piped in.
fn run(d: &Path, args: &[&str], stdin: &str) -> Output {
    let mut c = Command::new(BIN)
        .args(args)
        .env_clear()
        .env("PATH", d.join("bin"))
        .current_dir(d)
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

/// What the stand-in was called with, or "" when never called.
fn calls(d: &Path) -> String {
    fs::read_to_string(d.join("calls")).unwrap_or_default()
}

#[test]
fn an_item_check_runs_guild_task_check_with_its_evidence() {
    let d = scratch(None);
    let o = run(
        &d,
        &[
            "313",
            "--check",
            "tests",
            "--evidence",
            "test result: ok. 9 passed",
        ],
        "",
    );
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        calls(&d),
        "call: task check 313 tests --evidence test result: ok. 9 passed\n"
    );
    assert_eq!(
        out(&o),
        "== check tests: recorded\nkept: task check 313 tests --evidence test result: ok. 9 passed\n"
    );
}

#[test]
fn a_lesson_goes_on_stdin_with_role_task_failed_and_each_external() {
    let d = scratch(None);
    let o = run(
        &d,
        &[
            "guild/tasks#313",
            "--role",
            "rust-cli-coder",
            "--lesson",
            "one\ntwo\n",
            "--failed",
            "--external",
            "issue",
            "--external",
            "web page",
        ],
        "",
    );
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        calls(&d),
        "call: lesson --role rust-cli-coder --task guild/tasks#313 --failed --external issue --external web page\nstdin:\none\ntwo\n"
    );
    assert_eq!(
        out(&o),
        "== lesson: recorded\nkept: lesson --role rust-cli-coder --task guild/tasks#313 --failed --external issue --external web page\n"
    );
}

#[test]
fn a_lesson_is_read_from_stdin_with_a_dash() {
    let d = scratch(None);
    let o = run(
        &d,
        &["313", "--role", "warden", "--lesson", "-"],
        "read me\n",
    );
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        calls(&d),
        "call: lesson --role warden --task 313\nstdin:\nread me\n"
    );
}

#[test]
fn used_entries_go_in_one_guild_task_used_call() {
    let d = scratch(None);
    let o = run(
        &d,
        &["313", "--used", "a/core.md#x", "--used", "a/core.md#y"],
        "",
    );
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(calls(&d), "call: task used 313 a/core.md#x a/core.md#y\n");
    assert_eq!(
        out(&o),
        "== used: recorded\nkept: task used 313 a/core.md#x a/core.md#y\n"
    );
}

#[test]
fn a_lesson_over_three_lines_is_refused_before_anything_is_sent() {
    let d = scratch(None);
    let o = run(
        &d,
        &["313", "--role", "warden", "--lesson", "-"],
        "1\n2\n3\n4\n",
    );
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(calls(&d), "", "guild was never called");
    assert_eq!(
        out(&o),
        "== lesson: refused\nthe lesson is 4 lines, at most 3 are taken; nothing was sent. Cut it to 3 lines and run hand-in again with only --role and --lesson\n"
    );
}

#[test]
fn an_empty_lesson_is_refused_before_anything_is_sent() {
    let d = scratch(None);
    let o = run(&d, &["313", "--role", "warden", "--lesson", "\n"], "");
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(calls(&d), "");
    assert!(out(&o).starts_with("== lesson: refused\nthe lesson is empty; nothing was sent."));
}

#[test]
fn a_refused_lesson_is_reported_alongside_the_check_and_used_that_were_recorded() {
    let d = scratch(None);
    let o = run(
        &d,
        &[
            "313",
            "--check",
            "tests",
            "--evidence",
            "ok",
            "--role",
            "warden",
            "--lesson",
            "1\n2\n3\n4",
            "--used",
            "w/core.md#z",
        ],
        "",
    );
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        calls(&d),
        "call: task check 313 tests --evidence ok\ncall: task used 313 w/core.md#z\n"
    );
    assert_eq!(
        out(&o),
        "== check tests: recorded\nkept: task check 313 tests --evidence ok\n\n\
         == lesson: refused\nthe lesson is 4 lines, at most 3 are taken; nothing was sent. Cut it to 3 lines and run hand-in again with only --role and --lesson\n\n\
         == used: recorded\nkept: task used 313 w/core.md#z\n"
    );
}

#[test]
fn a_part_guild_refuses_is_reported_and_the_others_still_run() {
    let d = scratch(Some("task check 313 b"));
    let o = run(
        &d,
        &[
            "313",
            "--check",
            "a",
            "--evidence",
            "x",
            "--check",
            "b",
            "--evidence",
            "y",
            "--role",
            "warden",
            "--lesson",
            "fine",
            "--used",
            "w/core.md#z",
        ],
        "",
    );
    assert_eq!(o.status.code(), Some(1));
    assert_eq!(
        calls(&d),
        "call: task check 313 a --evidence x\n\
         call: task check 313 b --evidence y\n\
         call: lesson --role warden --task 313\nstdin:\nfine\n\
         call: task used 313 w/core.md#z\n"
    );
    assert_eq!(
        out(&o),
        "== check a: recorded\nkept: task check 313 a --evidence x\n\n\
         == check b: refused\nrefused: task check 313 b said no\nguild exited 1; fix what it says and run hand-in again with only this part\n\n\
         == lesson: recorded\nkept: lesson --role warden --task 313\n\n\
         == used: recorded\nkept: task used 313 w/core.md#z\n"
    );
}

#[test]
fn no_guild_on_path_refuses_every_part_with_the_fix() {
    let d = scratch(None);
    fs::remove_file(d.join("bin/guild")).unwrap();
    let o = run(&d, &["313", "--used", "w/core.md#z"], "");
    assert_eq!(o.status.code(), Some(1));
    let s = out(&o);
    assert!(s.starts_with("== used: refused\ncannot run guild: "), "{s}");
    assert!(s.contains("put guild's directory on PATH"));
}

#[test]
fn a_usage_error_runs_nothing_and_says_the_fix() {
    let d = scratch(None);
    for (args, fix) in [
        (&["313"][..], "name at least one part"),
        (&["313", "--check", "a"][..], "--check a --evidence <text>"),
        (
            &["313", "--lesson", "x"][..],
            "--lesson needs --role <role>",
        ),
        (
            &["313", "--external", "issue", "--used", "a#b"][..],
            "add --lesson <text>",
        ),
    ] {
        let o = run(&d, args, "");
        assert_eq!(o.status.code(), Some(2));
        assert_eq!(out(&o), "");
        assert!(err(&o).contains(fix), "{}", err(&o));
        assert!(err(&o).starts_with("usage: hand-in <task>"));
    }
    assert_eq!(calls(&d), "");
}

#[test]
fn help_and_version_answer_exit_zero() {
    let d = scratch(None);
    let o = run(&d, &["--help"], "");
    assert_eq!(o.status.code(), Some(0));
    assert!(out(&o).starts_with("usage: hand-in <task> [--check <item> --evidence <text>]..."));
    let o = run(&d, &["--version"], "");
    assert_eq!(out(&o), format!("hand-in {}\n", env!("CARGO_PKG_VERSION")));
}
