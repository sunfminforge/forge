# pr-get

One pull request of Guild's Forgejo, or of GitHub, with one call: `GET /repos/{owner}/{repo}/pulls/{number}`, printed as one JSON object, all of it or the fields you name.

```sh
pr-get guild/forge 1 --field title,state
```

It reads `GUILD_FORGEJO_URL` and the token in `GUILD_FORGEJO_TOKEN` from the environment, and asks the forge through `curl` (on every Mac at `/usr/bin/curl`), handing it the token on stdin, never on a command line. Run `pr-get doctor` first, then read `pr-get --help`: it is the whole manual, and a command's own `--help` says the rest.

## Install

From a checkout of guild/forge:

```sh
cargo install --path tools/pr-get
```

## Develop

```sh
cargo test --manifest-path tools/pr-get/Cargo.toml   # offline: curl is a stand-in
sh tools/pr-get/gate.sh                               # fmt, Flux, clippy -D warnings, tests
```

The gate's Flux part needs Homebrew's `sunfmin/tap/flux-toolchain`; `verify.sh` names the `brew` line when it is missing or old. Decisions are in `docs/decisions/`.
