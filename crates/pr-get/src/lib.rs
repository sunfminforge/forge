//! `pr-get`, the effect: it reads its arguments, the environment and the
//! PATH, asks `decide` what they mean, runs `curl` once, and prints what
//! `decide` makes of the answer. Every decision is `decide`'s; this file
//! only reads, runs and prints.

pub mod curl;
pub mod decide;
mod help;

agent_help::binary!("pr-get");

// Flux's refinements of the standard library; both are empty outside Flux.
extern crate flux_alloc;
extern crate flux_core;

use std::ffi::OsString;
use std::process::ExitCode;

use clap::ArgMatches;
use clap::error::ErrorKind;
use serde_json::Value;

use decide::{Asked, Fault, Number, Refusal, Repo, object};

/// Runs one request and answers it: one JSON object on stdout.
pub fn main(args: impl IntoIterator<Item = OsString>) -> ExitCode {
    let manual = help::manual();
    let command = match manual.clap() {
        Ok(command) => command,
        Err(e) => {
            return refuse(&[Fault::new(
                Refusal::Usage,
                format!("the manual: {e}"),
                "run `pr-get --help`",
            )]);
        }
    };
    let args: Vec<OsString> = args.into_iter().collect();
    let subcommand = args.iter().skip(1).find_map(|arg| {
        let arg = arg.to_str()?;
        command
            .get_subcommands()
            .find(|c| c.get_name() == arg)
            .map(|c| c.get_name().to_owned())
    });
    let matches = match command.try_get_matches_from(&args) {
        Ok(matches) => matches,
        Err(e) if matches!(e.kind(), ErrorKind::DisplayHelp | ErrorKind::DisplayVersion) => {
            print!("{e}");
            return ExitCode::SUCCESS;
        }
        Err(e) => return refuse(&[usage(&e, subcommand.as_deref())]),
    };
    let answer = match matches.subcommand() {
        Some(("help", sub)) => {
            let markdown = match text(sub, "topic").as_deref() {
                None => manual.index(),
                Some("all") => manual.manual(),
                Some(topic) if manual.topic_help(topic).is_ok() => manual.topic_help(topic),
                Some(command) => manual.command_help(&[command]),
            };
            return match markdown {
                Ok(text) => {
                    print!("{text}");
                    ExitCode::SUCCESS
                }
                Err(e) => refuse(&[Fault::new(
                    Refusal::Usage,
                    e.to_string(),
                    "run `pr-get --help` to see the topics and commands there are",
                )]),
            };
        }
        Some((_, sub)) => Ok(doctor(&Forge::of(sub))),
        None => get(
            &text(&matches, "repo").unwrap_or_default(),
            &text(&matches, "number").unwrap_or_default(),
            &texts(&matches, "field"),
            &Forge::of(&matches),
        ),
    };
    match answer {
        Ok(answer) => {
            println!("{answer}");
            ExitCode::from(help::ANSWERED)
        }
        Err(faults) => refuse(&faults),
    }
}

/// Argument `id`'s value, if given.
fn text(matches: &ArgMatches, id: &str) -> Option<String> {
    matches.get_one::<String>(id).cloned()
}

/// Every value of argument `id`, in order.
fn texts(matches: &ArgMatches, id: &str) -> Vec<String> {
    matches
        .get_many::<String>(id)
        .map(|values| values.cloned().collect())
        .unwrap_or_default()
}

/// Which forge to ask, and the variable of its token.
struct Forge {
    api: Option<String>,
    token_env: String,
}

impl Forge {
    fn of(matches: &ArgMatches) -> Forge {
        Forge {
            api: text(matches, "api"),
            token_env: text(matches, "token_env")
                .unwrap_or_else(|| decide::FORGEJO_TOKEN.to_string()),
        }
    }
}

/// What the environment holds of `name`, as text.
fn env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// `pr-get <OWNER/REPO> <NUMBER>`: every fault of the arguments and the
/// environment, or the pull request.
fn get(repo: &str, number: &str, fields: &[String], forge: &Forge) -> Result<Value, Vec<Fault>> {
    let repo = Repo::parse(repo);
    let number = Number::parse(number);
    let fields = decide::fields(fields);
    let api = decide::api_root(forge.api.as_deref(), env(decide::FORGEJO_URL).as_deref());
    let token = decide::token(&forge.token_env, env(&forge.token_env).as_deref());
    let curl = curl::find(env("PATH").as_deref()).ok_or_else(decide::no_curl);
    let (repo, number, fields, api, token, curl) = match (repo, number, fields, api, token, curl) {
        (Ok(r), Ok(n), Ok(f), Ok(a), Ok(t), Ok(c)) => (r, n, f, a, t, c),
        (r, n, f, a, t, c) => {
            let mut faults = Vec::new();
            faults.extend(r.err());
            faults.extend(n.err());
            faults.extend(f.err().unwrap_or_default());
            faults.extend(a.err());
            faults.extend(t.err());
            faults.extend(c.err());
            return Err(faults);
        }
    };
    let url = decide::pull_url(&api, &repo, number);
    let ran = match curl::run(&curl, &curl::config(&url, &token)) {
        Ok(ran) => ran,
        Err(e) => return Err(vec![decide::unreachable(&api, &e.to_string())]),
    };
    if !ran.ok {
        return Err(vec![decide::unreachable(&api, &ran.stderr)]);
    }
    let Some((status, body)) = decide::split_response(&ran.stdout) else {
        return Err(vec![decide::unreachable(
            &api,
            "curl answered no HTTP status",
        )]);
    };
    let asked = Asked {
        api: &api,
        repo: &repo,
        number,
        token_env: &forge.token_env,
    };
    match decide::interpret(status, body, &asked) {
        Ok(pull) => decide::select(pull, &fields),
        Err(fault) => Err(vec![fault]),
    }
}

/// `pr-get doctor`.
fn doctor(forge: &Forge) -> Value {
    let api = decide::api_root(forge.api.as_deref(), env(decide::FORGEJO_URL).as_deref());
    let token = decide::token(&forge.token_env, env(&forge.token_env).as_deref());
    let curl = curl::find(env("PATH").as_deref());
    let findings: Vec<Value> = decide::findings(&api, &token, curl.is_some())
        .into_iter()
        .map(|f| {
            object(vec![
                ("finding", Value::from(f.finding)),
                ("fix", Value::from(f.fix)),
            ])
        })
        .collect();
    let api = match api {
        Ok(root) => Value::from(root),
        Err(_) => Value::Null,
    };
    let curl = match curl {
        Some(path) => Value::from(path.display().to_string()),
        None => Value::Null,
    };
    object(vec![
        ("pr_get", Value::from(env!("PR_GET_VERSION"))),
        ("api", api),
        ("token_env", Value::from(forge.token_env.as_str())),
        ("token", Value::from(token.is_ok())),
        ("curl", curl),
        ("findings", Value::Array(findings)),
    ])
}

/// The refusal of a command line clap could not read.
fn usage(e: &clap::Error, subcommand: Option<&str>) -> Fault {
    let rendered = e.to_string();
    let first = rendered
        .lines()
        .next()
        .unwrap_or("")
        .trim_start_matches("error: ")
        .to_string();
    let command = match subcommand {
        Some(name) => format!("pr-get {name} --help"),
        None => "pr-get --help".to_string(),
    };
    Fault::new(
        Refusal::Usage,
        format!("the command line: {first}"),
        format!("run `{command}` to see how to write it"),
    )
}

/// The refusal: every fault, in one JSON object on stdout.
fn refuse(faults: &[Fault]) -> ExitCode {
    println!("{}", decide::refusal(faults));
    ExitCode::from(help::REFUSED)
}
