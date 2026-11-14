# test-filters

Runs one package's tests once per named filter (one per acceptance item, say) with `cargo test`, and answers one JSON object: per filter how many tests ran, passed, failed and were ignored. A filter that matches no test that runs is `empty`, a failure, never a pass: the call exits 1 when any filter is empty or has a failing test, so an item never looks green with no test behind it.

```
test-filters -p <package> <filter>...
```

Run it at the root of the cargo workspace. Needs cargo on PATH or at `$HOME/.cargo/bin/cargo`; installs nothing. Run `test-filters --help` for every key it answers, its exit codes and its refusals. Released by forge as a tool of warden and rust-cli-coder: `guild tool test-filters …`. Tests, on the fixture workspace in `fixtures/ws` (never on forge itself): `cargo test -p test-filters` at forge's root.
