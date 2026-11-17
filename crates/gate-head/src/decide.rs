//! What gate-head decides, with no I/O: the tree a gate's output names,
//! the tree a run was of, whether the gate was green on HEAD's tree with
//! the worktree clean, the exit code that says so, and the summary block.
//! What it decides from is read by `run` and passed in.

/// The exit of a gate green on HEAD's tree, the worktree clean.
pub const EXIT_GREEN_ON_HEAD: u8 = 0;

/// The exit of a gate green, but not on HEAD's tree or with the worktree
/// not clean.
pub const EXIT_NOT_HEAD: u8 = 1;

/// The exit of a refusal: outside a git repo, a usage error, or a program
/// it needs that cannot be run. Nothing was run, or nothing more.
pub const EXIT_REFUSED: u8 = 2;

/// The exit used for a gate that failed without an exit code of its own
/// (stopped by a signal) or with one outside 1 to 255.
pub const EXIT_GATE_NO_CODE: u8 = 1;

/// What opens the one line of `guild gate`'s output that names a tree: the
/// line of a reused green run, ``gate: reused the green run of `<command>`
/// on tree <tree> at …``. A run made afresh names none.
pub const REUSED_LINE: &str = "gate: reused the green run of `";

/// What precedes the tree on that line.
const ON_TREE: &str = "` on tree ";

/// Why gate-head refused: each refusal exits [`EXIT_REFUSED`] with its
/// cause and its fix on stderr.
#[derive(agent_help::Help, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The current directory is not inside a git work tree: run gate-head in the worktree whose gate you want, `cd <worktree> && gate-head`. Nothing was run.
    NotARepo,
    /// The repo has no commit yet, so HEAD has no tree: commit first, then run gate-head again. Nothing was run.
    NoCommit,
    /// git cannot be run from the PATH: put the directory holding git on the PATH (`command -v git` finds it). Nothing was run.
    NoGit,
    /// guild cannot be run from the PATH: put the directory holding guild on the PATH (`command -v guild` finds it), then run gate-head again.
    NoGuild,
}

impl Refusal {
    /// The code a refusal is named by.
    pub fn code(self) -> &'static str {
        agent_help::Table::entry(&self).name
    }

    /// The cause, given what was seen (`seen`: git's or the OS's words),
    /// and the line that fixes it.
    pub fn cause_and_fix(self, seen: &str) -> (String, String) {
        match self {
            Refusal::NotARepo => (
                format!("this directory is not inside a git work tree: {seen}"),
                "cd <worktree> && gate-head".into(),
            ),
            Refusal::NoCommit => (
                format!("HEAD names no commit yet, so it has no tree: {seen}"),
                "git commit, then run gate-head again".into(),
            ),
            Refusal::NoGit => (
                format!("git cannot be run: {seen}"),
                "put the directory holding git on the PATH (command -v git finds it)".into(),
            ),
            Refusal::NoGuild => (
                format!("guild cannot be run: {seen}"),
                "put the directory holding guild on the PATH (command -v guild finds it), then run gate-head again".into(),
            ),
        }
    }
}

/// Whether `text` is a git object name: 7 to 64 hex digits.
fn is_object_name(text: &str) -> bool {
    let n = text.len();
    let 7..=64 = n else {
        return false;
    };
    text.bytes().all(|b| b.is_ascii_hexdigit())
}

/// The tree one line names, when it is a reused run's line.
fn tree_of_line(line: &str) -> Option<&str> {
    let rest = line.strip_prefix(REUSED_LINE)?;
    let (_, after) = rest.rsplit_once(ON_TREE)?;
    let tree = match after.split_once(' ') {
        Some((tree, _)) => tree,
        None => after,
    };
    if is_object_name(tree) {
        Some(tree)
    } else {
        None
    }
}

/// The tree `guild gate`'s output names: that of its last reused-run line,
/// or `None` where no line names one (a run made afresh).
pub fn named_tree(output: &[String]) -> Option<String> {
    let mut found = None;
    for line in output {
        if let Some(tree) = tree_of_line(line) {
            found = Some(tree.to_string());
        }
    }
    found
}

/// The worktree as git showed it at one moment: the commit HEAD named, its
/// tree, and `git status --porcelain`'s lines (empty: clean).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    pub commit: String,
    pub tree: String,
    pub status: Vec<String>,
}

/// Where the gate's tree came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateTree {
    /// A line of the gate's output names it.
    Named(String),
    /// The output names none, and the run was of this tree: the worktree
    /// was clean at the same HEAD before and after it.
    Ran(String),
    /// The output names none, and the run was of no committed tree.
    None,
}

impl GateTree {
    /// The tree, where there is one.
    pub fn tree(&self) -> Option<&str> {
        match self {
            GateTree::Named(t) | GateTree::Ran(t) => Some(t),
            GateTree::None => None,
        }
    }
}

/// The tree the gate was green or red on: the one its output names, else
/// the one it ran on when the worktree was exactly a commit throughout.
pub fn gate_tree(output: &[String], before: &Seen, after: &Seen) -> GateTree {
    if let Some(tree) = named_tree(output) {
        return GateTree::Named(tree);
    }
    let still =
        before.status.is_empty() && after.status.is_empty() && before.commit == after.commit;
    if still {
        GateTree::Ran(before.tree.clone())
    } else {
        GateTree::None
    }
}

/// The exit code of a gate run as gate-head passes it on: 0 for 0, its own
/// code from 1 to 255, [`EXIT_GATE_NO_CODE`] for none or any other.
pub fn gate_code(code: Option<i32>) -> u8 {
    match code {
        Some(c @ 0..=255) => u8::try_from(c).unwrap_or(EXIT_GATE_NO_CODE),
        _ => EXIT_GATE_NO_CODE,
    }
}

/// gate-head's exit: the gate's code when it failed; 0 only when it was
/// green (`gate` 0) on HEAD's tree (`same`) with the worktree `clean`;
/// [`EXIT_NOT_HEAD`] for green otherwise.
#[flux_rs::sig(fn(gate: u8, same: bool, clean: bool) -> u8{v: (v == 0) <=> (gate == 0 && same && clean)})]
pub fn exit_code(gate: u8, same: bool, clean: bool) -> u8 {
    if gate != 0 {
        gate
    } else if same && clean {
        0
    } else {
        1
    }
}

/// What gate-head concludes from one run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    /// HEAD after the run.
    pub head: Seen,
    pub gate_tree: GateTree,
    /// The gate tree is HEAD's tree.
    pub same: bool,
    /// Every path git listed as changed or untracked, before or after the
    /// run, each once; empty: clean.
    pub dirty: Vec<String>,
    /// The gate's own exit code, as [`gate_code`] makes it.
    pub gate: u8,
    /// gate-head's exit.
    pub exit: u8,
}

/// The verdict on one run: `before` and `after` as git showed the worktree
/// either side of it, `output` its lines, `code` its exit.
pub fn verdict(output: &[String], code: Option<i32>, before: &Seen, after: &Seen) -> Verdict {
    let gate_tree = gate_tree(output, before, after);
    let same = gate_tree.tree() == Some(after.tree.as_str());
    let mut dirty: Vec<String> = Vec::new();
    for line in before.status.iter().chain(after.status.iter()) {
        if !dirty.contains(line) {
            dirty.push(line.clone());
        }
    }
    let gate = gate_code(code);
    let exit = exit_code(gate, same, dirty.is_empty());
    Verdict {
        head: after.clone(),
        gate_tree,
        same,
        dirty,
        gate,
        exit,
    }
}

/// The summary block printed after the gate's output.
pub fn summary(v: &Verdict) -> String {
    let mut s = String::from("== gate-head\n");
    s.push_str(&format!("head commit: {}\n", v.head.commit));
    s.push_str(&format!("head tree: {}\n", v.head.tree));
    let gate_tree = match &v.gate_tree {
        GateTree::Named(t) => format!("{t} (named by the gate's output)"),
        GateTree::Ran(t) => format!("{t} (the run's own: its output names none, and the worktree was clean at this HEAD before and after it)"),
        GateTree::None => "none (its output names none, and the worktree was not clean at one HEAD throughout the run)".to_string(),
    };
    s.push_str(&format!("gate tree: {gate_tree}\n"));
    s.push_str(if v.same {
        "tree: matches HEAD\n"
    } else {
        "tree: differs\n"
    });
    if v.dirty.is_empty() {
        s.push_str("worktree: clean\n");
    } else {
        s.push_str("worktree: dirty\n");
        for line in &v.dirty {
            s.push_str(&format!("  {line}\n"));
        }
    }
    if v.gate == 0 {
        s.push_str("gate: green\n");
    } else {
        s.push_str(&format!("gate: failed, exit {}\n", v.gate));
    }
    s
}

/// What stderr says of an exit that is not 0: why, and what to do, as one
/// line; `None` for 0.
pub fn advice(v: &Verdict) -> Option<String> {
    if v.gate != 0 {
        return Some(format!(
            "gate-head: the gate failed, exit {}: put right what its output names, commit, then run gate-head again",
            v.gate
        ));
    }
    match (v.same, v.dirty.is_empty()) {
        (true, true) => None,
        (_, false) => Some("gate-head: green, but the worktree is not clean: commit or remove the paths listed under worktree: dirty, then run gate-head again".into()),
        (false, true) => Some("gate-head: green, but not on HEAD's tree: run gate-head again in this worktree, with nothing uncommitted".into()),
    }
}
