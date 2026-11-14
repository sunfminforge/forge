# bouncing-tasks

A detector of the trainer crew: finds a piece of work passed between narrow roles, task after task, before it lands: roles too narrow for the foreman's cut, or one kind of work split too thin.

```
bouncing-tasks
```

Reads L3 only (GUILD_L3), read-only: flow for each task's goal, repo and landing; settlements and steps for the role that worked it, the warden's reviews aside. Calls no model. A piece of work is a goal's tasks on one repo in the order they were queued, up to and including the first that landed; it bounces when its tasks change role at least twice among at least two roles. Prints only [[finding]]s (role, saw, evidence), one per role the work passed through, all with the same saw. With no L3 to read it says so on stderr and finds nothing. Tests: cargo test -p bouncing-tasks at forge's root.
