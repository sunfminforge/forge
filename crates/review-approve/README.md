# review-approve

A forge tool of the warden: its passing review of a task and the approval of
the task's pull requests, in one command, in place of
`guild task review <task> --pass --evidence <text>` followed by one
`guild pull approve <repo> <number>` per pull.

```
review-approve pass <task> --evidence <text> --pull <repo>#<number> [--pull <repo>#<number>]... [--dry-run]
```

It runs the review first and each approve after it, in order, and stops at
the first step that fails, exiting non-zero and naming that step; no pull is
approved unless the review passed. It runs `guild` from the PATH and handles
no token. Everything else is in its help: `review-approve --help`, then
`review-approve pass --help`.

Install: released from forge's main by `guild forge publish`, run as
`guild tool review-approve …`. Built here: `cargo build -p review-approve`
at forge's root.

Tests: `cargo test --manifest-path crates/review-approve/Cargo.toml` runs the
real binary against a fake guild (`tests/fake/guild.rs`, a cargo example put
first on the PATH) and property tests of the plan. Flux:
`crates/review-approve/verify.sh`.

## Departures from the agent-cli way

- One crate, not a pure crate beside an effect crate: forge takes a tool as
  one crate under `crates/<tool>`. The pure part is the modules `plan` and
  `refusal` (no I/O); the effect part is `run`.
- Flux checks `plan` and `refusal` only, through this crate's `verify.sh`,
  which forge's gate does not run. `run` is not checked: in one crate it
  cannot be split out, and Flux stops with an internal error on clap's
  derive. The order of the steps is proved by the type `plan::Run` and by
  the property test `tests/plan.rs`.
- A command line clap cannot read is answered by clap's error on stderr,
  exit 2, not by a JSON object; `review-approve help answers` says so.
- No lock and no idempotence of its own: a step is a guild write. After an
  approve fails, the fault's `fix` names the `guild pull approve` lines left,
  so the review is not run twice.
