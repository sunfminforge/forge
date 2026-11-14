//! Every way review-approve refuses or stops, one variant each: its code is
//! the variant's name, its help the variant's doc comment, and
//! [`Refusal::fault`] writes the line that fixes it.

use crate::plan::{shell_line, Outcome, Pull, Request, NAME};

/// Why review-approve refused, or where it stopped.
#[derive(agent_help::Help, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The task is empty or holds a space: give it as `guild task review` takes it, such as `guild/tasks#59`. Nothing was run.
    BadTask,
    /// `--evidence` is missing or empty: say what the review saw, as `guild task review --pass --evidence` records it. Nothing was run.
    NoEvidence,
    /// No `--pull` was given: name each pull request to approve, `--pull <repo>#<number>`. Nothing was run.
    NoPull,
    /// A `--pull` is not `<repo>#<number>` with a number from 1, such as `--pull guild/forge#6`. Nothing was run.
    BadPull,
    /// The same pull was given twice: give each `--pull` once. Nothing was run.
    PullTwice,
    /// Step 1, `guild task review --pass`, failed or could not start, so no pull was approved. Its stdout and stderr are in the answer's `steps`; put right what they name, then run the same command again.
    ReviewFailed,
    /// A `guild pull approve` failed after the review passed. The pulls before it are approved, it and those after it are not; put right what its output names, then run the `guild pull approve` lines of `fix`, not this tool again, so the task is not reviewed twice.
    ApproveFailed,
}

/// One fault of an answer: its code, what it saw, and the line that fixes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fault {
    pub code: &'static str,
    pub message: String,
    pub fix: String,
}

const EXAMPLE: &str =
    "review-approve pass guild/tasks#59 --evidence '<what the review saw>' --pull guild/forge#6";

impl Refusal {
    /// The code an answer gives it.
    pub fn code(self) -> &'static str {
        agent_help::Table::entry(&self).name
    }

    /// A fault of the request; `what` is the part it is about.
    pub fn fault(self, what: &str) -> Fault {
        let (message, fix) = match self {
            Refusal::BadTask => (
                format!("the task {what:?} is empty or holds a space"),
                format!("name the task as guild task review takes it: {EXAMPLE}"),
            ),
            Refusal::NoEvidence => (
                format!("no --evidence for the review of {what}"),
                format!("add --evidence with what the review saw: {EXAMPLE}"),
            ),
            Refusal::NoPull => (
                format!("no --pull to approve after the review of {what}"),
                format!("add a --pull <repo>#<number> for each pull of the task: {EXAMPLE}"),
            ),
            Refusal::BadPull => (
                format!("--pull {what:?} is not <repo>#<number> with a number from 1"),
                "write it as --pull <repo>#<number>, such as --pull guild/forge#6".into(),
            ),
            Refusal::PullTwice => (
                format!("--pull {what} is given twice"),
                format!("drop the second --pull {what}"),
            ),
            // A step's fault is written by `step_fault`.
            Refusal::ReviewFailed | Refusal::ApproveFailed => {
                (what.to_string(), format!("{NAME} pass --help"))
            }
        };
        Fault {
            code: self.code(),
            message,
            fix,
        }
    }

    /// The fault of a step that did not succeed. `rest` is the pulls not
    /// approved: every one, for a review that failed; the failed one and
    /// those after it, for an approve.
    pub fn step_fault(self, request: &Request, outcome: &Outcome, rest: &[Pull]) -> Fault {
        let how = match outcome {
            Outcome::Succeeded => "succeeded".to_string(),
            Outcome::Failed { exit: Some(code) } => format!("exited {code}"),
            Outcome::Failed { exit: None } => "was stopped by a signal".to_string(),
            Outcome::NotStarted { why } => format!("could not start guild: {why}"),
        };
        let not_started = matches!(outcome, Outcome::NotStarted { .. });
        let (message, fix) = match self {
            Refusal::ApproveFailed => {
                let failed = rest.first().map(Pull::label).unwrap_or_default();
                let lines: Vec<String> = rest
                    .iter()
                    .map(|pull| shell_line("guild", &pull.approve_args()))
                    .collect();
                (
                    format!(
                        "step approve {failed}: guild pull approve {how}; the review of {} passed, and {} pull(s) are not approved",
                        request.task(),
                        rest.len()
                    ),
                    lines.join(" && "),
                )
            }
            _ => {
                let mut args = vec![
                    "pass".to_string(),
                    request.task().to_string(),
                    "--evidence".into(),
                    request.evidence().to_string(),
                ];
                for pull in request.pulls() {
                    args.push("--pull".into());
                    args.push(pull.label());
                }
                let again = shell_line(NAME, &args);
                (
                    format!(
                        "step review: guild task review {} --pass {how}; no pull was approved",
                        request.task()
                    ),
                    if not_started {
                        format!("run {NAME} doctor and do what it says, then: {again}")
                    } else {
                        again
                    },
                )
            }
        };
        Fault {
            code: self.code(),
            message,
            fix,
        }
    }
}
