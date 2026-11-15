//! What hand-in does to the world: reads its arguments and, for
//! `--lesson -`, its stdin; asks lib for the steps; runs each guild call
//! in turn, every one whatever became of the others; prints what lib makes
//! of them.

use std::io::{self, Read, Write};
use std::process::{Command, ExitCode, Stdio};

use crate::{
    exit_code, help, not_started, parse, plan, refused, render, report, Asked, Call, LessonText,
    Report, Step, EXIT_USAGE, GUILD, NAME, USAGE,
};

fn usage_error(msg: &str) -> ExitCode {
    eprintln!("{USAGE}");
    eprintln!("{NAME}: error: {msg}; see {NAME} --help");
    ExitCode::from(EXIT_USAGE)
}

/// Runs one guild call to its end, its stdin written and closed.
fn run(call: &Call) -> Report {
    let mut cmd = Command::new(GUILD);
    cmd.args(&call.args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(if call.stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return not_started(call, &e.to_string()),
    };
    if let (Some(text), Some(mut stdin)) = (&call.stdin, child.stdin.take()) {
        // A guild that exits before reading its stdin is judged by its
        // exit code, not by this write.
        let _ = stdin.write_all(text.as_bytes());
    }
    match child.wait_with_output() {
        Ok(o) => report(
            call,
            o.status.code(),
            &String::from_utf8_lossy(&o.stdout),
            &String::from_utf8_lossy(&o.stderr),
        ),
        Err(e) => not_started(call, &e.to_string()),
    }
}

pub fn main() -> ExitCode {
    let mut args = Vec::new();
    for a in std::env::args_os().skip(1) {
        match a.into_string() {
            Ok(s) => args.push(s),
            Err(a) => {
                return usage_error(&format!("argument {} is not UTF-8", a.to_string_lossy()))
            }
        }
    }
    let parts = match parse(&args) {
        Ok(Asked::Help) => {
            print!("{}", help());
            return ExitCode::SUCCESS;
        }
        Ok(Asked::Version) => {
            println!("{NAME} {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Ok(Asked::HandIn(p)) => p,
        Err(e) => return usage_error(&e),
    };
    let mut early = Vec::new();
    let text = match parts.lesson.as_ref().map(|l| &l.text) {
        None => None,
        Some(LessonText::Given(t)) => Some(t.clone()),
        Some(LessonText::Stdin) => {
            let mut s = String::new();
            match io::stdin().read_to_string(&mut s) {
                Ok(_) => Some(s),
                Err(e) => {
                    early.push(refused(
                        "lesson",
                        &format!("cannot read the lesson from stdin: {e}; nothing was sent. Pipe UTF-8 text into hand-in, or give it with --lesson <text>"),
                    ));
                    None
                }
            }
        }
    };
    let mut reports = Vec::new();
    let steps = plan(&parts, text.as_deref());
    let lesson_at = parts.checks.len();
    for (i, step) in steps.iter().enumerate() {
        if i == lesson_at {
            reports.append(&mut early);
        }
        reports.push(match step {
            Step::Run(call) => run(call),
            Step::Refused { label, why } => refused(label, why),
        });
    }
    reports.append(&mut early);
    let mut out = io::stdout().lock();
    if let Err(e) = out
        .write_all(render(&reports).as_bytes())
        .and_then(|_| out.flush())
    {
        eprintln!("{NAME}: error: cannot write stdout: {e}");
        return ExitCode::from(EXIT_USAGE);
    }
    ExitCode::from(exit_code(&reports))
}
