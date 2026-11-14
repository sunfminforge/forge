# 3. A member of forge's root workspace

Status: accepted (guild/tasks#242); amends the last consequence of decision 2

## Context

pr-get was built under tools/pr-get as its own Cargo workspace, outside forge's root workspace (`members = ["crates/*"]`). `guild forge check` and `guild forge publish` read the tools of `crates/*` only, so pr-get was never counted, released or put in the tool index.

## Decision

Move it, unchanged in behaviour, CLI and tests, to crates/pr-get, a member of the root workspace. What only a workspace root can hold goes:

- its own `[workspace]` table: the root's workspace holds it;
- its own `Cargo.lock`: its pinned versions are in the root `Cargo.lock`;
- its `rust-toolchain.toml` (stable, with clippy and rustfmt): rustup's default toolchain builds it, as it builds every other crate of forge.

Its `[package.metadata.flux]`, `[lints.rust]` and `build.rs` stay in its `Cargo.toml`, where a member crate keeps them. Its `gate.sh` and `verify.sh` stay beside it and name the package (`-p pr-get`), since cargo run in a member crate acts on the whole workspace otherwise.

## Consequences

- `cargo test -p pr-get` from forge's root runs its tests; `sh crates/pr-get/gate.sh` still runs fmt, Flux (`0 trusted; 0 ignored`), clippy `-D warnings` and the tests.
- Its `serde_json` asks for `preserve_order`, which its `--field` order rests on ("in the order given"). Cargo unifies features across the packages of one run, so a build or test of several members together (`cargo test --workspace`) gives their `serde_json` that feature too: forge-pull's `open --dry-run` then prints its body's keys in insertion order, and its test `open_dry_run_sends_nothing`, which expects them sorted, fails. Each package built alone (`-p <name>`) is unchanged. forge-pull was outside guild/tasks#242's scope and is left to a follow-up.
