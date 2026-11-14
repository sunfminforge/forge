//! What specific-entries does to the world: reads lore's main through
//! Forgejo, read-only, and prints.

use std::io::Write;
use std::process::ExitCode;

use crate::lore::{Lore, Unread};
use crate::{finding, in_order, memory, render, NAME};

/// What `--help` answers.
const HELP: &str = concat!(
    "usage: specific-entries\n\n",
    "A detector of the trainer crew: finds entries of a role's memory (core.md and skills/) that name a specific repo, path, issue or URL, where a role should hold only how it does its kind of work.\n\n",
    "Reads lore's main through Forgejo (GUILD_FORGEJO_URL, GUILD_FORGEJO_TOKEN; GUILD_LORE_REPO, default guild/lore). Calls no model. What it finds: a URL; an issue (owner/repo#n, #n); a path (rooted, ./ or ~/, with an extension, or three segments deep); an owner/repo in code. Not findings: the role's own skills/<name>.md, Guild's own repos (guild/tasks, guild/lore, guild/forge), and placeholders (<repo>, {name}, *, $VAR).\n\n",
    "Prints only [[finding]] tables (role, saw, evidence) on stdout, one per role with each entry's file:line and what it names, and exits 0. Without a token, lore is unread and it prints nothing. Exits 1 with the cause on stderr when GUILD_FORGEJO_URL is unset or Forgejo refuses its token. It takes no arguments: any but --help is refused with exit 2.\n",
);

fn fail(message: &str) -> ExitCode {
    eprintln!("{NAME}: {message}");
    ExitCode::from(1)
}

fn detect(lore: &mut Lore) -> Result<String, Unread> {
    let head = lore.tree(&lore.branch.clone())?;
    let at = format!("{}@{}", lore.repo, lore.branch);
    let mut found = Vec::new();
    for (role, files) in memory(&head) {
        let mut texts = Vec::new();
        for path in in_order(&files) {
            texts.push((path.clone(), lore.text(&files[path].1)?));
        }
        found.extend(finding(&role, &texts, &at));
    }
    Ok(render(&found))
}

pub fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{HELP}");
        return ExitCode::SUCCESS;
    }
    if let Some(a) = args.first() {
        eprintln!("{NAME}: takes no arguments, not {a:?}: run it as `{NAME}`");
        return ExitCode::from(2);
    }
    let mut lore = match Lore::from_env() {
        Ok(lore) => lore,
        Err(e) => return fail(&e),
    };
    let out = match detect(&mut lore) {
        Ok(out) => out,
        Err(Unread::NoAccess) => return ExitCode::SUCCESS, // no token given: lore unread, nothing seen
        Err(Unread::Fail(e)) => return fail(&e),
    };
    let mut stdout = std::io::stdout().lock();
    if stdout
        .write_all(out.as_bytes())
        .and_then(|()| stdout.flush())
        .is_err()
    {
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
