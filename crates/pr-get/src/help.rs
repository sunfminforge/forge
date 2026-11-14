//! The command tree and its text: the manual's one home. `pr-get --help`,
//! `pr-get <command> --help`, `pr-get help <topic>` and `pr-get help all`
//! all answer from what is written here, and from the refusals' own doc
//! comments in `decide`.
//!
//! The tree is built with clap's builder, each command's text a constant
//! beside the command it describes: clap's derive expands to code Flux
//! cannot check (docs/decisions/0002-flux-on-the-one-crate.md).

use agent_help::{Help, Topic};
use clap::{Arg, ArgAction, Command};

use crate::decide::{FORGEJO_TOKEN, Refusal};

/// The tool's own text, `--help`'s first part.
const TOOL: &str = r#"One pull request of a Forgejo or GitHub repository, `GET /repos/{owner}/{repo}/pulls/{number}`, printed as JSON, all of it or the fields you name.

`pr-get <OWNER/REPO> <NUMBER>` asks the forge once and prints the pull request as the forge's API gives it, one JSON object: `pr-get guild/forge 1 --field title,state` prints `{"title": "…", "state": "open"}`. It only reads: it writes no file and keeps nothing, so it may be run again and again, and needs no dry run.

It needs `curl` on the PATH, Guild's Forgejo in `GUILD_FORGEJO_URL` (or the API's root in `--api`) and a token that may read the repository in `GUILD_FORGEJO_TOKEN` (or the variable `--token-env` names). The token goes to curl on its stdin, never on a command line, and is never printed. Run `pr-get doctor` first to see that all three are there, and read `pr-get <command> --help` before a command's first run.

## Output

The pull request is one JSON object on stdout, with exit {const:answered}. With `--field`, its keys are the fields as written, in the order given, each once: `--field head.ref` prints `{"head.ref": "guild/tasks-63"}`. A field the pull request holds as `null` is printed `null`.

A refusal is `{"error": "…", "faults": [{"code", "message", "fix"}]}` with exit {const:refused}, naming every fault at once, each with the command, flag or variable that fixes it: see [errors](help:errors). A pull request that does not exist is the refusal `not_found`. Nothing goes to stderr.

The exceptions: `--help`, `-h` and `help` answer Markdown, and `--version` answers one line, `pr-get <version>`.

## Rules to know first

- Forgejo and GitHub answer the same request with some fields their own: see [forges](help:forges).
- A private repository the token cannot read is answered `not_found`, not `unauthorized`.
- A number that starts with `-` is read as a flag, and refused `usage`."#;

/// `pr-get doctor`'s text.
const DOCTOR: &str = r#"Names what keeps `pr-get` from asking the forge here, each with its fix; asks the forge nothing.

## When to run it

Before the first `pr-get <OWNER/REPO> <NUMBER>`, and when one is refused `no_api`, `no_token`, `no_curl` or `unreachable`. It reads what a request would: `--api` or `GUILD_FORGEJO_URL`, the token's variable, and the PATH for `curl`. It sends nothing, so a token the forge would refuse is not found here.

## Example

```sh
pr-get doctor
pr-get doctor --api https://api.github.com --token-env GITHUB_TOKEN
```

## What it answers

`{"pr_get": "0.1.0", "api": "https://forgejo.example/api/v1", "token_env": "GUILD_FORGEJO_TOKEN", "token": true, "curl": "/usr/bin/curl", "findings": []}`

- `pr_get`: this build's version, as `pr-get --version` says it.
- `api`: the API's root a request would ask; `null` when there is none.
- `token_env`: the variable the token is read from.
- `token`: whether that variable holds a token; the token itself is never printed.
- `curl`: the `curl` a request would run; `null` when the PATH has none.
- `findings`: what is wrong, `[]` when nothing is, each `{"finding": "…", "fix": "…"}`.

## What it may refuse

Nothing but a command line it cannot read: a finding is an answer, with exit {const:answered}."#;

/// `pr-get help`'s text.
const HELP: &str = "Shows one topic of this manual, a command's help, or all of it.

`pr-get help errors` shows every refusal and its fix; `pr-get help all` shows the whole manual; `pr-get help <command>` shows what `pr-get <command> --help` does.";

/// `--api`, which `pr-get` and `pr-get doctor` both take.
fn api() -> Arg {
    Arg::new("api")
        .long("api")
        .value_name("URL")
        .help("The API's root, such as `https://api.github.com`; without it, `GUILD_FORGEJO_URL` with `api/v1` below it.")
}

/// `--token-env`, which `pr-get` and `pr-get doctor` both take.
fn token_env() -> Arg {
    Arg::new("token_env")
        .long("token-env")
        .value_name("NAME")
        .default_value(FORGEJO_TOKEN)
        .help("The variable that holds the token.")
}

/// The first line of `text`: a command's line in the index.
fn about(text: &'static str) -> &'static str {
    text.lines().next().unwrap_or(text)
}

/// The command tree.
pub fn command() -> Command {
    Command::new("pr-get")
        .version(env!("PR_GET_VERSION"))
        .about(about(TOOL))
        .long_about(TOOL)
        .disable_help_subcommand(true)
        .args_conflicts_with_subcommands(true)
        .subcommand_negates_reqs(true)
        .arg(
            Arg::new("repo")
                .value_name("OWNER/REPO")
                .required(true)
                .help("The repository, `<owner>/<name>`, such as `guild/forge`."),
        )
        .arg(
            Arg::new("number")
                .value_name("NUMBER")
                .required(true)
                .help("The pull request's number, from 1."),
        )
        .arg(
            Arg::new("field")
                .long("field")
                .value_name("FIELD")
                .value_delimiter(',')
                .action(ArgAction::Append)
                .help("Print only these keys of the pull request, each a key such as `title` or keys joined by `.` into the objects it holds, such as `head.ref` or `user.login`; separated by `,` or given more than once, they add up. Without it, the whole pull request is printed."),
        )
        .arg(api())
        .arg(token_env())
        .subcommand(
            Command::new("doctor")
                .about(about(DOCTOR))
                .long_about(DOCTOR)
                .next_help_heading("Run when something is wrong")
                .arg(api())
                .arg(token_env()),
        )
        .subcommand(
            Command::new("help")
                .about(about(HELP))
                .long_about(HELP)
                .next_help_heading("Run by the agent")
                .arg(Arg::new("topic").help("The topic, `all`, or a command.")),
        )
}

/// Every refusal names each fault by its code, with what fixes it.
///
/// ## Its shape
///
/// `{"error": "<the first fault's message>", "faults": [{"code": "not_found", "message": "guild/forge pull request 9999 not found (HTTP 404)", "fix": "check the repository and the number; …"}]}`, with exit {const:refused}.
///
/// - `error`: what went wrong, in a line: the first fault's message, and how many more there are.
/// - `faults`: every fault, each `{"code", "message", "fix"}`: `code` one of those below, `message` what is wrong, `fix` the command, flag or variable that fixes it.
///
/// The arguments and the environment are read whole before the forge is asked, so one refusal names every fault of them. A refusal changes nothing.
///
/// ## The codes
///
/// {table:refusals}
#[derive(Topic)]
#[topic(name = "errors")]
pub struct Errors;

/// Forgejo and GitHub answer `GET /repos/{owner}/{repo}/pulls/{number}` alike, with some fields their own.
///
/// ## Guild's Forgejo
///
/// With no `--api`, `pr-get` asks `GUILD_FORGEJO_URL` with `api/v1` below it, with the token in `GUILD_FORGEJO_TOKEN`: `pr-get guild/forge 1`.
///
/// ## GitHub
///
/// Pass the API's root and the variable of a GitHub token: `pr-get --api https://api.github.com --token-env GITHUB_TOKEN owner/repo 1`.
///
/// ## Fields both have
///
/// `number`, `title`, `body`, `state` (`open` or `closed`), `merged`, `merged_at`, `html_url`, `user.login`, `head.ref`, `head.sha`, `base.ref`, `created_at`, `updated_at`. Forgejo has `mergeable` at once; GitHub may answer it `null` while it is still working it out. Other fields are each forge's own: run `pr-get <OWNER/REPO> <NUMBER>` with no `--field` to see them all.
#[derive(Topic)]
#[topic(name = "forges")]
pub struct Forges;

/// The exit code of an answer.
pub const ANSWERED: u8 = 0;
/// The exit code of a refusal.
pub const REFUSED: u8 = 1;

/// The manual: the command tree, its topics and the refusals' table.
pub fn manual() -> Help {
    Help::new(command())
        .topic::<Errors>()
        .topic::<Forges>()
        .table::<Refusal>("refusals")
        .constant("answered", ANSWERED)
        .constant("refused", REFUSED)
}

#[cfg(test)]
mod tests {
    /// Every command has text and a group, every placeholder and topic link
    /// names something.
    #[test]
    fn the_manual_is_whole() {
        agent_help::check(&super::manual()).unwrap();
    }
}
