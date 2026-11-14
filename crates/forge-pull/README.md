# forge-pull

Opens a pull request on Guild's Forgejo, or finds the pulls that say they close a task, in one call, as plain text, signed in as the instance's own account from GUILD_FORGEJO_URL and GUILD_FORGEJO_TOKEN (the token only ever in a request header). `open` is its only write, and safe to run again: a pull already open from the head into the base is answered, not opened twice. `find` only reads: every pull of the task owner's repos whose body says `Closes <owner/repo>#<n>`, with its state, head branch, head sha and review state.

```
forge-pull open <owner/repo> --head <branch> [--base main] --title <t> --body-file <f> [--dry-run]
forge-pull find <owner/repo>#<n>
```

Run `forge-pull --help` for what each prints and how it fails. Released by forge as a tool of foreman, rust-cli-coder, rust-web-coder, steward, trainer and warden: `guild tool forge-pull …`. Tests, offline against a stand-in Forgejo: `cargo test -p forge-pull` at forge's root.

## Tools looked at, not adopted

- **tea** (gitea.com/gitea/tea, MIT): `tea pulls create --head --base --title --description-file` opens a pull, but only through a login stored by `tea login add` in its config directory (GITEA_SERVER_URL and GITEA_SERVER_TOKEN feed only that login step); it cannot open a pull with the env token in one call, and would write the token to disk.
- **forgejo-cli** (codeberg.org/forgejo-contrib/forgejo-cli, `fj`, MIT or Apache-2.0): signs in by `fj auth login` (OAuth in a browser) or `fj auth add-token`, kept in its own key store; no env token, so no one-call open either.
- Neither finds a pull by `Closes <task>` across an organization, which is Guild's and built here either way; one Forgejo call (POST /repos/{owner}/{repo}/pulls) does the open, so forge-pull makes it with ureq as forge-read does, rather than adding a Go binary and a stored login.
