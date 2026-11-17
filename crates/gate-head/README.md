# gate-head

A forge tool of the warden: whether `guild gate` is green on exactly the
worktree's HEAD tree, with nothing uncommitted, in one call, in place of
`guild gate`, `guild gate` again (to see the tree a green run is of) and
`git rev-parse HEAD^{tree}`.

```
gate-head [--slow] [--remote <name>]
```

It runs `guild gate` once, passing `--slow` and `--remote <name>` through,
prints the gate's whole output as it comes, then a summary block: the head
commit, the head tree, the tree the gate's output names, `tree: matches HEAD`
or `tree: differs`, and `worktree: clean` or `worktree: dirty`. It exits 0
only when the gate was green on HEAD's tree with the worktree clean.
Everything else, the exit codes and the summary's every line, is in its help:
`gate-head --help`, then `gate-head help summary`.

Install: released from forge's main by `guild forge publish`, run as
`guild tool gate-head …`. Built here: `cargo build -p gate-head` at forge's
root.

Tests: `cargo test -p gate-head` at forge's root runs the real binary in a
temporary git repo against a fake guild (`tests/fake/guild.rs`, a cargo
example put first on the PATH, never the real Hub), and property tests of
`decide`. Flux: `crates/gate-head/verify.sh`.

## What was looked at before building

Before building (guild ADR 0013), these were looked at; none was adopted:

- **The guild CLI** (`guild gate [--slow] [--remote <name>]`): it prints the
  tree only on a reused run's line (`gate: reused the green run of … on tree
  <tree> …`); a run made afresh names no tree, and nothing says whether the
  tree is HEAD's or whether the worktree is clean. Changing guild is out of
  this task's scope; gate-head calls it as it is.
- **forge's tools** (the tool index, `guild tool <name> --help`, and
  `crates/`): repo-glance reads branch and status but runs no gate;
  cargo-failures and test-filters run cargo, not the gate; hand-in and
  review-approve call other guild commands. None runs `guild gate` or
  compares a tree with HEAD's.
- **git alone** (`git rev-parse HEAD^{tree}`, `git status --porcelain`,
  `git diff --quiet`): each answers one part, and none runs the gate.
- **pre-commit, lefthook, just and make**: run checks or recipes, but know
  nothing of guild's gate records or the tree a reused green run is of.

## Departures from the agent-cli way

- One crate, not a pure crate beside an effect crate: forge takes a tool as
  one crate under `crates/<tool>`. The pure part is the module `decide` (no
  I/O); the effect part is `run`, with `git` and `guild`, one module per
  program it calls.
- Flux checks `decide` only, through this crate's `verify.sh`, which forge's
  gate does not run, as review-approve's; the rule that exit 0 means green on
  HEAD's tree and clean is `decide::exit_code`'s refinement.
- Its stdout is the gate's own output then the summary block, plain lines,
  not one JSON object, as the task asks: the gate's words are passed on as
  they are. A refusal is `gate-head: error: <cause> (<code>)` then
  `fix: <line>` on stderr. Its help says so.
- A run made afresh names no tree in guild's output: gate-head reads HEAD
  and `git status` before and after the run, and takes the run as of HEAD's
  tree only when the worktree was clean at the same HEAD throughout.
