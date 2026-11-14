# with-cargo

Runs a command with the cargo bin dir put first on its PATH, and exits with that command's exit code, so `cargo test -p feed-parse` runs from a shell whose PATH lacks `~/.cargo/bin`, without setting PATH by hand. The dir is `$CARGO_HOME/bin` when CARGO_HOME is set, else `$HOME/.cargo/bin`, the first of them that holds an executable `cargo`. Only the PATH of the command it runs changes: no shell profile, no environment of the caller's.

```
with-cargo <command> [args…]
with-cargo --dir
```

Run `with-cargo --help` for where it looks, what it prints and how it fails. Released by forge as a tool of the warden: `guild tool with-cargo cargo test -p feed-parse`. Tests, against a stub cargo in a temp CARGO_HOME or HOME: `cargo test -p with-cargo` at forge's root.
