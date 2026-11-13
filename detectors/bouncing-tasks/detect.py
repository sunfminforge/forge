"""Run by ./detect, with the developer directory's python3 (see there)."""
"""Detector: bouncing tasks.

A piece of work passed between narrow roles, task after task, before it
lands: the foreman cuts it so that each task lands on a role too narrow
for it, or the roles are one kind of work split too thin.

A piece of work is a goal's tasks on one repo, in the order they were
first queued, up to and including the first of them that landed (then
the next piece starts). It bounces when its tasks change role at least
HANDOFFS times among at least two roles (a -> b -> a, or a -> b -> c).

Reads L3 only, read-only (`GUILD_L3`): `flow` for each task's goal, repo
and landing; `settlements` and `steps` for the role that worked it (the
warden's reviews aside). Calls no model; prints only `[[finding]]`s, one
per role the work passed through, all with the same `saw`, so the
trainer crew sees one finding on several roles.
"""

import json
import os
import sqlite3
import sys
from collections import Counter

HANDOFFS = 2
NOT_WORKERS = {"warden"}


def read(l3):
    db = sqlite3.connect(f"file:{l3}?mode=ro", uri=True)
    try:
        tasks = {}
        for task, at, record in db.execute("SELECT task, at, record FROM flow ORDER BY at, id"):
            try:
                r = json.loads(record)
            except ValueError:
                continue
            t = tasks.setdefault(task, {"task": task, "first": at, "goal": None, "repo": None, "landed": None})
            if r.get("goal") is not None:
                t["goal"] = r["goal"]
            if r.get("repo"):
                t["repo"] = r["repo"]
            if r.get("phase") == "landed" and t["landed"] is None:
                t["landed"] = r.get("at", at)
        votes = {}
        for table in ("settlements", "steps"):
            for task, role in db.execute(f"SELECT task, role FROM {table}"):
                if role in NOT_WORKERS:
                    continue
                num = str(task).rsplit("#", 1)[-1]
                if num.isdigit():
                    votes.setdefault(int(num), Counter())[role] += 1
    finally:
        db.close()
    for num, t in tasks.items():
        c = votes.get(num)
        t["role"] = sorted(c.items(), key=lambda kv: (-kv[1], kv[0]))[0][0] if c else None
    return tasks


def pieces(tasks):
    """Each piece of work: (goal, repo, its tasks in order, landed?)."""
    by = {}
    for t in tasks.values():
        if t["goal"] is None or not t["repo"] or not t["role"]:
            continue
        by.setdefault((t["goal"], t["repo"]), []).append(t)
    for (goal, repo), ts in sorted(by.items()):
        ts.sort(key=lambda t: (t["first"], t["task"]))
        piece = []
        for t in ts:
            piece.append(t)
            if t["landed"] is not None:
                yield goal, repo, piece, True
                piece = []
        if piece:
            yield goal, repo, piece, False


def findings(tasks):
    out = []
    for goal, repo, piece, landed in pieces(tasks):
        roles = [t["role"] for t in piece]
        handoffs = sum(1 for a, b in zip(roles, roles[1:]) if a != b)
        distinct = sorted(set(roles))
        if handoffs < HANDOFFS or len(distinct) < 2:
            continue
        saw = (
            f"a piece of work passed between narrow roles ({', '.join(distinct)}), "
            f"task after task, before it landed"
        )
        path = " -> ".join(f"#{t['task']} {t['role']}" for t in piece)
        evidence = [
            f"goal guild/tasks#{goal}, repo {repo}: {path}; {handoffs} hand-offs, "
            + ("then it landed" if landed else "not landed yet")
        ]
        for role in distinct:
            out.append((role, saw, evidence))
    # One finding per role and saw: pieces alike join their evidence.
    merged = {}
    for role, saw, evidence in out:
        merged.setdefault((role, saw), []).extend(evidence)
    return [(role, saw, ev) for (role, saw), ev in merged.items()]


def render(found):
    q = lambda s: json.dumps(s, ensure_ascii=False)
    return "\n".join(
        "[[finding]]\n"
        f"role = {q(role)}\n"
        f"saw = {q(saw)}\n"
        "evidence = [\n" + "".join(f"  {q(e)},\n" for e in evidence) + "]\n"
        for role, saw, evidence in found
    )


def main():
    l3 = os.environ.get("GUILD_L3")
    if not l3 or not os.path.exists(l3):
        print(f"bouncing-tasks: no L3 at {l3!r}: nothing read", file=sys.stderr)
        return 0
    try:
        tasks = read(l3)
    except sqlite3.Error as e:
        print(f"bouncing-tasks: cannot read L3 at {l3}: {e}: nothing read", file=sys.stderr)
        return 0
    sys.stdout.write(render(findings(tasks)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
