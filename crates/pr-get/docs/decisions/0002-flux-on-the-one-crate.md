# 2. One crate, Flux on all of it

Status: accepted (guild/tasks#63)

## Context

agent-cli puts decisions in pure crates checked by Flux and effects in one unchecked binary crate. guild/forge holds one crate per tool, and this task asks for one crate under tools/pr-get (moved to crates/pr-get by guild/tasks#242: decision 3).

Flux crashes on what clap's derive generates (an internal error in `update_from_arg_matches_mut` of `#[derive(Subcommand)]`), and refuses `serde_json::json!`, which unwraps inside.

## Decision

One crate, `[package.metadata.flux]` enabled on all of it, `0 trusted; 0 ignored`: its pure part is the module `decide` (no I/O; what it needs is passed in), its effects `lib.rs` and `curl.rs`. The command tree is built with clap's builder, each command's text a constant beside the command it describes rather than a doc comment on a derived type; JSON objects are built with `decide::object`.

## Consequences

- Flux checks the effect code as well as the decisions: an `unwrap` or a bound broken anywhere in the crate fails the gate.
- Help still has one home, `src/help.rs`, and `agent_help::check` still keeps it whole; the refusals' help is the doc comments of `decide::Refusal`.
- tools/pr-get is its own Cargo workspace, since forge's root workspace is `crates/*`, and has its own `gate.sh`.
