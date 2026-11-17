//! repo-glance's binary run on fixture repos made with the real git in a
//! temp dir of the test's own, isolated from the user's git config.
//! Each test prints what the binary printed, seen with `-- --nocapture`.
//! Every expected output is worked by hand; commit hashes come from git.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

const BIN: &str = env!("CARGO_BIN_EXE_repo-glance");

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A fresh directory of this test run's own.
fn scratch(name: &str) -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::SeqCst);
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("repo-glance")
        .join(format!("{name}-{}-{n}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A command with git's config isolated, and no repo found above `dir`'s
/// parent (the temp dir sits inside forge's own worktree).
fn isolated(cmd: &mut Command, dir: &Path) {
    cmd.current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CEILING_DIRECTORIES", dir.parent().unwrap())
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.com")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.com");
}

fn sh_git(dir: &Path, args: &[&str]) -> String {
    let mut c = Command::new("git");
    c.args(args);
    isolated(&mut c, dir);
    let o = c.output().unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8(o.stdout).unwrap()
}

fn commit(dir: &Path, file: &str, text: &str, msg: &str) {
    fs::write(dir.join(file), text).unwrap();
    sh_git(dir, &["add", file]);
    sh_git(dir, &["commit", "-q", "-m", msg]);
}

fn short(dir: &Path, rev: &str) -> String {
    sh_git(dir, &["rev-parse", "--short", rev])
        .trim()
        .to_string()
}

/// repo-glance run in `dir`.
fn glance(dir: &Path, args: &[&str]) -> Output {
    let mut c = Command::new(BIN);
    c.args(args);
    isolated(&mut c, dir);
    let o = c.output().unwrap();
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

/// A clone of an origin with commits one and two, a local commit three
/// (ahead 1), commit four on origin fetched (behind 1), README.md
/// modified, new.txt untracked, and a src/ directory.
fn fixture(name: &str) -> PathBuf {
    let root = scratch(name);
    let origin = root.join("origin");
    fs::create_dir_all(&origin).unwrap();
    sh_git(&origin, &["init", "-q", "-b", "main"]);
    commit(&origin, "README.md", "# r\n", "one");
    commit(&origin, "Cargo.toml", "[package]\nname = \"r\"\n", "two");
    sh_git(&root, &["clone", "-q", "origin", "work"]);
    let work = root.join("work");
    commit(&origin, "o.txt", "o\n", "four");
    fs::create_dir_all(work.join("src")).unwrap();
    commit(&work, "src/lib.rs", "pub fn f() {}\n", "three");
    sh_git(&work, &["fetch", "-q"]);
    fs::write(work.join("README.md"), "# r\nmore\n").unwrap();
    fs::write(work.join("new.txt"), "n\n").unwrap();
    work
}

#[test]
fn git_state_then_files_and_directories() {
    let w = fixture("state");
    let index = fs::read(w.join(".git/index")).unwrap();
    let o = glance(&w, &["README.md", "src", "Cargo.toml"]);
    assert_eq!(o.status.code(), Some(0));
    let expected = format!(
        "== git\n\
         branch main -> origin/main (ahead 1, behind 1)\n\
         status:\n \
         M README.md\n\
         ?? new.txt\n\
         last 5 commits:\n\
         {} three\n\
         {} two\n\
         {} one\n\
         \n\
         == README.md\n\
         1: # r\n\
         2: more\n\
         \n\
         == src\n\
         lib.rs\n\
         \n\
         == Cargo.toml\n\
         1: [package]\n\
         2: name = \"r\"\n",
        short(&w, "HEAD"),
        short(&w, "HEAD~1"),
        short(&w, "HEAD~2"),
    );
    assert_eq!(out(&o), expected);
    assert!(o.stderr.is_empty());
    // Read-only: the index is byte for byte as it was, and the status
    // is unchanged.
    assert_eq!(fs::read(w.join(".git/index")).unwrap(), index);
    assert_eq!(
        sh_git(&w, &["status", "--short"]),
        " M README.md\n?? new.txt\n"
    );
}

#[test]
fn only_the_last_five_commits() {
    let w = scratch("five");
    sh_git(&w, &["init", "-q", "-b", "trunk"]);
    for i in 1..=7 {
        commit(&w, "f", &format!("{i}\n"), &format!("c{i}"));
    }
    let o = glance(&w, &[]);
    assert_eq!(o.status.code(), Some(0));
    let s = out(&o);
    assert!(
        s.starts_with("== git\nbranch trunk (no upstream)\nstatus: clean\nlast 5 commits:\n"),
        "{s}"
    );
    let subjects: Vec<&str> = s
        .lines()
        .skip(4)
        .map(|l| l.split_once(' ').unwrap().1)
        .collect();
    assert_eq!(subjects, ["c7", "c6", "c5", "c4", "c3"]);
}

#[test]
fn a_repo_with_no_commits_yet() {
    let w = scratch("empty");
    sh_git(&w, &["init", "-q", "-b", "main"]);
    fs::write(w.join("a"), "a\n").unwrap();
    let o = glance(&w, &[]);
    assert_eq!(o.status.code(), Some(0));
    assert_eq!(
        out(&o),
        "== git\nbranch main (no upstream)\nstatus:\n?? a\nlast commits: none yet\n"
    );
}

#[test]
fn a_detached_head() {
    let w = scratch("detached");
    sh_git(&w, &["init", "-q", "-b", "main"]);
    commit(&w, "a", "a\n", "one");
    commit(&w, "a", "b\n", "two");
    sh_git(&w, &["checkout", "-q", "HEAD~1"]);
    let o = glance(&w, &[]);
    let s = out(&o);
    let head = sh_git(&w, &["rev-parse", "HEAD"]);
    assert!(
        s.starts_with(&format!(
            "== git\nbranch (detached at {}) (no upstream)\n",
            &head[..7]
        )),
        "{s}"
    );
}

#[test]
fn a_missing_path_is_said_and_the_rest_still_print() {
    let w = fixture("missing");
    let o = glance(&w, &["nope.md", "Cargo.toml"]);
    assert_eq!(o.status.code(), Some(2));
    let s = out(&o);
    assert!(s.starts_with("== git\nbranch main -> origin/main"), "{s}");
    assert!(
        s.contains(
            "\n== nope.md\nerror: no such file or directory; check the name with peek ., relative to the current directory or absolute\n\n== Cargo.toml\n1: [package]\n"
        ),
        "{s}"
    );
}

#[test]
fn outside_a_repo_it_says_so_and_still_prints_the_paths() {
    let d = scratch("outside");
    fs::write(d.join("a.txt"), "x\n").unwrap();
    let o = glance(&d, &["a.txt"]);
    assert_eq!(o.status.code(), Some(0));
    let dir = d.canonicalize().unwrap();
    let s = out(&o);
    let line = s.lines().nth(1).unwrap();
    assert!(line.starts_with("not a git repository: "), "{s}");
    assert!(
        line.ends_with("; to see git state, run repo-glance from inside a worktree"),
        "{s}"
    );
    assert!(
        line.contains(d.to_str().unwrap()) || line.contains(dir.to_str().unwrap()),
        "{s}"
    );
    assert!(s.ends_with("\n\n== a.txt\n1: x\n"), "{s}");
}

#[test]
fn without_git_on_path_it_says_how_to_fix_it() {
    let d = scratch("nogit");
    fs::write(d.join("a.txt"), "x\n").unwrap();
    let o = Command::new(BIN)
        .arg("a.txt")
        .current_dir(&d)
        .env("PATH", d.join("no-bin"))
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(2));
    let s = out(&o);
    assert!(
        s.starts_with("== git\nerror: git could not be run; put git on PATH"),
        "{s}"
    );
    assert!(s.ends_with("\n\n== a.txt\n1: x\n"), "{s}");
}

#[test]
fn usage_help_and_version() {
    let d = scratch("usage");
    let o = glance(&d, &["-x"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(o.stdout.is_empty());
    let e = String::from_utf8(o.stderr).unwrap();
    assert!(e.starts_with("usage: repo-glance [path…]\n"), "{e}");
    assert!(e.contains("put -- before it"), "{e}");

    fs::write(d.join("-x"), "dash\n").unwrap();
    let o = glance(&d, &["--", "-x"]);
    assert!(out(&o).ends_with("\n== -x\n1: dash\n"));

    let o = glance(&d, &["--help"]);
    assert_eq!(o.status.code(), Some(0));
    assert!(out(&o).starts_with("usage: repo-glance [path…]\n"));
    assert!(out(&o).contains("Exits 2 when git"));

    let o = glance(&d, &["--version"]);
    assert_eq!(out(&o), "repo-glance 0.1.1\n");
}
