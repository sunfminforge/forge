# tree-grep

Lists a directory's tree and greps it in one call. Without it you need three steps: `ls` for the directory, `ls` again one level down, then `grep -rn`. For each directory given it prints a `== tree <dir>` header and its entries down to `--depth` levels (2 by default), sorted, indented by level, each directory with a trailing `/`. Then, for each file under it that holds a match, it prints a `== <file>` header and each matching line numbered (`<line>: <text>`). With `--context N` it prints grep-around's regions instead. It honours `.gitignore` and skips `target/`, `.git/`, hidden files and binary files, in the tree and the search alike.

```
tree-grep <pattern> [dir…] [--depth N] [--glob <g>]... [--context N] [--fixed]
```

Run `tree-grep --help` for what it prints and how it exits. It is built on ripgrep's library crates, `grep-regex` and `grep-searcher` for matching and `ignore` for the walk, at the versions `Cargo.toml` pins (the same as grep-around's, whose regions it reuses), so it needs nothing installed. Forge releases it as a tool of the foreman, rust-cli-coder, trainer and warden: `guild tool tree-grep 'name = "peek"' crates/peek`. Tests, on fixture trees written to a temp dir: run `cargo test -p tree-grep` at forge's root.
