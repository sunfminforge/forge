# grep-around

Finds a pattern and reads around it in one call. Without it you need three steps: `grep` for the files, `grep -n` for the line numbers, then `sed -n a,bp` for the lines around them. It searches the paths recursively, honouring `.gitignore` and skipping `target/`, hidden files and binary files. For each file with a match it prints a `== <file>` header, then each match's region with line numbers (`<line>: <text>`, the matching line marked `>`), with overlapping regions merged and 10 lines of context by default. `grep-around <file>:<from>-<to>` prints that range of one file with line numbers.

```
grep-around <pattern> [path…] [--glob <g>]... [--context N] [--fixed]
grep-around <file>:<from>-<to>
```

Run `grep-around --help` for what it prints and how it exits. It is built on ripgrep's library crates, `grep-regex` and `grep-searcher` for matching and `ignore` for the walk, at the versions `Cargo.toml` pins, so it needs nothing installed. Released by forge as a tool of the foreman, rust-cli-coder, rust-web-coder, trainer and warden: `guild tool grep-around 'fn main' crates --glob '*.rs'`. Tests, on fixture trees written to a temp dir: `cargo test -p grep-around` at forge's root.
