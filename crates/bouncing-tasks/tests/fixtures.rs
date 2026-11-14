//! The fixture tests of bouncing-tasks, against the binary forge releases.
//!
//! Each case builds an L3 from fixtures/*.sql in a scratch directory, runs
//! the detector as the scheduler would, with only GUILD_L3 and
//! GUILD_FORGEJO_* set, and checks what it printed and how it exited.

mod findings;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use findings::parse;

const DETECTOR: &str = env!("CARGO_BIN_EXE_bouncing-tasks");
const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures");

/// A scratch directory, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(case: &str) -> Scratch {
        let dir =
            std::env::temp_dir().join(format!("bouncing-tasks-{case}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// An L3 in `scratch` made of the fixtures named, in order.
fn l3(scratch: &Scratch, names: &[&str]) -> PathBuf {
    let path = scratch.0.join("l3.sqlite");
    let db = rusqlite::Connection::open(&path).unwrap();
    for name in names {
        let sql = std::fs::read_to_string(Path::new(FIXTURES).join(name)).unwrap();
        db.execute_batch(&sql).unwrap();
    }
    db.close().unwrap();
    path
}

fn run(path: &Path) -> (i32, String) {
    let out = Command::new(DETECTOR)
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("GUILD_L3", path)
        .env("GUILD_FORGEJO_URL", "http://127.0.0.1:9")
        .env("GUILD_FORGEJO_TOKEN", "")
        .output()
        .expect("the detector runs");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8(out.stdout).unwrap(),
    )
}

#[test]
fn finds_work_passed_between_roles_task_after_task_landed_or_not() {
    let scratch = Scratch::new("bounce");
    let (code, out) = run(&l3(&scratch, &["schema.sql", "quiet.sql", "bounce.sql"]));
    assert_eq!(code, 0, "exited {code}");
    let found = parse(&out);
    let mut roles: Vec<&str> = found.iter().map(|f| f.role.as_str()).collect();
    roles.sort();
    assert_eq!(
        roles,
        [
            "handler-writer",
            "rust-cli-coder",
            "schema-writer",
            "test-writer",
            "web-coder"
        ]
    );
    let shop: Vec<_> = found
        .iter()
        .filter(|f| f.role == "rust-cli-coder" || f.role == "web-coder")
        .collect();
    assert_eq!(
        shop[0].saw, shop[1].saw,
        "one saw for every role the work passed through"
    );
    assert!(
        shop[0].saw.contains("(rust-cli-coder, web-coder)"),
        "{}",
        shop[0].saw
    );
    assert_eq!(
        shop[0].evidence,
        ["goal guild/tasks#40, repo shop: #41 rust-cli-coder -> #42 web-coder -> #43 rust-cli-coder; 2 hand-offs, then it landed"]
    );
    let api = found.iter().find(|f| f.role == "test-writer").unwrap();
    assert_eq!(
        api.evidence,
        ["goal guild/tasks#80, repo api: #81 schema-writer -> #82 handler-writer -> #83 test-writer; 2 hand-offs, not landed yet"]
    );
}

#[test]
fn prints_byte_for_byte_what_the_script_detector_printed() {
    let scratch = Scratch::new("bytes");
    let (code, out) = run(&l3(&scratch, &["schema.sql", "quiet.sql", "bounce.sql"]));
    assert_eq!(
        (code, out.as_str()),
        (0, include_str!("expected/bounce.txt"))
    );
}

#[test]
fn stays_quiet_on_one_hand_off_one_role_a_landing_between_hand_offs_and_the_wardens_reviews() {
    let scratch = Scratch::new("quiet");
    assert_eq!(
        run(&l3(&scratch, &["schema.sql", "quiet.sql"])),
        (0, String::new())
    );
}

#[test]
fn stays_quiet_on_an_empty_l3() {
    let scratch = Scratch::new("empty");
    assert_eq!(run(&l3(&scratch, &["schema.sql"])), (0, String::new()));
}

#[test]
fn stays_quiet_exit_0_with_no_l3_to_read() {
    let scratch = Scratch::new("missing");
    assert_eq!(run(&scratch.0.join("missing.sqlite")), (0, String::new()));
}

#[test]
fn reads_l3_without_writing_it() {
    let scratch = Scratch::new("readonly");
    let path = l3(&scratch, &["schema.sql", "quiet.sql", "bounce.sql"]);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap();
    std::fs::set_permissions(&scratch.0, std::fs::Permissions::from_mode(0o555)).unwrap();
    let (code, out) = run(&path);
    std::fs::set_permissions(&scratch.0, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        code == 0 && out.matches("[[finding]]").count() == 5,
        "{code} {out}"
    );
}

#[test]
fn answers_help_with_its_usage() {
    let out = Command::new(DETECTOR).arg("--help").output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8(out.stdout)
        .unwrap()
        .starts_with("usage: bouncing-tasks\n"));
}

#[test]
fn refuses_an_argument_naming_how_to_run_it() {
    let out = Command::new(DETECTOR).arg("--days").output().unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert!(String::from_utf8(out.stderr)
        .unwrap()
        .contains("run it as `bouncing-tasks`"));
}
