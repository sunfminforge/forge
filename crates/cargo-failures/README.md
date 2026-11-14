# cargo-failures

Runs `cargo build` or `cargo test` once, in the current Rust workspace, and prints only what explains a failure: each compile error (rustc's error with its code, file:line and message; warnings dropped), each failing test with its panic message, and every `test result:` line. It exits with cargo's own exit code.

```
cargo-failures build|test [cargo args…]
```

Run `cargo-failures --help` for what it prints, where it finds cargo and how it fails. Released by forge as a tool of rust-cli-coder and rust-web-coder: `guild tool cargo-failures …`. Tests, on fixture workspaces with the real cargo and on stand-in cargos: `cargo test -p cargo-failures` at forge's root.
