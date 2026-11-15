# subst

Applies one or more regex substitutions to several files in one call, instead of `sed -i 's/…/…/g'` once per file, or a python3 heredoc that splices out the text between `s.index(A)` and `s.index(B)`. Each substitution is a regex in Rust's regex syntax and its replacement, with capture groups (`$1`, `${name}`); `(?s)A.*?B` matches from A to B across lines, so one substitution is the anchor splice. Every substitution is checked against every file before any file is written: one that matches nothing in a file, or not the count `-n` requires, writes no file at all and exits 1, naming the substitution, the file and its match count, with the fix. On success each changed file is written once (a tempfile renamed over it; a failure part-way puts back what was done) and stdout shows, per file, each changed region with line numbers (`<line>: <text>`). `--dry-run` prints the same and writes nothing.

```
subst [--dry-run] <regex> <replacement> [-n <count>] <file>...
subst [--dry-run] -e <regex> <replacement> [-n <count>] [-e ...]... <file>...
```

Exact-text edits, with nothing to escape, are apply-edits' (one file) and apply-patch's (several), not subst's.

Run `subst --help` for the regex and replacement syntax, what it prints and how it fails. Released by forge as a tool of the rust-cli-coder and trainer; renaming a call across two files:

```
guild tool subst '\badd\(' 'add_item(' a.rs b.rs
```

Built on the `regex` crate (pinned, the version ripgrep's grep-regex already brings into forge), after looking at sd, fastmod and ruplacer: each applies one pattern, file by file, with no required match count and no all-or-none write across files.

Tests, each in a temp dir of its own: `cargo test -p subst` at forge's root.
