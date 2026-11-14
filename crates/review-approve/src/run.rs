//! The effect part: reads the command line, runs `guild` as [`crate::plan`]
//! decides, and answers one JSON object on stdout.

use std::process::{Command, ExitCode};

use agent_help::{Help, Topic};
use clap::{FromArgMatches, Parser, Subcommand};
use serde_json::{json, Value};

use crate::plan::{shell_line, Outcome, Request, Run, Step, NAME};
use crate::refusal::{Fault, Refusal};

/// The program each step runs, found on the PATH.
pub const GUILD: &str = "guild";

/// The exit of a request refused before any step ran.
pub const EXIT_REFUSED: u8 = 2;

/// The exit of a run that stopped at a failed step.
pub const EXIT_STOPPED: u8 = 1;

/// The warden's review of a task and the approval of its pull requests, in one command: `guild task review <task> --pass --evidence <text>`, then `guild pull approve <repo> <number>` for each pull, in order.
///
/// It stops at the first step that fails, exits non-zero and says which step it was; it never approves a pull whose review did not pass. Every command answers one JSON object on stdout; see [the answer's shape](help:answers).
///
/// Run `review-approve pass --help` before its first run, and `review-approve doctor` to see that guild can be run.
#[derive(Parser)]
#[command(name = "review-approve", verbatim_doc_comment, disable_help_subcommand = true, version = env!("REVIEW_APPROVE_VERSION"),
    override_usage = "review-approve pass <task> --evidence <text> --pull <repo>#<number> [--pull <repo>#<number>]... [--dry-run]\n       review-approve doctor\n       review-approve help [<topic>|all]"
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Reviews a task as passed, then approves each of its pulls: `review-approve pass <task> --evidence <text> --pull <repo>#<number>...`.
    ///
    /// ## When
    ///
    /// When the warden's review of a task passed, in place of `guild task review <task> --pass --evidence <text>` followed by one `guild pull approve <repo> <number>` per pull. A review that sends work back is still `guild task review --send-back`; this command only passes.
    ///
    /// ## What it runs
    ///
    /// 1. `guild task review <task> --pass --evidence <text>`
    /// 2. `guild pull approve <repo> <number>` for each `--pull`, in the order given.
    ///
    /// Each step runs only when the one before it exited 0. guild is found on the PATH; its stdout and stderr are kept in the answer, not printed.
    ///
    /// ## What it answers
    ///
    /// Exit 0, every step done:
    ///
    /// ```json
    /// {"task": "guild/tasks#59", "reviewed": true, "approved": ["guild/forge#6"], "dry_run": false, "steps": [{"step": "review", "command": "guild task review guild/tasks#59 --pass --evidence 'gate green'", "exit": 0, "stdout": "", "stderr": ""}, {"step": "approve guild/forge#6", "command": "guild pull approve guild/forge 6", "exit": 0, "stdout": "", "stderr": ""}]}
    /// ```
    ///
    /// With `--dry-run` it runs nothing and answers the same keys, `reviewed` false, `approved` empty, and each step with its `command` alone.
    ///
    /// Exit {const:EXIT_STOPPED}, a step failed: `{"error", "step", "faults", "reviewed", "approved", "not_approved", "steps"}`, where `step` is the step that failed (`review` or `approve <repo>#<number>`), and `steps` holds every step run, the failed one last. Exit {const:EXIT_REFUSED}: the request was refused and nothing ran.
    ///
    /// ## What it may refuse
    ///
    /// {table:refusal}
    ///
    /// ## What trips you up
    ///
    /// After an approve fails the review has passed: run the `guild pull approve` lines of the fault's `fix`, not this command again.
    #[command(next_help_heading = "Run by the warden", verbatim_doc_comment)]
    #[command(
        override_usage = "review-approve pass <task> --evidence <text> --pull <repo>#<number> [--pull <repo>#<number>]... [--dry-run]"
    )]
    Pass {
        /// The task, as `guild task review` takes it, such as `guild/tasks#59`.
        #[arg(verbatim_doc_comment)]
        task: String,
        /// What the review saw, recorded as the review's evidence.
        #[arg(long, value_name = "TEXT", verbatim_doc_comment)]
        evidence: String,
        /// A pull request to approve after the review, `<repo>#<number>` such as `guild/forge#6`; once per pull, approved in the order given.
        #[arg(long = "pull", value_name = "REPO#NUMBER", verbatim_doc_comment)]
        pulls: Vec<String>,
        /// Answers the commands it would run, and runs none.
        #[arg(long, verbatim_doc_comment)]
        dry_run: bool,
    },
    /// Says whether guild can be run from here: `{"findings": [...]}`, empty when it can.
    ///
    /// It runs `guild --version` and nothing else. Each finding is `{"finding", "fix"}`. Exit 0 either way.
    #[command(next_help_heading = "Run by the warden", verbatim_doc_comment)]
    Doctor,
    /// Shows one topic of this manual, or all of it: `review-approve help all`.
    #[command(next_help_heading = "Run by the warden", verbatim_doc_comment)]
    Help {
        /// The topic, such as `answers`, or `all`.
        #[arg(verbatim_doc_comment)]
        topic: Option<String>,
    },
}

/// Every answer is one JSON object on stdout; a failure is `{"error": "<summary>", "faults": [{"code", "message", "fix"}]}` with a non-zero exit.
///
/// `fix` is the command line that puts the fault right. Exit 0 is an answer, {const:EXIT_STOPPED} a step that failed, {const:EXIT_REFUSED} a request refused before anything ran. A command line clap cannot read is answered by clap's own error on stderr, exit 2.
#[derive(Topic)]
#[topic(name = "answers")]
struct Answers;

fn help() -> Help {
    Help::new(<Cli as clap::CommandFactory>::command())
        .topic::<Answers>()
        .table::<Refusal>("refusal")
        .constant("EXIT_REFUSED", EXIT_REFUSED)
        .constant("EXIT_STOPPED", EXIT_STOPPED)
}

/// Reads the command line and answers it.
pub fn main() -> ExitCode {
    let help = help();
    let command = match help.clap() {
        Ok(command) => command,
        Err(error) => {
            return answer(
                &json!({"error": error.to_string(), "faults": []}),
                EXIT_REFUSED,
            )
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
        Cmd::Help { topic: None } => {
            print!("{}", help.index().expect("checked by clap()"));
            ExitCode::SUCCESS
        }
        Cmd::Help { topic: Some(topic) } => match help.topic_help(&topic) {
            Ok(markdown) => {
                print!("{markdown}");
                ExitCode::SUCCESS
            }
            Err(error) => answer(
                &json!({"error": error.to_string(), "faults": [{
                    "code": "no_topic", "message": error.to_string(), "fix": format!("{NAME} --help"),
                }]}),
                EXIT_REFUSED,
            ),
        },
        Cmd::Doctor => doctor(),
        Cmd::Pass {
            task,
            evidence,
            pulls,
            dry_run,
        } => pass(&task, &evidence, &pulls, dry_run),
    }
}

fn pass(task: &str, evidence: &str, pulls: &[String], dry_run: bool) -> ExitCode {
    let request = match Request::read(task, evidence, pulls) {
        Ok(request) => request,
        Err(faults) => {
            return answer(
                &json!({
                    "error": format!("{NAME} refused the request: nothing was run"),
                    "faults": faults.iter().map(fault_json).collect::<Vec<_>>(),
                }),
                EXIT_REFUSED,
            )
        }
    };
    if dry_run {
        let steps: Vec<Value> = request
            .steps()
            .iter()
            .map(|step| json!({"step": step.label(), "command": shell_line(GUILD, &step.args())}))
            .collect();
        return answer(
            &json!({"task": request.task(), "reviewed": false, "approved": [], "dry_run": true, "steps": steps}),
            0,
        );
    }
    let mut steps = Vec::new();
    let mut run: Run<'_> = request.run();
    while let Some(step) = run.next() {
        let (outcome, record) = take(&step);
        steps.push(record);
        run = match run.record(&outcome) {
            Ok(run) => run,
            Err(stopped) => {
                let labels = |pulls: &[crate::plan::Pull]| -> Vec<String> {
                    pulls.iter().map(|p| p.label()).collect()
                };
                return answer(
                    &json!({
                        "error": format!("{NAME} stopped at step {}: {}", stopped.step, stopped.fault.message),
                        "step": stopped.step,
                        "faults": [fault_json(&stopped.fault)],
                        "reviewed": stopped.fault.code != Refusal::ReviewFailed.code(),
                        "approved": labels(&stopped.approved),
                        "not_approved": labels(&stopped.not_approved),
                        "steps": steps,
                    }),
                    EXIT_STOPPED,
                );
            }
        };
    }
    let approved: Vec<String> = run.approved().iter().map(|p| p.label()).collect();
    answer(
        &json!({"task": request.task(), "reviewed": run.reviewed(), "approved": approved, "dry_run": false, "steps": steps}),
        0,
    )
}

/// Runs one step's guild command and keeps what it said.
fn take(step: &Step<'_>) -> (Outcome, Value) {
    let args = step.args();
    let command = shell_line(GUILD, &args);
    match Command::new(GUILD).args(&args).output() {
        Ok(out) => {
            let outcome = if out.status.success() {
                Outcome::Succeeded
            } else {
                Outcome::Failed {
                    exit: out.status.code(),
                }
            };
            let record = json!({
                "step": step.label(),
                "command": command,
                "exit": out.status.code(),
                "stdout": String::from_utf8_lossy(&out.stdout),
                "stderr": String::from_utf8_lossy(&out.stderr),
            });
            (outcome, record)
        }
        Err(error) => {
            let why = error.to_string();
            let record =
                json!({"step": step.label(), "command": command, "exit": null, "error": why});
            (Outcome::NotStarted { why }, record)
        }
    }
}

fn doctor() -> ExitCode {
    let findings = match Command::new(GUILD).arg("--version").output() {
        Ok(out) if out.status.success() => vec![],
        Ok(out) => vec![json!({
            "finding": format!("guild --version exited {:?}: {}", out.status.code(), String::from_utf8_lossy(&out.stderr).trim()),
            "fix": "run guild doctor and do what it says",
        })],
        Err(error) => vec![json!({
            "finding": format!("guild cannot be run from the PATH: {error}"),
            "fix": "put the directory holding guild on the PATH (command -v guild finds it), then run review-approve doctor again",
        })],
    };
    answer(&json!({"findings": findings}), 0)
}

fn fault_json(fault: &Fault) -> Value {
    json!({"code": fault.code, "message": fault.message, "fix": fault.fix})
}

fn answer(value: &Value, code: u8) -> ExitCode {
    println!("{value}");
    ExitCode::from(code)
}

#[cfg(test)]
mod tests {
    /// The manual names every command, topic and placeholder it uses.
    #[test]
    fn the_manual_is_whole() {
        agent_help::check(&super::help()).unwrap();
    }
}
