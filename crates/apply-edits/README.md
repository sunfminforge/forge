# apply-edits

Edits one file in one call, instead of a python3 heredoc of chained `str.replace`. You send one or more SEARCH/REPLACE blocks on stdin, and it applies every block or none:

```
<<<<<<< SEARCH
the exact old text
=======
the new text
>>>>>>> REPLACE
```

Each search text is matched byte for byte and must match exactly once, unless its block opens with `<<<<<<< SEARCH all`, which replaces every match. Blocks apply in order, and an empty REPLACE deletes the text. When any block matches zero or several times, nothing is written: stderr names each such block, its match count, where its matches (or a line like its first) are, and the fix, and it exits 1. On success the file is written once (a temp file persisted over it) and stdout shows, per block, the changed region with line numbers (`<line>: <text>`), so you need no grep afterwards. A malformed block or an unreadable file exits 2. `--dry-run` prints the same regions and writes nothing. To read the regions before editing, use read-files or peek.

```
apply-edits <file> [--dry-run] < blocks
```

Run `apply-edits --help` for the exact format, what it prints and how it fails. Released by forge as a tool of the rust-cli-coder, rust-web-coder and trainer: `printf '<<<<<<< SEARCH\nb = 2\n=======\nb = 3\n>>>>>>> REPLACE\n' | guild tool apply-edits config.toml`. Tests, on copies of `tests/fixtures` in a temp dir: `cargo test -p apply-edits` at forge's root.

Built on two upstream crates, pinned in forge's `Cargo.lock`: `memchr` 2.8.3 (its `memmem` finds every exact, non-overlapping match) and `tempfile` 3.27.0 (a temp file beside the target, persisted over it, so a write is whole or absent). No tool found did the job itself: `sd`, `fastmod` and `patch` take regexes, ask interactively, or take diffs with fuzz, and atomwrite's apply replaces the first match and silently skips a block that matches nothing.
