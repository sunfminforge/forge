#!/usr/bin/env python3
"""near-duplicates: a detector of the trainer crew. Finds entries of the roles' memory
(core.md, skills/) that say nearly the same, within a role or across roles. Reads lore
through Forgejo only; calls no model; prints only [[finding]]s."""

NAME = "near-duplicates"

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

import difflib

STOP = set("""
a about above after again all also am an and any are as at be because been before being below
between both but by can could did do does doing done down during each either else even ever every
few for from further had has have having he her here hers him his how i if in into is it its
itself just let may me might more most must my no nor not now of off on once one only or other
our ours out over own per same shall she should so some such than that the their theirs them then
there these they this those through to too under until up upon us very was we were what when
where which while who whom whose why will with within without would yet you your yours
""".split())

MIN_WORDS = 8         # shorter entries are too small to call duplicates
JACCARD = 0.6         # share of their words two entries have in common
SEQUENCE = 0.75       # or how much of one reads as the other, word for word
CONTAINED = 0.9       # or how much of the shorter one the longer says again,
CONTAINED_WORDS = 10  # when the shorter has at least this many distinct words


def words(entry):
    out = []
    for w in re.findall(r"[a-z0-9]+(?:['’][a-z]+)?", entry.lower()):
        w = re.sub(r"['’][a-z]+$", "", w)
        if w in STOP or len(w) < 2:
            continue
        if len(w) > 4 and w.endswith("ies"):
            w = w[:-3] + "y"
        elif len(w) > 3 and w.endswith("s") and not w.endswith("ss"):
            w = w[:-1]
        out.append(w)
    return out


def similar(a, b):
    """How alike two entries are, from 0 to 1, or None below the bar."""
    sa, sb = set(a), set(b)
    jaccard = len(sa & sb) / len(sa | sb)
    if jaccard >= JACCARD:
        return jaccard
    small = min(len(sa), len(sb))
    contained = len(sa & sb) / small
    if small >= CONTAINED_WORDS and contained >= CONTAINED:
        return contained
    # Cheap bound first: the sequence ratio is at most 2*min/(len a + len b).
    if 2 * min(len(a), len(b)) / (len(a) + len(b)) < SEQUENCE or jaccard < SEQUENCE / 2:
        return None
    seq = difflib.SequenceMatcher(None, a, b, autojunk=False).ratio()
    return seq if seq >= SEQUENCE else None


def clip(entry, n=160):
    one = " ".join(entry.split())
    return one if len(one) <= n else one[: n - 1] + "…"


def main():
    lore = Lore()
    try:
        head = lore.tree(lore.branch)
    except NoAccess:
        return  # no token given: lore unread, nothing seen
    items = []  # (role, path, line, text, words)
    for role, files in sorted(memory(head).items()):
        for path in sorted(files, key=lambda p: (p != "core.md", p)):
            for line, entry in entries(lore.text(files[path][1])):
                if entry.lstrip().startswith(("```", "~~~")):
                    continue  # an example block, not a way of working
                w = words(entry)
                if len(w) >= MIN_WORDS:
                    items.append((role, path, line, entry, w))

    # Group the pairs that are alike, so an entry said three times is one finding.
    parent = list(range(len(items)))

    def find(i):
        while parent[i] != i:
            parent[i] = parent[parent[i]]
            i = parent[i]
        return i

    score = {}
    for i in range(len(items)):
        for j in range(i + 1, len(items)):
            s = similar(items[i][4], items[j][4])
            if s is not None:
                score[(i, j)] = s
                parent[find(j)] = find(i)
    groups = {}
    for i, j in score:
        groups.setdefault(find(i), set()).update((i, j))

    findings = []
    for members in groups.values():
        members = sorted(members)
        roles = sorted({items[i][0] for i in members})
        counts = {r: sum(items[i][0] == r for i in members) for r in roles}
        role = max(roles, key=lambda r: (counts[r], -roles.index(r)))
        where = "within the role" if len(roles) == 1 else "across the roles " + ", ".join(roles)
        pairs = [f"{s:.2f} between {items[i][1]}:{items[i][2]} and {items[j][1]}:{items[j][2]}"
                 if items[i][0] == items[j][0] else
                 f"{s:.2f} between {items[i][0]}/{items[i][1]}:{items[i][2]} and {items[j][0]}/{items[j][1]}:{items[j][2]}"
                 for (i, j), s in sorted(score.items()) if i in members]
        findings.append({
            "role": role,
            "saw": (
                f"{len(members)} entries say nearly the same {where}: "
                + ("merge them into one" if len(roles) == 1 else
                   "lift what they share into one place, a shared skill or the level above, instead of each role keeping its own copy")
            ),
            "evidence": f"lore {lore.repo}@{lore.branch}:\n"
                        + "\n".join(f"{items[i][0]}/{items[i][1]}:{items[i][2]}: \"{clip(items[i][3])}\"" for i in members)
                        + "\nalike (shared words, word order, or the shorter said again in the longer): " + "; ".join(pairs),
        })
    findings.sort(key=lambda f: (f["role"], f["evidence"]))
    emit(findings)


if __name__ == "__main__":
    main()
