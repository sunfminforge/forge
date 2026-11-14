# repo-glance

Orients in a worktree in one call instead of `git status`, `git log` and `cat`: prints under a `== git` header the current repo's branch with its upstream and ahead/behind, `git status --short`, and the last 5 commits one line each, then each named path as peek prints it (a file numbered, a directory listed). Outside a git repo it says so under `== git` and still prints the paths. It only reads: git runs with optional locks off, so not even the index is refreshed.

```
repo-glance [path…]
```

Run `repo-glance --help` for what it prints and how it fails. Released by forge as a tool of the rust-cli-coder and trainer: `guild tool repo-glance README.md crates`. Tests, on fixture repos made with git in a temp dir: `cargo test -p repo-glance` at forge's root.
