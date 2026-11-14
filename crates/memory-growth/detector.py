#!/usr/bin/env python3
"""memory-growth: a detector of the trainer crew. Finds roles whose memory (core.md,
skills/) grew faster than the work they did: lore size per role, read through Forgejo,
against the tasks each settled, read from L3. Calls no model; prints only [[finding]]s."""

NAME = "memory-growth"

# --- lore through Forgejo, L3, findings (the same in each detector: each is self-contained) ---

import base64
import json
import os
import re
import sqlite3
import sys
import urllib.error
import urllib.parse
import urllib.request


class NoAccess(Exception):
    """No Forgejo token was given (as under `guild detector try`): lore cannot be read."""


def fail(message):
    sys.stderr.write(f"{NAME}: {message}\n")
    sys.exit(1)


class Lore:
    """The lore repo, read through Forgejo's API only."""

    def __init__(self):
        self.base = os.environ.get("GUILD_FORGEJO_URL", "").rstrip("/")
        self.token = os.environ.get("GUILD_FORGEJO_TOKEN", "")
        self.repo = os.environ.get("GUILD_LORE_REPO", "guild/lore")
        self.branch = os.environ.get("GUILD_LORE_BRANCH", "main")
        if not self.base:
            fail("GUILD_FORGEJO_URL is not set")
        # No proxy: a detector reaches nothing beyond the Hub.
        self.opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        self.trees = {}

    def get(self, path, **query):
        url = f"{self.base}/api/v1/repos/{self.repo}/{path}"
        if query:
            url += "?" + urllib.parse.urlencode(query)
        headers = {"Accept": "application/json"}
        if self.token:
            headers["Authorization"] = f"token {self.token}"
        try:
            with self.opener.open(urllib.request.Request(url, headers=headers), timeout=30) as r:
                return json.load(r), r.headers
        except urllib.error.HTTPError as e:
            if e.code in (401, 403) and not self.token:
                raise NoAccess()
            fail(f"Forgejo answered {e.code} for {url}")
        except (urllib.error.URLError, OSError, ValueError) as e:
            fail(f"Forgejo could not be read at {url}: {e}")

    def tree(self, ref):
        """{path: (size, blob sha)} of every file at ref."""
        if ref in self.trees:
            return self.trees[ref]
        files, page = {}, 1
        while True:
            data, _ = self.get(f"git/trees/{ref}", recursive="true", per_page=1000, page=page)
            entries = data.get("tree") or []
            for e in entries:
                if e.get("type") == "blob":
                    files[e["path"]] = (int(e.get("size") or 0), e["sha"])
            if not data.get("truncated") or not entries:
                break
            page += 1
        self.trees[ref] = files
        return files

    def text(self, sha):
        data, _ = self.get(f"git/blobs/{sha}")
        raw = base64.b64decode(data.get("content") or "")
        return raw.decode("utf-8", errors="replace")


def memory(files):
    """{role: {path within the role: (size, sha)}}: each role's core.md and skills/, the role's memory.

    A role is a top-level directory of lore holding a role.toml."""
    roles = {p.split("/")[0] for p in files if p.count("/") == 1 and p.endswith("/role.toml")}
    out = {r: {} for r in roles}
    for path, blob in files.items():
        role, _, rest = path.partition("/")
        if role not in out:
            continue
        if rest == "core.md" or (rest.startswith("skills/") and rest.endswith(".md")):
            out[role][rest] = blob
    return out


ITEM = re.compile(r"^\s*(?:[-*+]|\d+[.)])\s+")


def entries(text):
    """(line number, text) of each entry of a markdown file: a list item with its
    continuation lines, a paragraph, or a fenced block. Headings are no entries."""
    out, cur, start, fenced = [], [], 0, False

    def close():
        nonlocal cur
        if cur:
            out.append((start, "\n".join(cur)))
        cur = []

    for n, line in enumerate(text.splitlines(), 1):
        stripped = line.strip()
        if stripped.startswith("```") or stripped.startswith("~~~"):
            if fenced:
                cur.append(line)
                close()
                fenced = False
            else:
                close()
                fenced, start, cur = True, n, [line]
            continue
        if fenced:
            cur.append(line)
            continue
        if not stripped or stripped.startswith("#"):
            close()
            continue
        if ITEM.match(line) or not cur:
            close()
            start, cur = n, [line]
        else:
            cur.append(line)
    close()
    return out


def l3():
    path = os.environ.get("GUILD_L3", "")
    if not path or not os.path.exists(path):
        fail(f"no L3 at GUILD_L3={path!r}")
    try:
        db = sqlite3.connect(f"file:{path}?mode=ro", uri=True)
        db.execute("SELECT 1 FROM sqlite_master LIMIT 1").fetchall()
        return db
    except sqlite3.Error:
        # The sandbox lets a detector write nothing beside L3: read it as it stands.
        return sqlite3.connect(f"file:{path}?immutable=1", uri=True)


def toml_string(s):
    out = []
    for ch in s:
        if ch == "\\":
            out.append("\\\\")
        elif ch == '"':
            out.append('\\"')
        elif ch == "\n":
            out.append("\\n")
        elif ch == "\t":
            out.append("\\t")
        elif ord(ch) < 0x20 or ord(ch) == 0x7F:
            out.append(f"\\u{ord(ch):04X}")
        else:
            out.append(ch)
    return '"' + "".join(out) + '"'


def emit(findings):
    """Prints each finding as a [[finding]] table: role, saw, evidence. Nothing else goes to stdout."""
    blocks = []
    for f in findings:
        blocks.append(
            "[[finding]]\n"
            f"role = {toml_string(f['role'])}\n"
            f"saw = {toml_string(f['saw'])}\n"
            f"evidence = {toml_string(f['evidence'])}\n"
        )
    sys.stdout.write("\n".join(blocks))

# --- end of the common part ---


# --- the detector ---

import argparse
import time
from datetime import datetime, timezone

MAX_COMMITS = 2000  # of lore's history read back; older than this is outside any window


def when(iso):
    return datetime.fromisoformat(iso.replace("Z", "+00:00")).timestamp()


def day(t):
    return datetime.fromtimestamp(t, timezone.utc).strftime("%Y-%m-%d")


def history(lore):
    """Lore's main as it landed, newest first: [(sha, time, subject)] along first parents."""
    commits, page = {}, 1
    head = None
    while len(commits) < MAX_COMMITS:
        data, headers = lore.get("commits", sha=lore.branch, limit=50, page=page,
                                 stat="false", verification="false", files="false")
        if not data:
            break
        for c in data:
            if head is None:
                head = c["sha"]
            parents = [p["sha"] for p in c.get("parents") or []]
            date = (c.get("commit") or {}).get("committer", {}).get("date") or c.get("created")
            subject = ((c.get("commit") or {}).get("message") or "").splitlines()[:1]
            commits[c["sha"]] = (when(date), parents, subject[0] if subject else "")
        if (headers.get("X-HasMore") or "").lower() != "true":
            break
        page += 1
    chain, sha = [], head
    while sha in commits:
        t, parents, subject = commits[sha]
        chain.append((sha, t, subject))
        sha = parents[0] if parents else None
    return chain


def sizes(lore, sha):
    """{role: {path: size}} of each role's memory at a commit."""
    return {role: {p: size for p, (size, _) in files.items()}
            for role, files in memory(lore.tree(sha)).items()}


def settled(db, since):
    """{role: [task]}: the tasks each role settled work on since a time.

    The foreman settles no task of its own: its work is its goals' tasks, so it is
    credited with every task settled since that belongs to a goal."""
    out, all_tasks = {}, []
    for role, task in db.execute(
        "SELECT role, task FROM settlements WHERE at > ? GROUP BY role, task ORDER BY MIN(at)",
        (int(since),),
    ):
        out.setdefault(role, []).append(task)
        if task not in all_tasks:
            all_tasks.append(task)
    of_goals = set()
    for (record,) in db.execute("SELECT record FROM flow"):
        try:
            r = json.loads(record)
        except ValueError:
            continue
        if r.get("goal") is not None and r.get("task") is not None:
            of_goals.add(str(r["task"]))
    out.setdefault("foreman", [])
    for task in all_tasks:
        if task.rpartition("#")[2] in of_goals and task not in out["foreman"]:
            out["foreman"].append(task)
    return out


def change(before, after):
    """What changed in a role's memory between two states, file by file."""
    out = []
    for p in sorted(set(before) | set(after), key=lambda p: (p != "core.md", p)):
        a, b = before.get(p), after.get(p)
        if a == b:
            continue
        if a is None:
            out.append(f"{p} added ({b} bytes)")
        elif b is None:
            out.append(f"{p} removed ({a} bytes)")
        else:
            out.append(f"{p} {a}→{b} ({b - a:+d})")
    return out


def main():
    ap = argparse.ArgumentParser(prog=NAME, description=(
        "Finds roles whose memory (core.md and skills/) grew faster than the work they did: "
        "more bytes added over the window than --bytes-per-task for each task they settled."))
    ap.add_argument("--days", type=float, default=14, help="the window, in days back from now (14)")
    ap.add_argument("--bytes-per-task", type=int, default=1024,
                    help="growth a settled task may bring before it is too much (1024)")
    ap.add_argument("--min-bytes", type=int, default=2048,
                    help="growth below this is never a finding (2048)")
    args = ap.parse_args()

    now = time.time()
    since = now - args.days * 86400
    lore = Lore()
    db = l3()
    try:
        chain = history(lore)
    except NoAccess:
        return  # no token given: lore unread, nothing seen
    if not chain:
        return
    tasks = settled(db, since)

    window = [c for c in chain if c[1] > since][::-1]       # oldest first
    before = next((c for c in chain if c[1] <= since), None)  # lore as the window opened
    states = []  # (commit, {role: {path: size}}) from the window's opening to now
    if before:
        states.append((before, sizes(lore, before[0])))
    for c in window:
        states.append((c, sizes(lore, c[0])))
    if not states:
        return
    head = states[-1][1]

    findings = []
    for role in sorted(head):
        # From the window's opening, or from when the role first came into lore within it:
        # a new role's first draft is not growth.
        start = next(((c, s[role]) for c, s in states if role in s), None)
        if start is None:
            continue
        (first, base) = start
        grew = sum(head[role].values()) - sum(base.values())
        done = tasks.get(role, [])
        allowed = args.bytes_per_task * len(done)
        if grew < args.min_bytes or grew <= allowed:
            continue
        steps = []
        prev = base
        for c, s in states:
            if c[1] <= first[1] or role not in s:
                continue
            what = change(prev, s[role])
            if what:
                steps.append(f"lore {c[0][:10]} {day(c[1])} \"{c[2]}\": " + "; ".join(what))
            prev = s[role]
        opened = (f"since the window opened {day(since)} ({first[0][:10]})" if before and first is before
                  else f"since it came into lore {day(first[1])} ({first[0][:10]})")
        per = f"{grew // len(done)} bytes per task" if done else "with no task settled"
        findings.append({
            "role": role,
            "saw": (
                f"its memory (core.md and skills/) grew {grew} bytes, {sum(base.values())}→"
                f"{sum(head[role].values())}, over the last {args.days:g} days while it settled "
                f"{len(done)} task{'s' if len(done) != 1 else ''} ({per}; at most "
                f"{args.bytes_per_task} per task): memory growing faster than the work"
            ),
            "evidence": (
                f"growth {opened}:\n" + "\n".join(steps)
                + f"\nL3 settlements of {role} since {day(since)}: "
                + (", ".join(done) if done else "none")
            ),
        })
    emit(findings)


if __name__ == "__main__":
    main()
