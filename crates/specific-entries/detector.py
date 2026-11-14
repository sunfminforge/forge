#!/usr/bin/env python3
"""specific-entries: a detector of the trainer crew. Finds entries of a role's memory
(core.md, skills/) that name a specific repo, path, issue or URL. Reads lore through
Forgejo only; calls no model; prints only [[finding]]s."""

NAME = "specific-entries"

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

# Guild's own repos are where every role's work is handed and kept, not a case: naming them is general.
GUILD_REPOS = {"guild/tasks", "guild/lore", "guild/forge"}

URL = re.compile(r"\bhttps?://[^\s`)>\]]+")
ISSUE = re.compile(r"(?<![\w/&])(?:([A-Za-z0-9][\w.-]*/[\w.-]+))?#(\d+)\b")
PATHLIKE = re.compile(r"(?<![\w.<{/-])(?:(?:~|\.{1,2})/|/)?(?:[\w.-]+/)*[\w.-]+/?")
CODE = re.compile(r"`([^`\n]+)`")
EXTENSION = re.compile(r"\.[A-Za-z][A-Za-z0-9]{0,7}$")
ROLE_OWN = ("skills/",)


def generic(token):
    """A placeholder or pattern names no specific thing: `<repo>/src`, `skills/{name}.md`, `*.rs`."""
    return any(c in token for c in "<>{}*$")


def around(text, start, end):
    """The whitespace-delimited word the match sits in."""
    s = start
    while s > 0 and not text[s - 1].isspace():
        s -= 1
    e = end
    while e < len(text) and not text[e].isspace():
        e += 1
    return text[s:e]


def hits(entry):
    """[(kind, token)] of what an entry names that is specific: a URL, an issue, a path, a repo."""
    found = []
    taken = []

    def add(kind, token, span):
        taken.append(span)
        if (kind, token) not in found:
            found.append((kind, token))

    def free(span):
        return not any(a < span[1] and span[0] < b for a, b in taken)

    for m in URL.finditer(entry):
        if not generic(m.group(0)):
            add("URL", m.group(0).rstrip(".,;:"), m.span())
    for m in ISSUE.finditer(entry):
        if free(m.span()) and not generic(around(entry, *m.span())):
            add("issue", m.group(0), m.span())
    code_spans = [m.span(1) for m in CODE.finditer(entry)]
    for m in PATHLIKE.finditer(entry):
        if not free(m.span()):
            continue
        token = m.group(0).rstrip(".,;:")
        if "/" not in token:
            continue
        if generic(around(entry, *m.span())):
            continue
        segments = [s for s in token.split("/") if s not in ("", "~", ".", "..")]
        rooted = token.startswith(("/", "~/", "./", "../"))
        if len(segments) < (1 if rooted else 2) or not all(re.search(r"[a-z]", s) for s in segments):
            continue  # I/O, 1/2: no path
        if token.startswith(ROLE_OWN) and token.count("/") == 1:
            continue  # the role's own skill
        in_code = any(a <= m.start() and m.end() <= b for a, b in code_spans)
        if rooted or EXTENSION.search(segments[-1]) or len(segments) >= 3:
            add("path", token, m.span())
        elif in_code and token not in GUILD_REPOS:
            add("repo", token, m.span())
    return found


def main():
    lore = Lore()
    try:
        head = lore.tree(lore.branch)
    except NoAccess:
        return  # no token given: lore unread, nothing seen
    findings = []
    for role, files in sorted(memory(head).items()):
        lines, named = [], 0
        for path in sorted(files, key=lambda p: (p != "core.md", p)):
            for line, entry in entries(lore.text(files[path][1])):
                found = hits(entry)
                named += bool(found)
                for kind, token in found:
                    lines.append(f"{role}/{path}:{line} names the {kind} `{token}`")
        if lines:
            n = named
            findings.append({
                "role": role,
                "saw": (
                    f"{n} {'entry names' if n == 1 else 'entries name'} a specific repo, path or issue: "
                    "a role holds how it does its kind of work, true beyond the case it came from; "
                    "the case belongs to its home"
                ),
                "evidence": f"lore {lore.repo}@{lore.branch}:\n" + "\n".join(lines),
            })
    emit(findings)


if __name__ == "__main__":
    main()
