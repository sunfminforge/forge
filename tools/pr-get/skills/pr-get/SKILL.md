---
name: pr-get
description: Fetch one pull request of Guild's Forgejo (or GitHub) as JSON, with the fields you name, in one call, instead of curl or gh api by hand.
---

# pr-get

Use `pr-get <OWNER/REPO> <NUMBER>` whenever you need a pull request's title, state, branches, author or any other field: `pr-get guild/forge 1 --field title,state`.

Run `pr-get --help` first: it is the whole manual. Run `pr-get doctor` when a call is refused for its setup.
