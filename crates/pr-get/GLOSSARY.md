# Glossary

- **API root**: the URL below which a forge's REST API answers: `https://<host>/api/v1` for Forgejo, `https://api.github.com` for GitHub. `--api` names it; without it, `GUILD_FORGEJO_URL` with `api/v1` below it.
- **Field**: a key of the pull request's JSON object, or keys joined by `.` into the objects it holds, such as `head.ref`.
- **Fault**: one thing wrong with a request, `{"code", "message", "fix"}`; a refusal names every fault at once.
- **Stand-in**: `examples/curl.rs`, the `curl` the tests put first on the PATH; it answers from a JSON state file and records each call, so no test reaches a network.
