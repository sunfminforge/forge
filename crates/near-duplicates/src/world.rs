//! What near-duplicates does to the world: reads lore's main through
//! Forgejo, read-only, and prints.

use std::io::Write;
use std::process::ExitCode;

use crate::lore::{Lore, Unread};
use crate::{findings, in_order, items_of, memory, render, NAME};

/// What `--help` answers.
const HELP: &str = concat!(
    "usage: near-duplicates\n\n",
    "A detector of the trainer crew: finds entries of the roles' memory (core.md and skills/) that say nearly the same, within a role or across roles.\n\n",
    "Reads lore's main through Forgejo (GUILD_FORGEJO_URL, GUILD_FORGEJO_TOKEN; GUILD_LORE_REPO, default guild/lore). Calls no model. An entry is a list item or a paragraph of at least 8 content words; example blocks are none. Two are alike when they share 60% of their words, read 75% the same word for word, or the longer says again 90% of a shorter one of 10 words or more.\n\n",
    "Prints only [[finding]] tables (role, saw, evidence) on stdout, one per group of alike entries, and exits 0: within a role, merge them; across roles, lift what they share to the level above. Without a token, lore is unread and it prints nothing. Exits 1 with the cause on stderr when GUILD_FORGEJO_URL is unset or Forgejo refuses its token. It takes no arguments: any but --help is refused with exit 2.\n",
);

fn fail(message: &str) -> ExitCode {
    eprintln!("{NAME}: {message}");
    ExitCode::from(1)
}

fn detect(lore: &mut Lore) -> Result<String, Unread> {
    let head = lore.tree(&lore.branch.clone())?;
    let mut items = Vec::new();
    for (role, files) in memory(&head) {
        for path in in_order(&files) {
            let text = lore.text(&files[path].1)?;
            items.extend(items_of(&role, path, &text));
        }
    }
    Ok(render(&findings(
        &items,
        &format!("{}@{}", lore.repo, lore.branch),
    )))
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
