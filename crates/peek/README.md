# peek

Does `cat a; sed -n '10,20p' b; ls d` in one call: given any mix of files, line ranges of files and directories, prints per argument, in the order named, a `== <argument>` header, then a file's lines numbered (`<line>: <text>`), only a range's lines with their real numbers, or a directory's entries one per line, sorted, each directory with a trailing `/`. A binary file prints `(binary, N bytes)`; a missing or unreadable path, or a range it cannot print whole, prints an `error:` line with its fix under its header while the others still print, and it exits 2. It only reads.

```
peek <path>[:<from>-<to>]…
```

An argument ending in `:<from>-<to>` (digits, lines counted from 1, both included) is a range: `peek Cargo.toml src/lib.rs:10-20 crates` prints Cargo.toml whole, lines 10 to 20 of src/lib.rs as `10: …` to `20: …`, and the entries of crates. An argument that exists as a path as written is read as that path, even when it looks like a range. A range whose end is past the last line prints the lines there, then an `error:` line naming the range to ask; one starting past the end, at 0, after its end, or asked of a directory prints only its `error:` line.

Run `peek --help` for what it prints and how it fails. Released by forge as a tool of the foreman, rust-cli-coder, rust-web-coder, trainer, warden, flow-keeper and ui-tester: `guild tool peek crates/peek crates/peek/tool.toml:1-2`. Tests, on fixture trees in a temp dir: `cargo test -p peek` at forge's root.
