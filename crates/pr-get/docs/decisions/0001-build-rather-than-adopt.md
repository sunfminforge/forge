# 1. Build pr-get rather than adopt a forge CLI

Status: accepted (guild/tasks#63)

## Context

Trainer and warden fetch a pull request of Guild's Forgejo with one call, the token read from the environment and never written down, its output JSON with fields selectable. Guild's ADR 0013 asks first for an existing tool. Looked at, November 2026:

- **tea** (gitea.com/gitea/tea, Go, MIT): `tea pr <n> --output json` exists in recent releases, but tea reads its server and token from a login it writes to its own config file (`tea login add`), its JSON is tea's own shape rather than the API's, and it selects no fields.
- **fj**, forgejo-cli (codeberg.org/forgejo-contrib/forgejo-cli, Rust, Apache-2.0 OR MIT): `fj pr view` prints text for people, and its token lives in a keys file written by `fj auth`.
- **berg**, codeberg-cli (codeberg.org/Aviac/codeberg-cli, Rust, AGPL-3.0): `berg pull` prints for people, logs in with `berg auth login` storing the token, and its licence would bind a tool shipped with Guild.
- **gh** (`gh pr view --json`, `gh api`, MIT): fields selectable and JSON, but it speaks to GitHub and GitHub Enterprise only; it does not reach a Forgejo.
- **forgejo-api** (crates.io, Rust, Apache-2.0 OR MIT), the SDK fj is built on: an async HTTP client with typed models, a dependency tree far past the few agent-cli allows, models that drop the fields they do not know, and an SDK, where agent-cli reaches another system through a CLI.

Each would need a login written to disk or a wrapper doing what pr-get does, so none was worth adopting.

## Decision

Build pr-get, one crate: it asks the forge through `curl`, the CLI every Mac has, with the request on curl's stdin (`-q --config -`) so the token is never on a command line, and prints the forge's own JSON, whole or the fields named.

## Consequences

- pr-get handles a token, which agent-cli's first rule would leave to another system's CLI: the task requires a token from the environment, and no forge CLI takes one without writing it down. It reads it, checks it holds nothing that could break curl's config, hands it to curl on stdin, and never prints it; its help says so.
- The answer is the forge's JSON, so Forgejo's and GitHub's fields differ: `pr-get help forges` names those both have.
