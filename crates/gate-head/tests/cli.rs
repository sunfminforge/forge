//! The real binary, run in a temporary git repo with the fake guild
//! (`tests/fake/guild.rs`) first on the PATH: its stdout, its stderr, its
//! exit code, and the guild calls it made.

use std::path::{Path, PathBuf};
use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_gate-head");

/// The directory cargo built the fake guild into: `<target>/<profile>/examples`.
fn fakes() -> PathBuf {
    let dir = Path::new(BIN).parent().unwrap().join("examples");
    assert!(
        dir.join("guild").is_file(),
        "the fake guild is built by cargo test as an example, at {}",
        dir.display()
    );
    dir
}

/// A new empty directory under the system's temp dir.
fn temp_dir() -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "gate-head-test-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    std::fs::remove_dir_all(&dir).ok();
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

/// A git repo with one commit, its root holding `repo/`, the fake's files
/// beside it.
struct Repo {
    root: PathBuf,
    dir: PathBuf,
}

impl Repo {
    fn new() -> Repo {
        let root = temp_dir();
        let dir = root.join("repo");
        std::fs::create_dir_all(&dir).unwrap();
        git(&dir, &["init", "--quiet", "-b", "main"]);
        git(&dir, &["config", "user.email", "test@example.invalid"]);
        git(&dir, &["config", "user.name", "test"]);
        std::fs::write(dir.join("a.txt"), "a\n").unwrap();
        git(&dir, &["add", "a.txt"]);
        git(&dir, &["commit", "--quiet", "-m", "one"]);
        Repo { root, dir }
    }

    fn head(&self) -> String {
        git(&self.dir, &["rev-parse", "HEAD"])
    }

    fn tree(&self) -> String {
        git(&self.dir, &["rev-parse", "HEAD^{tree}"])
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).ok();
    }
}

struct Ran {
    stdout: String,
    stderr: String,
    exit: i32,
    /// Each guild call, its arguments space-joined.
    calls: Vec<String>,
}

/// The PATH the tests give: the fake guild first, then what was there.
fn path_with_fakes() -> String {
    format!(
        "{}:{}",
        fakes().display(),
        std::env::var("PATH").unwrap_or_default()
    )
}

/// Runs gate-head in `dir` with `path`, the fake guild printing `output`
/// and exiting `exit`; `root` holds the fake's files.
fn run_with(dir: &Path, root: &Path, path: &str, args: &[&str], output: &str, exit: u8) -> Ran {
    let out_file = root.join("guild-output");
    let calls_file = root.join("guild-calls");
    std::fs::write(&out_file, output).unwrap();
    std::fs::write(&calls_file, "").unwrap();
    let out = Command::new(BIN)
        .args(args)
        .current_dir(dir)
        .env("PATH", path)
        .env("GIT_CEILING_DIRECTORIES", root.parent().unwrap())
        .env("FAKE_GUILD_OUTPUT", &out_file)
        .env("FAKE_GUILD_EXIT", exit.to_string())
        .env("FAKE_GUILD_CALLS", &calls_file)
        .output()
        .unwrap();
    Ran {
        stdout: String::from_utf8(out.stdout).unwrap(),
        stderr: String::from_utf8(out.stderr).unwrap(),
        exit: out.status.code().unwrap(),
        calls: std::fs::read_to_string(&calls_file)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect(),
    }
}

fn run(repo: &Repo, args: &[&str], output: &str, exit: u8) -> Ran {
    run_with(
        &repo.dir,
        &repo.root,
        &path_with_fakes(),
        args,
        output,
        exit,
    )
}

fn reused(tree: &str) -> String {
    format!(
        "gate: reused the green run of `./gate.sh fast` on tree {tree} at 1794876508 on mac in 0s, on macos-aarch64: nothing ran\n"
    )
}

#[test]
fn green_on_heads_tree_named_by_a_reused_run_exits_0() {
    let repo = Repo::new();
    let (head, tree) = (repo.head(), repo.tree());
    let ran = run(&repo, &[], &reused(&tree), 0);
    assert_eq!(ran.exit, 0, "stderr: {}", ran.stderr);
    let expected = format!(
        "{}== gate-head\nhead commit: {head}\nhead tree: {tree}\ngate tree: {tree} (named by the gate's output)\ntree: matches HEAD\nworktree: clean\ngate: green\n",
        reused(&tree)
    );
    assert_eq!(ran.stdout, expected);
    assert_eq!(ran.stderr, "");
    assert_eq!(ran.calls, vec!["gate"], "guild gate runs once");
}

#[test]
fn green_on_heads_tree_run_afresh_exits_0() {
    let repo = Repo::new();
    let tree = repo.tree();
    let ran = run(&repo, &[], "checking fast\ngate: green\n", 0);
    assert_eq!(ran.exit, 0, "stderr: {}", ran.stderr);
    assert!(
        ran.stdout
            .starts_with("checking fast\ngate: green\n== gate-head\n"),
        "{}",
        ran.stdout
    );
    assert!(
        ran.stdout
            .contains(&format!("gate tree: {tree} (the run's own:")),
        "{}",
        ran.stdout
    );
    assert!(ran
        .stdout
        .contains("tree: matches HEAD\nworktree: clean\ngate: green\n"));
}

#[test]
fn green_on_another_tree_exits_1() {
    let repo = Repo::new();
    let other = "4ae03fcc6b0028d555a1ddc5588c941e7049a531";
    let ran = run(&repo, &[], &reused(other), 0);
    assert_eq!(ran.exit, 1);
    assert!(ran
        .stdout
        .contains(&format!("head tree: {}\n", repo.tree())));
    assert!(ran.stdout.contains(&format!(
        "gate tree: {other} (named by the gate's output)\n"
    )));
    assert!(
        ran.stdout
            .contains("tree: differs\nworktree: clean\ngate: green\n"),
        "{}",
        ran.stdout
    );
    assert!(ran.stderr.contains("not on HEAD's tree"), "{}", ran.stderr);
    assert!(
        ran.stderr.contains("run gate-head again"),
        "the fix: {}",
        ran.stderr
    );
}

#[test]
fn a_failing_gate_exits_with_its_code() {
    let repo = Repo::new();
    let ran = run(
        &repo,
        &[],
        "checking fast\ntest result: FAILED. 1 failed\n",
        3,
    );
    assert_eq!(ran.exit, 3);
    assert!(ran
        .stdout
        .starts_with("checking fast\ntest result: FAILED. 1 failed\n== gate-head\n"));
    assert!(
        ran.stdout.ends_with("gate: failed, exit 3\n"),
        "{}",
        ran.stdout
    );
    assert!(
        ran.stderr.contains("the gate failed, exit 3"),
        "{}",
        ran.stderr
    );
}

#[test]
fn a_failing_gate_on_heads_tree_still_exits_with_its_code() {
    let repo = Repo::new();
    let ran = run(&repo, &[], "gate: red\n", 1);
    assert_eq!(ran.exit, 1);
    assert!(
        ran.stdout
            .contains("tree: matches HEAD\nworktree: clean\ngate: failed, exit 1\n"),
        "{}",
        ran.stdout
    );
}

#[test]
fn a_dirty_worktree_exits_1_and_lists_its_paths() {
    let repo = Repo::new();
    std::fs::write(repo.dir.join("a.txt"), "changed\n").unwrap();
    std::fs::write(repo.dir.join("new.txt"), "new\n").unwrap();
    let ran = run(
        &repo,
        &[],
        "gate: not of a committed tree, so not on record:  M a.txt, ?? new.txt\n",
        0,
    );
    assert_eq!(ran.exit, 1);
    assert!(ran.stdout.contains("gate tree: none ("), "{}", ran.stdout);
    assert!(
        ran.stdout
            .contains("tree: differs\nworktree: dirty\n   M a.txt\n  ?? new.txt\ngate: green\n"),
        "{}",
        ran.stdout
    );
    assert!(
        ran.stderr.contains("worktree is not clean"),
        "{}",
        ran.stderr
    );
}

#[test]
fn a_dirty_worktree_with_heads_tree_named_still_exits_1() {
    let repo = Repo::new();
    std::fs::write(repo.dir.join("new.txt"), "new\n").unwrap();
    let ran = run(&repo, &[], &reused(&repo.tree()), 0);
    assert_eq!(ran.exit, 1);
    assert!(
        ran.stdout
            .contains("tree: matches HEAD\nworktree: dirty\n  ?? new.txt\n"),
        "{}",
        ran.stdout
    );
}

#[test]
fn outside_a_repo_exits_2_with_the_fix_and_runs_nothing() {
    let root = temp_dir();
    let dir = root.join("plain");
    std::fs::create_dir_all(&dir).unwrap();
    let ran = run_with(&dir, &root, &path_with_fakes(), &[], "", 0);
    assert_eq!(ran.exit, 2);
    assert_eq!(ran.stdout, "");
    assert!(
        ran.stderr
            .contains("gate-head: error: this directory is not inside a git work tree"),
        "{}",
        ran.stderr
    );
    assert!(ran.stderr.contains("(not_a_repo)"), "{}", ran.stderr);
    assert!(
        ran.stderr.contains("fix: cd <worktree> && gate-head"),
        "{}",
        ran.stderr
    );
    assert!(ran.calls.is_empty(), "guild was run: {:?}", ran.calls);
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn a_repo_with_no_commit_exits_2_with_the_fix() {
    let root = temp_dir();
    let dir = root.join("empty");
    std::fs::create_dir_all(&dir).unwrap();
    git(&dir, &["init", "--quiet"]);
    let ran = run_with(&dir, &root, &path_with_fakes(), &[], "", 0);
    assert_eq!(ran.exit, 2);
    assert!(ran.stderr.contains("(no_commit)"), "{}", ran.stderr);
    assert!(
        ran.stderr
            .contains("fix: git commit, then run gate-head again"),
        "{}",
        ran.stderr
    );
    assert!(ran.calls.is_empty());
    std::fs::remove_dir_all(&root).ok();
}

#[test]
fn no_guild_on_the_path_exits_2_with_the_fix() {
    let repo = Repo::new();
    // A PATH holding git alone, never the real guild.
    let git_dir = repo.root.join("only-git");
    std::fs::create_dir_all(&git_dir).unwrap();
    let real_git = std::env::var("PATH")
        .unwrap()
        .split(':')
        .map(|d| Path::new(d).join("git"))
        .find(|p| p.is_file())
        .unwrap();
    std::os::unix::fs::symlink(real_git, git_dir.join("git")).unwrap();
    let ran = run_with(&repo.dir, &repo.root, git_dir.to_str().unwrap(), &[], "", 0);
    assert_eq!(ran.exit, 2);
    assert!(ran.stderr.contains("(no_guild)"), "{}", ran.stderr);
    assert!(
        ran.stderr
            .contains("fix: put the directory holding guild on the PATH"),
        "{}",
        ran.stderr
    );
}

#[test]
fn slow_and_remote_are_passed_to_guild_gate() {
    let repo = Repo::new();
    let ran = run(
        &repo,
        &["--slow", "--remote", "forgejo"],
        &reused(&repo.tree()),
        0,
    );
    assert_eq!(ran.exit, 0);
    assert_eq!(ran.calls, vec!["gate --slow --remote forgejo"]);
}

#[test]
fn a_usage_error_exits_2_and_runs_nothing() {
    let repo = Repo::new();
    let ran = run(&repo, &["--fast"], "", 0);
    assert_eq!(ran.exit, 2);
    assert!(ran.stderr.contains("--fast"), "{}", ran.stderr);
    assert!(ran.calls.is_empty());
}

#[test]
fn it_never_changes_the_worktree_or_the_index() {
    let repo = Repo::new();
    // The same content with a new mtime: the index's stat of a.txt is stale,
    // so a git status that may take the index lock would rewrite it.
    std::fs::write(repo.dir.join("a.txt"), "a\n").unwrap();
    let touched = Command::new("touch")
        .args(["-t", "203001010000", "a.txt"])
        .current_dir(&repo.dir)
        .status()
        .unwrap();
    assert!(touched.success());
    std::fs::write(repo.dir.join("b.txt"), "untracked\n").unwrap();
    let index = std::fs::read(repo.dir.join(".git/index")).unwrap();
    let ran = run(&repo, &[], "gate: green\n", 0);
    assert_eq!(ran.exit, 1);
    assert_eq!(
        std::fs::read(repo.dir.join(".git/index")).unwrap(),
        index,
        "the index changed"
    );
    assert_eq!(
        std::fs::read_to_string(repo.dir.join("a.txt")).unwrap(),
        "a\n"
    );
    assert_eq!(
        std::fs::read_to_string(repo.dir.join("b.txt")).unwrap(),
        "untracked\n"
    );
}

#[test]
fn doctor_finds_nothing_when_git_and_guild_run() {
    let repo = Repo::new();
    let ran = run(&repo, &["doctor"], "", 0);
    assert_eq!(ran.exit, 0);
    assert_eq!(ran.stdout, "doctor: nothing found\n");
    assert_eq!(ran.calls, vec!["--version"]);
}

#[test]
fn help_names_guild_gate_heads_tree_and_the_exit_codes() {
    let out = Command::new(BIN).arg("--help").output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let help = String::from_utf8(out.stdout).unwrap();
    assert!(help.contains("gate-head [--slow] [--remote <name>]"));
    assert!(help.contains("`guild gate`"));
    assert!(help.contains("HEAD's tree"));
    for code in ["- `0`:", "- `1`:", "- `2`:", "The gate's own exit code"] {
        assert!(help.contains(code), "{code} missing from:\n{help}");
    }
    assert!(!help.contains("{const:"), "a placeholder left in:\n{help}");
    let all = Command::new(BIN).args(["help", "all"]).output().unwrap();
    assert!(!String::from_utf8(all.stdout).unwrap().contains("{const:"));
}
