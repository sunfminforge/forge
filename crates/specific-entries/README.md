# specific-entries

A detector of the trainer crew: finds entries of a role's memory (core.md and skills/) that name a specific repo, path, issue or URL, where a role should hold only how it does its kind of work.

```
specific-entries
```

Reads lore's main through Forgejo (GUILD_FORGEJO_URL, GUILD_FORGEJO_TOKEN; GUILD_LORE_REPO, default guild/lore). Calls no model. Prints only [[finding]]s (role, saw, evidence), one per role with each entry's file:line and what it names: a URL; an issue (owner/repo#n, #n); a path (rooted, ./ or ~/, with an extension, or three segments deep); an owner/repo in code. Not findings: the role's own skills/<name>.md, Guild's own repos (guild/tasks, guild/lore, guild/forge), and placeholders (<repo>, {name}, *, $VAR). Without a token, lore is unread and it prints nothing. Tests: cargo test -p specific-entries at forge's root.
