# forge-read

Reads Guild's Forgejo in one call, read-only (GET only), as plain text, signed in as the instance's own account from GUILD_FORGEJO_URL and GUILD_FORGEJO_TOKEN: one issue with its number, title, state, labels, author, body and, with `--comments`, every comment; or one line per issue or pull of a repo by label and state.

```
forge-read issue <owner/repo> <n> [--comments]
forge-read list <owner/repo> issues|pulls [--label <name>]... [--state open|closed|all]
```

Run `forge-read --help` for what each prints and how it fails. Released by forge as a tool of steward, warden, trainer and rust-cli-coder: `guild tool forge-read …`. Tests, offline against a stand-in Forgejo: `cargo test -p forge-read` at forge's root.
