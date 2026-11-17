//! The effect part: reads the command line, reads git before and after one
//! `guild gate`, passes the gate's output on, and prints what
//! [`crate::decide`] makes of it.

use std::io::Write;
use std::process::{Command, ExitCode};

use agent_help::{Help, Topic};
use clap::{FromArgMatches, Parser, Subcommand};

use crate::decide::{
    advice, summary, verdict, Refusal, EXIT_GREEN_ON_HEAD, EXIT_NOT_HEAD, EXIT_REFUSED,
};
use crate::{git, guild};

/// Whether `guild gate` is green on exactly the worktree's HEAD tree, with nothing uncommitted, in one call: it runs `guild gate` once in the current directory, prints the gate's whole output as it comes, then a summary block naming HEAD's commit, HEAD's tree and the tree the gate's output names, and exits 0 only when the gate was green on HEAD's tree with the worktree clean.
///
/// It takes the place of `guild gate`, `guild gate` again (to see which tree a green run is of) and `git rev-parse HEAD^{tree}`. `--slow` and `--remote <name>` are passed to `guild gate` as given.
///
/// ## Exit codes
///
/// - `{const:EXIT_GREEN_ON_HEAD}`: the gate was green on HEAD's tree, and the worktree clean before and after it.
/// - The gate's own exit code: the gate failed (a code it cannot pass on, such as a signal's, is {const:EXIT_NOT_HEAD}).
/// - `{const:EXIT_NOT_HEAD}`: the gate was green, but on another tree, or the worktree was dirty.
/// - `{const:EXIT_REFUSED}`: outside a git repo, a usage error, or a program it needs cannot be run; the cause and the line that fixes it on stderr.
///
/// A non-zero exit also says on stderr why, and what to do. What the summary block holds: [the summary](help:summary).
///
/// ## Departure from the agent-cli way
///
/// Its stdout is not one JSON object: it is the gate's own output, as `guild gate` prints it, then the summary block, plain lines, so the gate's words are read as they are. A refusal is two lines on stderr, `gate-head: error: <cause>` and `fix: <line>`, not a JSON object.
///
/// It may refuse:
///
/// {table:refusal}
///
/// Run `gate-head doctor` to see that git and guild can be run from here.
#[derive(Parser)]
#[command(name = "gate-head", verbatim_doc_comment, disable_help_subcommand = true,
    args_conflicts_with_subcommands = true, version = env!("GATE_HEAD_VERSION"),
    override_usage = "gate-head [--slow] [--remote <name>]\n       gate-head doctor\n       gate-head help [<topic>|all]"
)]
struct Cli {
    /// Runs the slow gate: `guild gate --slow`.
    #[arg(long, verbatim_doc_comment)]
    slow: bool,
    /// The remote whose gate records `guild gate` reads and writes: `guild gate --remote <name>`; guild's own default when not given.
    #[arg(long, value_name = "NAME", verbatim_doc_comment)]
    remote: Option<String>,
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Says whether git and guild can be run from here: one line per finding, each `finding: <what>` then `fix: <line>`, or `doctor: nothing found`.
    ///
    /// It runs `git --version` and `guild --version`, and nothing else. Exit 0 either way.
    #[command(next_help_heading = "Run by the warden", verbatim_doc_comment)]
    Doctor,
    /// Shows one topic of this manual, or all of it: `gate-head help all`.
    #[command(next_help_heading = "Run by the warden", verbatim_doc_comment)]
    Help {
        /// The topic, such as `summary`, or `all`.
        #[arg(verbatim_doc_comment)]
        topic: Option<String>,
    },
}

/// The summary block gate-head prints after the gate's output, one fact a line.
///
/// ```text
/// == gate-head
/// head commit: 3f1c0a2e9d4b7c6a5f8e1d0c9b8a7f6e5d4c3b2a
/// head tree: 87ed25a41779279fbfa9a4d49f5e1c8fb52cd07f
/// gate tree: 87ed25a41779279fbfa9a4d49f5e1c8fb52cd07f (named by the gate's output)
/// tree: matches HEAD
/// worktree: clean
/// gate: green
/// ```
///
/// - `head commit`, `head tree`: what `git rev-parse HEAD` and `HEAD^{tree}` name after the run.
/// - `gate tree`: the tree of the line `` gate: reused the green run of `<command>` on tree <tree> … `` when the gate reused a green run on record. A run made afresh names no tree in its output: then it is the tree the run was of, HEAD's, `(the run's own: …)`, when the worktree was clean at the same HEAD before and after it, and `none (…)` when it was not.
/// - `tree: matches HEAD` or `tree: differs`.
/// - `worktree: clean`, or `worktree: dirty` followed by each path `git status --porcelain` listed before or after the run, indented two spaces.
/// - `gate: green`, or `gate: failed, exit <code>`.
///
/// It reads git only (`rev-parse`, `status` with `--no-optional-locks`): it never changes the worktree or the index.
#[derive(Topic)]
#[topic(name = "summary")]
struct Summary;

fn help() -> Help {
    Help::new(<Cli as clap::CommandFactory>::command())
        .topic::<Summary>()
        .table::<Refusal>("refusal")
        .constant("EXIT_GREEN_ON_HEAD", EXIT_GREEN_ON_HEAD)
        .constant("EXIT_NOT_HEAD", EXIT_NOT_HEAD)
        .constant("EXIT_REFUSED", EXIT_REFUSED)
}

/// Says a refusal on stderr: its cause, then the line that fixes it.
fn refuse(refusal: Refusal, seen: &str) -> ExitCode {
    let (cause, fix) = refusal.cause_and_fix(seen);
    eprintln!("gate-head: error: {cause} ({})", refusal.code());
    eprintln!("fix: {fix}");
    ExitCode::from(EXIT_REFUSED)
}

/// Reads the command line and answers it.
pub fn main() -> ExitCode {
    let help = help();
    let command = match help.clap() {
        Ok(command) => command,
        Err(error) => {
            eprintln!("gate-head: error: its manual is broken: {error}");
            return ExitCode::from(EXIT_REFUSED);
        }
    };
    let cli = match command
        .try_get_matches()
        .and_then(|m| Cli::from_arg_matches(&m))
    {
        Ok(cli) => cli,
        Err(error) => error.exit(),
    };
    match cli.command {
        Some(Cmd::Help { topic: None }) => {
            print!("{}", help.index().expect("checked by clap()"));
            ExitCode::SUCCESS
        }
        Some(Cmd::Help { topic: Some(topic) }) => match help.topic_help(&topic) {
            Ok(markdown) => {
                print!("{markdown}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("gate-head: error: {error}");
                eprintln!("fix: gate-head --help");
                ExitCode::from(EXIT_REFUSED)
            }
        },
        Some(Cmd::Doctor) => doctor(),
        None => gate(cli.slow, cli.remote.as_deref()),
    }
}

fn gate(slow: bool, remote: Option<&str>) -> ExitCode {
    let before = match git::seen() {
        Ok(seen) => seen,
        Err((refusal, seen)) => return refuse(refusal, &seen),
    };
    let mut out = std::io::stdout().lock();
    let mut open = true;
    let ran = guild::run(&guild::gate_args(slow, remote), |line| {
        // A reader that closed early ends the printing, not the run.
        open = open && writeln!(out, "{line}").and_then(|_| out.flush()).is_ok();
    });
    let (output, code) = match ran {
        Ok(ran) => ran,
        Err(why) => return refuse(Refusal::NoGuild, &why),
    };
    let after = match git::seen() {
        Ok(seen) => seen,
        Err((refusal, seen)) => return refuse(refusal, &seen),
    };
    let v = verdict(&output, code, &before, &after);
    if open {
        let _ = write!(out, "{}", summary(&v)).and_then(|_| out.flush());
    }
    if let Some(advice) = advice(&v) {
        eprintln!("{advice}");
    }
    ExitCode::from(v.exit)
}

fn doctor() -> ExitCode {
    let mut findings = Vec::new();
    for (program, put_right) in [
        (git::GIT, "put the directory holding git on the PATH (command -v git finds it)"),
        (guild::GUILD, "put the directory holding guild on the PATH (command -v guild finds it); then guild doctor"),
    ] {
        match Command::new(program).arg("--version").output() {
            Ok(out) if out.status.success() => {}
            Ok(out) => findings.push((
                format!("{program} --version exited {:?}: {}", out.status.code(), String::from_utf8_lossy(&out.stderr).trim()),
                put_right,
            )),
            Err(error) => findings.push((format!("{program} cannot be run from the PATH: {error}"), put_right)),
        }
    }
    if findings.is_empty() {
        println!("doctor: nothing found");
    }
    for (finding, fix) in findings {
        println!("finding: {finding}");
        println!("fix: {fix}");
    }
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    /// The manual names every command, topic and placeholder it uses.
    #[test]
    fn the_manual_is_whole() {
        agent_help::check(&super::help()).unwrap();
    }

    /// clap refuses a malformed command tree here, not on a user's run.
    #[test]
    fn the_command_tree_is_sound() {
        <super::Cli as clap::CommandFactory>::command().debug_assert();
    }
}
