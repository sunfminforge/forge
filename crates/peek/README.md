# peek

Does `cat a; ls d; cat b` in one call: given any mix of files and directories, prints per path, in the order named, a `== <path>` header, then a file's lines numbered (`<line>: <text>`) or a directory's entries one per line, sorted, each directory with a trailing `/`. A binary file prints `(binary, N bytes)`; a missing or unreadable path prints an `error:` line with its fix under its header while the others still print, and it exits 2. Line ranges are grep-around's (`<file>:<from>-<to>`), not peek's. It only reads.

```
peek <path>…
```

Run `peek --help` for what it prints and how it fails. Released by forge as a tool of the foreman, rust-cli-coder, trainer and warden: `guild tool peek crates/peek crates/peek/tool.toml`. Tests, on fixture trees in a temp dir: `cargo test -p peek` at forge's root.
