//! The git commands gate-head runs, each only reading: where HEAD is, its
//! tree, and what `git status` lists. `--no-optional-locks` keeps `status`
//! from refreshing the index, so nothing in the worktree or the index is
//! changed.

use std::process::Command;

use crate::decide::{Refusal, Seen};

/// The program run, found on the PATH.
pub const GIT: &str = "git";

/// Runs `git <args>`: its stdout on success, else the refusal and what git
/// said.
fn git(args: &[&str], on_fail: Refusal) -> Result<String, (Refusal, String)> {
    let out = Command::new(GIT)
        .arg("--no-optional-locks")
        .args(args)
        .output()
        .map_err(|e| (Refusal::NoGit, e.to_string()))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let said = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let said = if said.is_empty() {
            format!("git {} exited {:?}", args.join(" "), out.status.code())
        } else {
            said
        };
        Err((on_fail, said))
    }
}

/// The worktree as git shows it now, in the current directory.
pub fn seen() -> Result<Seen, (Refusal, String)> {
    git(&["rev-parse", "--is-inside-work-tree"], Refusal::NotARepo)?;
    let commit = git(
        &["rev-parse", "--verify", "HEAD^{commit}"],
        Refusal::NoCommit,
    )?;
    let tree = git(&["rev-parse", "--verify", "HEAD^{tree}"], Refusal::NoCommit)?;
    let status = git(&["status", "--porcelain"], Refusal::NotARepo)?;
    Ok(Seen {
        commit: commit.trim().to_string(),
        tree: tree.trim().to_string(),
        status: status
            .lines()
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect(),
    })
}
