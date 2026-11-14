"""Run by ./detect, with the developer directory's python3 (see there)."""
"""Detector: one failure, several roles.

The same failure or send-back reason in the tasks of several roles is a
gap in the level above them (a shared skill, a broader role, or the
foreman's way of splitting), not a patch in each.

Reads Forgejo only (the task issues of guild/tasks and their comments),
read-only; calls no model; prints only `[[finding]]`s, one per role whose
tasks share a reason with another role's, all with the same `saw`, so
the trainer crew sees one finding on several roles.

Reasons it reads, each on a task whose `role/<role>` label names its role:
- a send-back: the reason the scheduler quotes in its `sent-back` comment;
- a failure Guild saw the cause of: `<!--guild:failed cause=<cause> ...-->`;
- a failure the steward put back in the queue, with its reason
  (`<!--guild:requeue-->`).

Two reasons are the same when their words, once tokens that only name
this case (hashes, numbers, paths, quoted output) are dropped, overlap at
least SIMILAR (Jaccard); a cause of a failed mark is the same only as the
same cause. Forgejo it cannot read is said on stderr and found nothing.
"""

import json
import os
import re
import sys
import urllib.error
import urllib.request

ORG, REPO = "guild", "tasks"
SIMILAR = 0.5
MIN_WORDS = 4
PAGE = 50

SENT_BACK = "<!--guild:sent-back "
FAILED = "<!--guild:failed "
REQUEUE = "<!--guild:requeue-->"

STOP = set(
    """a an the and or of to in on at by for with from as is are was were be
    been it its this that these those not no but if then than so do does did
    i we you he she they them their our my me us which who what when where
    how there here all any each every one into out up down over under""".split()
)


def api(base, token, path):
    """Every page of a Forgejo list at `path`."""
    out, page = [], 1
    while True:
        sep = "&" if "?" in path else "?"
        url = f"{base}/api/v1{path}{sep}limit={PAGE}&page={page}"
        req = urllib.request.Request(url, headers={"Accept": "application/json"})
        if token:
            req.add_header("Authorization", f"token {token}")
        with urllib.request.urlopen(req, timeout=20) as resp:
            items = json.load(resp)
        out.extend(items)
        if len(items) < PAGE:
            return out
        page += 1


def role_of(issue):
    for label in issue.get("labels") or []:
        name = label.get("name", "")
        if name.startswith("role/"):
            return name[len("role/"):]
    return None


def issue_number(comment):
    url = comment.get("issue_url") or ""
    tail = url.rstrip("/").rsplit("/", 1)[-1]
    return int(tail) if tail.isdigit() else None


def reasons(body):
    """(kind, text, exact) of each reason a comment says, if any."""
    if body.startswith(SENT_BACK):
        quoted = [l[1:].strip() for l in body.splitlines() if l.startswith(">")]
        text = " ".join(quoted).strip()
        if text:
            yield ("send-back", text, False)
    elif body.startswith(FAILED):
        m = re.search(r"\bcause=(\S+?)(?:\s|-->)", body)
        if m:
            yield ("failure", f"cause={m.group(1)}", True)
    elif body.startswith(REQUEUE):
        text = body[len(REQUEUE):].strip()
        if text:
            yield ("failure", text, False)


def words(text):
    text = text.lower()
    text = re.sub(r"`[^`]*`", " ", text)  # code: names this case
    text = re.sub(r"'[^']*'|\"[^\"]*\"", " ", text)  # quoted output
    text = re.sub(r"\S*[/\\#]\S*", " ", text)  # paths, refs
    text = re.sub(r"\b[0-9a-f]{7,40}\b", " ", text)  # hashes
    text = re.sub(r"\d+", " ", text)
    return {w for w in re.findall(r"[a-z][a-z'-]+", text) if w not in STOP and len(w) > 2}


def same(a, b):
    if a["exact"] or b["exact"]:
        return a["exact"] and b["exact"] and a["text"] == b["text"]
    wa, wb = a["words"], b["words"]
    if len(wa) < MIN_WORDS or len(wb) < MIN_WORDS:
        return False
    return len(wa & wb) / len(wa | wb) >= SIMILAR


def clusters(seen):
    parent = list(range(len(seen)))

    def find(i):
        while parent[i] != i:
            parent[i] = parent[parent[i]]
            i = parent[i]
        return i

    for i in range(len(seen)):
        for j in range(i + 1, len(seen)):
            if seen[i]["kind"] == seen[j]["kind"] and same(seen[i], seen[j]):
                parent[find(i)] = find(j)
    groups = {}
    for i in range(len(seen)):
        groups.setdefault(find(i), []).append(seen[i])
    return list(groups.values())


def short(text, n=160):
    text = " ".join(text.split())
    return text if len(text) <= n else text[: n - 1].rstrip() + "…"


def toml_str(s):
    return json.dumps(s, ensure_ascii=False)


def findings(issues, comments):
    roles = {}
    for issue in issues:
        if issue.get("pull_request"):
            continue
        role = role_of(issue)
        if role:
            roles[issue["number"]] = role
    seen = []
    for c in sorted(comments, key=lambda c: (c.get("created_at") or "", c.get("id") or 0)):
        task = issue_number(c)
        role = roles.get(task)
        if role is None:
            continue
        for kind, text, exact in reasons(c.get("body") or ""):
            seen.append({
                "kind": kind, "text": text, "exact": exact, "role": role,
                "task": task, "at": c.get("created_at") or "",
                "words": set() if exact else words(text),
            })
    out = []
    for group in clusters(seen):
        group_roles = sorted({s["role"] for s in group})
        if len(group_roles) < 2:
            continue
        first = group[0]  # earliest: stable as the group grows
        saw = (
            f"the same {first['kind']} reason came back in the tasks of several roles "
            f"({', '.join(group_roles)}): {short(first['text'])}"
        )
        evidence = [
            f"{ORG}/{REPO}#{s['task']} ({s['role']}, {s['kind']} at {s['at']}): {short(s['text'], 240)}"
            for s in group
        ]
        for role in group_roles:
            out.append((role, saw, evidence))
    return out


def render(found):
    blocks = []
    for role, saw, evidence in found:
        blocks.append(
            "[[finding]]\n"
            f"role = {toml_str(role)}\n"
            f"saw = {toml_str(saw)}\n"
            "evidence = [\n" + "".join(f"  {toml_str(e)},\n" for e in evidence) + "]\n"
        )
    return "\n".join(blocks)


def main():
    base = (os.environ.get("GUILD_FORGEJO_URL") or "").rstrip("/")
    token = os.environ.get("GUILD_FORGEJO_TOKEN") or ""
    if not base:
        print("shared-failures: no GUILD_FORGEJO_URL: nothing read", file=sys.stderr)
        return 0
    try:
        issues = api(base, token, f"/repos/{ORG}/{REPO}/issues?state=all&type=issues")
        comments = api(base, token, f"/repos/{ORG}/{REPO}/issues/comments")
    except (urllib.error.URLError, OSError, ValueError) as e:
        print(f"shared-failures: cannot read Forgejo at {base}: {e}: nothing read", file=sys.stderr)
        return 0
    sys.stdout.write(render(findings(issues, comments)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
