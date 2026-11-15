# forge-comment

Says something on an issue of Guild's Forgejo in one call, as plain text, signed in as the instance's own account from GUILD_FORGEJO_URL and GUILD_FORGEJO_TOKEN (the token only ever in a request header). `say` posts a new comment on an issue (POST /repos/{owner}/{repo}/issues/{n}/comments); `correct` replaces the body of a comment the account wrote (PATCH /repos/{owner}/{repo}/issues/comments/{id}), safe to run again. Both print `<comment-id> <url>`; with `--dry-run` they print the request and send nothing.

```
forge-comment say <owner/repo>#<n> (--text <t> | --body-file <f>) [--dry-run]
forge-comment correct <owner/repo> <comment-id> (--text <t> | --body-file <f>) [--dry-run]
```

Run `forge-comment --help` for what each prints and how it fails: exit 1 naming Forgejo's status and the issue or comment, exit 2 naming GUILD_FORGEJO_URL or GUILD_FORGEJO_TOKEN when unset, or on an argument it does not take. Reading an issue and its comments is forge-read's. Released by forge as a tool of foreman: `guild tool forge-comment …`. Tests, offline against a stand-in Forgejo: `cargo test -p forge-comment` at forge's root.

## Tools looked at, not adopted

- **tea** (gitea.com/gitea/tea, MIT): `tea comment <issue> <body>` posts a comment, but only through a login stored by `tea login add` in its config directory; it has no command to edit a comment, and would write the token to disk.
- **forgejo-cli** (codeberg.org/forgejo-contrib/forgejo-cli, `fj`, MIT or Apache-2.0): comments on and edits issues, but signs in by `fj auth login` or `fj auth add-token`, kept in its own key store; no env token, so no one-call comment.
- Each command is one Forgejo call with a `{"body": …}` JSON body, so forge-comment makes it with ureq as forge-pull and forge-read do, rather than adding a Go or second Rust binary and a stored login.
