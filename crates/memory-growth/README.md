# memory-growth

A detector of the trainer crew: finds roles whose memory (core.md and skills/) grew faster than the work they did, lore's size per role against the tasks each settled over a window.

```
memory-growth [--days N] [--bytes-per-task B] [--min-bytes M]
```

Reads lore's main through Forgejo (GUILD_FORGEJO_URL, GUILD_FORGEJO_TOKEN; GUILD_LORE_REPO, default guild/lore) and the settlements of L3 (GUILD_L3). Calls no model. Prints only [[finding]]s (role, saw, evidence): a role whose memory grew at least M bytes (2048) and more than B bytes (1024) for each task it settled in the last N days (14). Growth is counted along main's first parents, from the window's opening or from when the role came into lore within it, so a new role's first draft is no growth. The foreman settles no task of its own: it is credited with every settled task that belongs to a goal. Without a token, lore is unread and it prints nothing. Tests: sh crates/memory-growth/test, or cargo test -p memory-growth at forge's root.
