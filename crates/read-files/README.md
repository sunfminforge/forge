# read-files

Prints several files in one call, each whole or only a line range of it, under a `==> <path>[:<start>-<end>] <==` header line, each line with its line number: what `cat a; cat b; sed -n '40,80p' c` does to get one's bearings in a crate, without its three calls and with the numbers to cite. A missing file or a range past the end is said on stderr while the others still print, and it exits 1 when any argument failed. It only reads.

```
read-files <path>[:<start>-<end>] …
```

Run `read-files --help` for what it prints and how it fails. Released by forge as a tool of the foreman, rust-cli-coder, rust-web-coder, trainer and ui-tester: `guild tool read-files Cargo.toml src/lib.rs:1-40`. Tests, on files in a temp dir: `cargo test -p read-files` at forge's root.
