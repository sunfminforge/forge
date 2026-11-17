# hand-in

Ends a task in one call, instead of `guild task check`, then `printf '…' | guild lesson`, then `guild task used` and a look to see each took. Each part is optional and runs as its own guild call: every `--check <item> --evidence <text>` is a `guild task check`; `--lesson <text>` (or `--lesson -` for stdin) with `--role` is a `guild lesson --role <role> --task <task>`, with `--failed` and each `--external <kind>` as given; every `<file>#<id>` named after `--used`, one or several, goes in one `guild task used`. A lesson over 3 lines, or empty, is refused before anything is sent. A refused part never hides the others: each prints under `== <part>: recorded|refused` what guild said, or why it was refused and the fix, and the exit code is 1 when any part was refused.

```
hand-in <task> [--check <item> --evidence <text>]... [--role <role> --lesson <text>|- [--failed] [--external <kind>]...] [--used <file>#<id>...]...
```

Run `hand-in --help` for each part, what it prints and how it fails. Released by forge as a tool of the flow-keeper, rust-cli-coder, trainer and warden:

```
guild tool hand-in 313 --check tests --evidence 'test result: ok. 11 passed; 0 failed' \
  --role rust-cli-coder --lesson 'One line another instance would work better for knowing.' \
  --used rust-cli-coder/core.md#read-first
```

It calls the guild CLI and reimplements none of it. Tests run the binary against a stand-in guild, a shell script alone on PATH, never the real Hub: `cargo test -p hand-in` at forge's root.

## What was looked at before building

Before building (guild ADR 0013), these were looked at; none was adopted:

- **The guild CLI itself** (`guild` usage, `guild task …`): `guild task check`, `guild lesson` and `guild task used` are three separate commands, and no subcommand runs them together; `guild lesson` alone refuses an empty lesson but takes any number of lines. hand-in calls these three as they are, rather than adding to guild, which is out of this task's scope.
- **forge's own tools** (apply-edits, apply-patch, cargo-failures, forge-comment, forge-pull, forge-read, grep-around, peek, repo-glance, subst, test-filters and the rest of `crates/`): none runs guild's task, lesson or used commands; forge-pull and forge-read talk to Forgejo, not to guild.
- **just** (https://github.com/casey/just) and **go-task** (https://github.com/go-task/task): generic command runners. A recipe could chain the three calls, but each is a new install on every machine, a recipe stops at the first failing command (or, set to go on, does not say per part what took), and neither checks a lesson's line count before sending; the recipe would be as long as the hand sequence it replaces.
- **make** (GNU make, already on the machines): the same as just, with `-k` to go on after a failure, and the same missing per-part report and lesson check.

What the task needs that none of them gives is the lesson refused before sending, every part run whatever became of the others, and one report per part, which is a few hundred lines of Rust with no dependency.
