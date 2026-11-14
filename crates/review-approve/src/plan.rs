//! What review-approve decides, with no I/O: the request it was given, the
//! guild command lines it runs, in which order, and where it stops.
//!
//! The order is a type. A [`Run`] hands out one [`Step`] at a time and moves
//! past it only on the outcome of that step; an approve step is handed out
//! only once the review step's outcome was a success, so no pull is approved
//! whose review did not pass, and the first failure ends the run.

use crate::refusal::{Fault, Refusal};

/// The tool's own name, as a fix line writes it.
pub const NAME: &str = "review-approve";

/// One pull request to approve: the repo as `guild pull approve` takes it,
/// and its number, from 1.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pull {
    repo: String,
    number: u64,
}

impl Pull {
    /// Reads `<repo>#<number>`, such as `guild/forge#6`: the repo is passed
    /// to guild as written, the number is a whole number from 1.
    pub fn parse(text: &str) -> Result<Pull, Fault> {
        let bad = || Refusal::BadPull.fault(text);
        let (repo, number) = text.rsplit_once('#').ok_or_else(bad)?;
        if repo.is_empty() || repo.chars().any(char::is_whitespace) {
            return Err(bad());
        }
        if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
            return Err(bad());
        }
        match number.parse::<u64>() {
            Ok(n) if n >= 1 => Ok(Pull {
                repo: repo.to_string(),
                number: n,
            }),
            _ => Err(bad()),
        }
    }

    pub fn repo(&self) -> &str {
        &self.repo
    }

    pub fn number(&self) -> u64 {
        self.number
    }

    /// `<repo>#<number>`, as it was asked for.
    pub fn label(&self) -> String {
        format!("{}#{}", self.repo, self.number)
    }

    /// The arguments of `guild pull approve` for this pull.
    pub fn approve_args(&self) -> Vec<String> {
        vec![
            "pull".into(),
            "approve".into(),
            self.repo.clone(),
            self.number.to_string(),
        ]
    }
}

/// A request whose every part was read and found whole: the task, the
/// review's evidence and at least one pull, each pull once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    task: String,
    evidence: String,
    pulls: Vec<Pull>,
}

impl Request {
    /// Reads the request, naming every fault at once.
    pub fn read(task: &str, evidence: &str, pulls: &[String]) -> Result<Request, Vec<Fault>> {
        let mut faults = Vec::new();
        if task.trim().is_empty() || task.chars().any(char::is_whitespace) {
            faults.push(Refusal::BadTask.fault(task));
        }
        if evidence.trim().is_empty() {
            faults.push(Refusal::NoEvidence.fault(task));
        }
        if pulls.is_empty() {
            faults.push(Refusal::NoPull.fault(task));
        }
        let mut read: Vec<Pull> = Vec::new();
        for text in pulls {
            match Pull::parse(text) {
                Ok(pull) if read.contains(&pull) => {
                    faults.push(Refusal::PullTwice.fault(&pull.label()))
                }
                Ok(pull) => read.push(pull),
                Err(fault) => faults.push(fault),
            }
        }
        if faults.is_empty() {
            Ok(Request {
                task: task.to_string(),
                evidence: evidence.to_string(),
                pulls: read,
            })
        } else {
            Err(faults)
        }
    }

    pub fn task(&self) -> &str {
        &self.task
    }

    pub fn evidence(&self) -> &str {
        &self.evidence
    }

    pub fn pulls(&self) -> &[Pull] {
        &self.pulls
    }

    /// The arguments of `guild task review <task> --pass --evidence <text>`.
    pub fn review_args(&self) -> Vec<String> {
        vec![
            "task".into(),
            "review".into(),
            self.task.clone(),
            "--pass".into(),
            "--evidence".into(),
            self.evidence.clone(),
        ]
    }

    /// Every step, in the order a run takes them: the review, then each
    /// pull's approve in the order given.
    pub fn steps(&self) -> Vec<Step<'_>> {
        let mut steps = vec![Step::Review(self)];
        steps.extend(self.pulls.iter().map(Step::Approve));
        steps
    }

    /// A run of this request, before its first step.
    pub fn run(&self) -> Run<'_> {
        Run {
            request: self,
            passed: Passed::No,
            approved: Vec::new(),
            rest: &self.pulls,
        }
    }
}

/// One guild command the run takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step<'a> {
    /// `guild task review <task> --pass --evidence <text>`.
    Review(&'a Request),
    /// `guild pull approve <repo> <number>`.
    Approve(&'a Pull),
}

impl Step<'_> {
    /// The step's name in an answer: `review` or `approve <repo>#<number>`.
    pub fn label(&self) -> String {
        match self {
            Step::Review(_) => "review".into(),
            Step::Approve(pull) => format!("approve {}", pull.label()),
        }
    }

    /// The arguments it gives guild.
    pub fn args(&self) -> Vec<String> {
        match self {
            Step::Review(request) => request.review_args(),
            Step::Approve(pull) => pull.approve_args(),
        }
    }
}

/// How one step ended, as the effect crate saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// guild exited 0.
    Succeeded,
    /// guild ran and exited non-zero (`None`: stopped by a signal).
    Failed { exit: Option<i32> },
    /// guild could not be started at all.
    NotStarted { why: String },
}

/// Whether the review passed: what lets a run hand out an approve step.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Passed {
    No,
    Yes,
}

/// A run part way: the review not yet passed, or passed and some pulls
/// approved. Only [`Run::record`] moves it on: it takes no index and does no
/// arithmetic, so it holds what was approved and what is left, every pull in
/// one of the two, by its making alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run<'a> {
    request: &'a Request,
    passed: Passed,
    approved: Vec<Pull>,
    rest: &'a [Pull],
}

/// Where a run stopped: the step that failed and the fault that says so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stopped {
    /// The step's name, as [`Step::label`] writes it.
    pub step: String,
    pub fault: Fault,
    /// The pulls approved before it failed.
    pub approved: Vec<Pull>,
    /// The pulls not approved: the failed one, if an approve failed, and
    /// every one after it.
    pub not_approved: Vec<Pull>,
}

impl<'a> Run<'a> {
    /// The next step to take, or `None` when every step succeeded. An
    /// approve step is only ever handed out after the review passed.
    pub fn next(&self) -> Option<Step<'a>> {
        match self.passed {
            Passed::No => Some(Step::Review(self.request)),
            Passed::Yes => self.rest.first().map(Step::Approve),
        }
    }

    /// The pulls approved so far.
    pub fn approved(&self) -> &[Pull] {
        &self.approved
    }

    /// Whether the review passed.
    pub fn reviewed(&self) -> bool {
        self.passed == Passed::Yes
    }

    /// Takes the outcome of the step [`Run::next`] handed out: on success
    /// the run moves past it, on any failure it stops there.
    pub fn record(self, outcome: &Outcome) -> Result<Run<'a>, Box<Stopped>> {
        let Some(step) = self.next() else {
            return Ok(self);
        };
        if *outcome == Outcome::Succeeded {
            return Ok(self.past(step));
        }
        let refusal = match step {
            Step::Review(_) => Refusal::ReviewFailed,
            Step::Approve(_) => Refusal::ApproveFailed,
        };
        Err(Box::new(Stopped {
            step: step.label(),
            fault: refusal.step_fault(self.request, outcome, self.rest),
            approved: self.approved,
            not_approved: self.rest.to_vec(),
        }))
    }

    /// The run past a step that succeeded.
    fn past(self, step: Step<'a>) -> Run<'a> {
        match (step, self.rest.split_first()) {
            (Step::Approve(_), Some((pull, rest))) => {
                let mut approved = self.approved;
                approved.push(pull.clone());
                Run {
                    approved,
                    rest,
                    ..self
                }
            }
            _ => Run {
                passed: Passed::Yes,
                ..self
            },
        }
    }
}

/// A command line as a shell reads it: each argument quoted where it needs
/// to be, with single quotes.
pub fn shell_line(program: &str, args: &[String]) -> String {
    let mut line = program.to_string();
    for arg in args {
        line.push(' ');
        line.push_str(&quoted(arg));
    }
    line
}

fn quoted(arg: &str) -> String {
    let plain = !arg.is_empty()
        && arg
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_./#:=@%+,".contains(&b));
    if plain {
        arg.to_string()
    } else {
        format!("'{}'", arg.replace('\'', r"'\''"))
    }
}
