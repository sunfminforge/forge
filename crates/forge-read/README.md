# forge-read

Reads Guild's Forgejo in one call, read-only (GET only), as plain text, signed in as the instance's own account from GUILD_FORGEJO_URL and GUILD_FORGEJO_TOKEN: one issue with its number, title, state, labels, author, body and, with `--comments`, every comment; one line per issue of a repo by label and state; or one line per pull of a repo, newest first, read from its pulls endpoint, as `#<number> <state> <head branch> -> <base> <head sha (12)> [<labels>] <title>` (state open, closed or merged), by label, state and `--head` branch.

```
forge-read issue <owner/repo> <n> [--comments]
forge-read list <owner/repo> issues|pulls [--label <name>]... [--state open|closed|all] [--head <branch>]
```

It replaces these Forgejo calls made by hand (curl, a token, then parsing JSON):

- `GET /repos/{o}/{r}/issues/{n}`: `forge-read issue <o>/<r> <n>`
- `GET /repos/{o}/{r}/issues/{n}/comments`, every page: `forge-read issue <o>/<r> <n> --comments`
- `GET /repos/{o}/{r}/pulls`, every page: `forge-read list <o>/<r> pulls [--state …] [--label …] [--head …]`
- the issue search, `GET /repos/{o}/{r}/issues?state=…&labels=…`, every page: `forge-read list <o>/<r> issues [--state …] [--label …]`

Run `forge-read --help` for what each prints and how it fails. Released by forge as a tool of steward, foreman, warden, trainer, rust-cli-coder and rust-web-coder: `guild tool forge-read …`. Tests, offline against a stand-in Forgejo: `cargo test -p forge-read` at forge's root.
