//! hand-in: what it decides, without touching the world. The arguments it
//! takes, the guild calls they make, whether a lesson may be sent, and
//! what each part prints, recorded or refused with its fix. `world` reads
//! stdin, runs guild and prints.

pub mod world;

pub const NAME: &str = "hand-in";

/// Exit code when any part was refused, by hand-in or by guild.
pub const EXIT_REFUSED: u8 = 1;

/// Exit code of a usage error: nothing was run.
pub const EXIT_USAGE: u8 = 2;

/// The most lines a lesson may hold.
pub const MAX_LESSON_LINES: usize = 3;

/// The program every part calls.
pub const GUILD: &str = "guild";

pub const USAGE: &str = "usage: hand-in <task> [--check <item> --evidence <text>]... [--role <role> --lesson <text>|- [--failed] [--external <kind>]...] [--used <file>#<id>]...";

/// What `--help` answers, before its constants are put in by `help`.
const HELP: &str = concat!(
    "usage: hand-in <task> [--check <item> --evidence <text>]... [--role <role> --lesson <text>|- [--failed] [--external <kind>]...] [--used <file>#<id>]...\n",
    "       hand-in --version\n\n",
    "Ends a task in one call instead of guild task check, then printf '…' | guild lesson, then guild task used and a look to see each took. Every part is optional; name at least one. Example:\n",
    "  hand-in 313 --check tests --evidence 'cargo test -p hand-in: test result: ok. 9 passed; 0 failed' \\\n",
    "    --role rust-cli-coder --lesson 'A fake guild on PATH is a shell script the test writes.' \\\n",
    "    --used rust-cli-coder/core.md#read-first --used rust-cli-coder/core.md#test-the-binary\n\n",
    "<task> is passed to guild as given, a number (313) or a reference (guild/tasks#313).\n\n",
    "The parts, run in this order, each one guild call:\n",
    "  --check <item> --evidence <text>   guild task check <task> <item> --evidence <text>. Repeat the pair for each item; each runs on its own.\n",
    "  --lesson <text> | --lesson -       guild lesson --role <role> --task <task> [--failed] [--external <kind>]..., the lesson on its stdin. '-' reads the lesson from hand-in's stdin. --role is needed with it; --failed and each --external <kind> go with it and only with it.\n",
    "                                     A lesson of more than {MAX} lines, or an empty one, is refused before anything is sent. Trailing newlines are not lines.\n",
    "  --used <file>#<id>                 every --used, in the order given, in one guild task used <task> <file>#<id>... call.\n\n",
    "What it prints, on stdout, per part in the order run, a blank line between parts:\n",
    "  == check <item>: recorded          then what guild printed, its stdout then its stderr\n",
    "  == lesson: refused                 then why, and the line that fixes it\n",
    "  == used: recorded\n",
    "A part guild refused (a non-zero exit) is 'refused', with guild's own words and their fix. A refused part never stops the others: every part named is run, and each says what became of it.\n\n",
    "options:\n",
    "  -h, --help      print this help\n",
    "  -V, --version   print hand-in's version\n\n",
    "Exits 0 when every part was recorded. Exits {EXIT_REFUSED} when any part was refused, after running and printing all the others. Exits {EXIT_USAGE}, with the usage on stderr and nothing run, when the arguments are wrong: no task, no part, --evidence without --check, --role or --failed or --external without --lesson, --lesson without --role, or an option it does not take.\n",
);

/// What `--help` answers, its constants as they are.
pub fn help() -> String {
    HELP.replace("{MAX}", &MAX_LESSON_LINES.to_string())
        .replace("{EXIT_REFUSED}", &EXIT_REFUSED.to_string())
        .replace("{EXIT_USAGE}", &EXIT_USAGE.to_string())
}

/// Where the lesson comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LessonText {
    /// Given on the command line.
    Given(String),
    /// Read from hand-in's stdin (`--lesson -`).
    Stdin,
}

/// The lesson part as asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Lesson {
    pub role: String,
    pub text: LessonText,
    pub failed: bool,
    pub external: Vec<String>,
}

/// Every part the arguments ask for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parts {
    pub task: String,
    /// Each (item, evidence), in the order given.
    pub checks: Vec<(String, String)>,
    pub lesson: Option<Lesson>,
    pub used: Vec<String>,
}

/// What the arguments ask.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked {
    Help,
    Version,
    HandIn(Parts),
}

/// The value after option `flag`, or what fixes its absence.
fn value<'a>(it: &mut std::slice::Iter<'a, String>, flag: &str) -> Result<&'a String, String> {
    it.next()
        .ok_or_else(|| format!("{flag} needs a value: {flag} <{}>", placeholder(flag)))
}

fn placeholder(flag: &str) -> &'static str {
    match flag {
        "--check" => "item",
        "--evidence" | "--lesson" => "text",
        "--role" => "role",
        "--external" => "kind",
        "--used" => "file>#<id",
        _ => "value",
    }
}

/// The arguments, after the program's name.
pub fn parse(args: &[String]) -> Result<Asked, String> {
    match args.first().map(String::as_str) {
        None => return Err("name the task and at least one part".into()),
        Some("-h" | "--help") => return Ok(Asked::Help),
        Some("-V" | "--version") if args.len() == 1 => return Ok(Asked::Version),
        _ => {}
    }
    let mut task: Option<String> = None;
    let mut checks = Vec::new();
    let mut pending: Option<String> = None;
    let mut role = None;
    let mut text = None;
    let mut failed = false;
    let mut external = Vec::new();
    let mut used = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-h" | "--help" => return Ok(Asked::Help),
            "--check" => {
                if let Some(item) = pending.take() {
                    return Err(format!(
                        "--check {item} has no --evidence; give --check {item} --evidence <text>"
                    ));
                }
                pending = Some(value(&mut it, a)?.clone());
            }
            "--evidence" => {
                let ev = value(&mut it, a)?.clone();
                match pending.take() {
                    Some(item) => checks.push((item, ev)),
                    None => {
                        return Err(
                            "--evidence must follow its --check <item>: --check <item> --evidence <text>".into(),
                        )
                    }
                }
            }
            "--role" => role = Some(value(&mut it, a)?.clone()),
            "--lesson" => {
                let v = value(&mut it, a)?;
                text = Some(if v == "-" {
                    LessonText::Stdin
                } else {
                    LessonText::Given(v.clone())
                });
            }
            "--failed" => failed = true,
            "--external" => external.push(value(&mut it, a)?.clone()),
            "--used" => used.push(value(&mut it, a)?.clone()),
            f if f.starts_with('-') && f.len() > 1 => {
                return Err(format!("no such option: {f}"));
            }
            t => {
                if let Some(first) = &task {
                    return Err(format!(
                        "one task only: {first} is the task, {t} is not an option's value; quote a value with spaces"
                    ));
                }
                task = Some(t.to_string());
            }
        }
    }
    if let Some(item) = pending {
        return Err(format!(
            "--check {item} has no --evidence; give --check {item} --evidence <text>"
        ));
    }
    let task = task.ok_or("name the task first: hand-in <task> …")?;
    let lesson = match (text, role) {
        (Some(text), Some(role)) => Some(Lesson {
            role,
            text,
            failed,
            external,
        }),
        (Some(_), None) => return Err("--lesson needs --role <role>".into()),
        (None, _) if failed || !external.is_empty() => {
            return Err("--failed and --external go with a lesson: add --lesson <text>".into())
        }
        (None, Some(_)) => {
            return Err("--role goes with a lesson: add --lesson <text>, or drop --role".into())
        }
        (None, None) => None,
    };
    if checks.is_empty() && lesson.is_none() && used.is_empty() {
        return Err("name at least one part: --check <item> --evidence <text>, --lesson <text>, or --used <file>#<id>".into());
    }
    Ok(Asked::HandIn(Parts {
        task,
        checks,
        lesson,
        used,
    }))
}

/// How many lines a lesson holds: its trailing newlines are not lines.
pub fn lesson_lines(text: &str) -> usize {
    let t = text.trim_end_matches(['\n', '\r']);
    if t.is_empty() {
        0
    } else {
        t.lines().count()
    }
}

/// Why a lesson may not be sent, with the fix; `None` when it may.
pub fn lesson_refusal(text: &str) -> Option<String> {
    match lesson_lines(text) {
        0 => Some("the lesson is empty; nothing was sent. Give its text with --lesson <text>, or hand in no lesson (drop --lesson)".into()),
        n if n > MAX_LESSON_LINES => Some(format!(
            "the lesson is {n} lines, at most {MAX_LESSON_LINES} are taken; nothing was sent. Cut it to {MAX_LESSON_LINES} lines and run hand-in again with only --role and --lesson"
        )),
        _ => None,
    }
}

/// One guild call: what it is called in the output, its arguments, its
/// stdin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    pub label: String,
    pub args: Vec<String>,
    pub stdin: Option<String>,
}

/// One part's plan: a guild call, or refused before anything is sent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Run(Call),
    Refused { label: String, why: String },
}

/// The steps the parts make, in the order they run: each check, the
/// lesson (its text already read), then the used entries in one call.
pub fn plan(parts: &Parts, lesson_text: Option<&str>) -> Vec<Step> {
    let s = |x: &str| x.to_string();
    let mut steps = Vec::new();
    for (item, evidence) in &parts.checks {
        steps.push(Step::Run(Call {
            label: format!("check {item}"),
            args: vec![
                s("task"),
                s("check"),
                parts.task.clone(),
                item.clone(),
                s("--evidence"),
                evidence.clone(),
            ],
            stdin: None,
        }));
    }
    if let (Some(l), Some(text)) = (&parts.lesson, lesson_text) {
        let label = s("lesson");
        match lesson_refusal(text) {
            Some(why) => steps.push(Step::Refused { label, why }),
            None => {
                let mut args = vec![
                    s("lesson"),
                    s("--role"),
                    l.role.clone(),
                    s("--task"),
                    parts.task.clone(),
                ];
                if l.failed {
                    args.push(s("--failed"));
                }
                for k in &l.external {
                    args.push(s("--external"));
                    args.push(k.clone());
                }
                let mut body = text.trim_end_matches(['\n', '\r']).to_string();
                body.push('\n');
                steps.push(Step::Run(Call {
                    label,
                    args,
                    stdin: Some(body),
                }));
            }
        }
    }
    if !parts.used.is_empty() {
        let mut args = vec![s("task"), s("used"), parts.task.clone()];
        args.extend(parts.used.iter().cloned());
        steps.push(Step::Run(Call {
            label: s("used"),
            args,
            stdin: None,
        }));
    }
    steps
}

/// What became of one part.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    pub label: String,
    pub recorded: bool,
    pub body: String,
}

/// What a guild call that ran made of its part: recorded on exit 0,
/// refused otherwise, with guild's stdout then stderr as the body.
pub fn report(call: &Call, code: Option<i32>, stdout: &str, stderr: &str) -> Report {
    let mut body = String::new();
    for s in [stdout, stderr] {
        let s = s.trim_end();
        if !s.is_empty() {
            body.push_str(s);
            body.push('\n');
        }
    }
    let recorded = code == Some(0);
    if !recorded {
        let how = match code {
            Some(c) => format!("guild exited {c}"),
            None => "guild was killed by a signal".to_string(),
        };
        body.push_str(&format!(
            "{how}; fix what it says and run hand-in again with only this part\n"
        ));
    }
    Report {
        label: call.label.clone(),
        recorded,
        body,
    }
}

/// What a guild call that could not start made of its part.
pub fn not_started(call: &Call, err: &str) -> Report {
    Report {
        label: call.label.clone(),
        recorded: false,
        body: format!(
            "cannot run {GUILD}: {err}; put guild's directory on PATH (guild doctor says where it is) and run hand-in again with only this part\n"
        ),
    }
}

/// A part refused before anything was sent.
pub fn refused(label: &str, why: &str) -> Report {
    Report {
        label: label.to_string(),
        recorded: false,
        body: format!("{why}\n"),
    }
}

/// Every part as printed, a blank line between parts.
pub fn render(reports: &[Report]) -> String {
    let mut out = String::new();
    for (i, r) in reports.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        let state = if r.recorded { "recorded" } else { "refused" };
        out.push_str(&format!("== {}: {state}\n", r.label));
        out.push_str(&r.body);
    }
    out
}

/// The exit code of a run: refused when any part was.
pub fn exit_code(reports: &[Report]) -> u8 {
    if reports.iter().all(|r| r.recorded) {
        0
    } else {
        EXIT_REFUSED
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn lines_are_counted_without_trailing_newlines() {
        assert_eq!(lesson_lines(""), 0);
        assert_eq!(lesson_lines("\n\n"), 0);
        assert_eq!(lesson_lines("a"), 1);
        assert_eq!(lesson_lines("a\nb\nc\n\n"), 3);
        assert_eq!(lesson_lines("a\n\nb\nc"), 4);
    }

    #[test]
    fn usage_errors_name_their_fix() {
        let e = |a: &[&str]| parse(&v(a)).unwrap_err();
        assert!(e(&["313"]).contains("name at least one part"));
        assert!(e(&["313", "--check", "x"]).contains("--check x --evidence <text>"));
        assert!(e(&["313", "--evidence", "y"]).contains("--evidence must follow"));
        assert!(e(&["313", "--lesson", "x"]).contains("--lesson needs --role"));
        assert!(e(&["313", "--failed", "--used", "a#b"]).contains("go with a lesson"));
        assert!(e(&["313", "--role", "r", "--used", "a#b"]).contains("--role goes with a lesson"));
        assert!(e(&["--used", "a#b"]).contains("name the task first"));
        assert!(e(&["313", "--nope"]).contains("no such option: --nope"));
        assert!(e(&["313", "314", "--used", "a#b"]).contains("one task only"));
    }

    #[test]
    fn help_has_its_constants_put_in() {
        let h = help();
        assert!(h.contains("more than 3 lines"));
        assert!(h.contains("Exits 1 when any part"));
        assert!(!h.contains('{'));
    }
}
