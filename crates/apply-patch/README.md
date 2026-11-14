# apply-patch

Applies one change set to several files in one call, instead of a python3 heredoc of `str.replace` for one file, `cat > f <<EOF` for a new one, and python3 again for the next. You send one patch on stdin; every change is checked against the files before any is written, and it writes all of them or none:

```
*** Begin Patch
*** Add File: new.txt
+each line of the new file, after a '+'
*** Update File: old.txt
<<<<<<< SEARCH
a
=======
b
>>>>>>> REPLACE
*** Delete File: gone.txt
*** End Patch
```

The envelope and headers are codex's apply_patch grammar; an update takes apply-edits' SEARCH/REPLACE blocks, applied by apply-edits' own code (each search text byte for byte, exactly once, or every match under `<<<<<<< SEARCH all`). When any change cannot be made (an add over a file that exists, an update or delete of a missing file, a block that matches zero or several times), no file is written: stderr names every such change by its file and line in the patch, with the fix, and it exits 1. On success stdout shows, per file, what it did and each changed region with line numbers (`<line>: <text>`). A malformed patch exits 2. `--dry-run` prints the same and writes nothing.

```
apply-patch [--dry-run] < patch
```

Run `apply-patch --help` for the exact format, what it prints and how it fails. Released by forge as a tool of the rust-cli-coder, rust-web-coder and trainer:

```
printf '*** Begin Patch\n*** Add File: new.txt\n+hello\n*** Update File: old.txt\n<<<<<<< SEARCH\na\n=======\nb\n>>>>>>> REPLACE\n*** End Patch\n' | guild tool apply-patch
```

Tests, each in a temp dir of its own: `cargo test -p apply-patch` at forge's root.
