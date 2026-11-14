"""A fixture Forgejo for a detector's tests: the few API calls a detector makes on lore,
answered from a local git repository, with Forgejo's paging and sign-in rule."""

import base64
import json
import subprocess
import threading
import urllib.parse
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

TOKEN = "fixture-token"
TREE_PAGE_MAX = 3  # small, so a detector's tests show it reads every page of a tree


def git(repo, *args):
    return subprocess.run(["git", "-C", repo, *args], check=True, capture_output=True).stdout


class Fake:
    def __init__(self, repo, owner_repo="guild/lore"):
        self.repo, self.prefix = repo, f"/api/v1/repos/{owner_repo}/"
        fake = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *a):
                pass

            def do_GET(self):
                fake.handle(self)

        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.url = f"http://127.0.0.1:{self.server.server_address[1]}"
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def close(self):
        self.server.shutdown()
        self.server.server_close()

    def handle(self, h):
        if h.headers.get("Authorization") != f"token {TOKEN}":
            return self.send(h, 401, {"message": "Only signed in user is allowed to call APIs."})
        url = urllib.parse.urlsplit(h.path)
        q = {k: v[-1] for k, v in urllib.parse.parse_qs(url.query).items()}
        if not url.path.startswith(self.prefix):
            return self.send(h, 404, {"message": "not found"})
        rest = url.path[len(self.prefix):]
        try:
            if rest == "commits":
                return self.commits(h, q)
            if rest.startswith("git/trees/"):
                return self.tree(h, rest[len("git/trees/"):], q)
            if rest.startswith("git/blobs/"):
                sha = rest[len("git/blobs/"):]
                raw = git(self.repo, "cat-file", "blob", sha)
                return self.send(h, 200, {"sha": sha, "size": len(raw), "encoding": "base64",
                                          "content": base64.b64encode(raw).decode()})
        except subprocess.CalledProcessError:
            return self.send(h, 404, {"message": "not found"})
        self.send(h, 404, {"message": "not found"})

    def commits(self, h, q):
        out = git(self.repo, "log", "--format=%H%x00%P%x00%cI%x00%B%x01", q.get("sha", "main"))
        all_ = []
        for rec in out.decode().split("\x01"):
            rec = rec.strip("\n")
            if not rec:
                continue
            sha, parents, date, message = rec.split("\x00")
            all_.append({"sha": sha, "created": date,
                         "commit": {"message": message, "committer": {"date": date}},
                         "parents": [{"sha": p} for p in parents.split()]})
        limit, page = int(q.get("limit", 30)), int(q.get("page", 1))
        chunk = all_[(page - 1) * limit: page * limit]
        more = page * limit < len(all_)
        self.send(h, 200, chunk, {"X-Total": str(len(all_)), "X-HasMore": "true" if more else "false"})

    def tree(self, h, ref, q):
        sha = git(self.repo, "rev-parse", ref + "^{tree}").decode().strip()
        entries = []
        for line in git(self.repo, "ls-tree", "-r", "-t", "-l", sha).decode().splitlines():
            meta, path = line.split("\t", 1)
            _, kind, obj, size = meta.split()
            entries.append({"path": path, "type": kind, "sha": obj,
                            "size": 0 if size == "-" else int(size)})
        per = min(int(q.get("per_page", TREE_PAGE_MAX)), TREE_PAGE_MAX)
        page = int(q.get("page", 1))
        chunk = entries[(page - 1) * per: page * per]
        self.send(h, 200, {"sha": sha, "tree": chunk, "truncated": page * per < len(entries),
                           "page": page, "total_count": len(entries)})

    def send(self, h, code, body, headers=None):
        data = json.dumps(body).encode()
        h.send_response(code)
        h.send_header("Content-Type", "application/json")
        h.send_header("Content-Length", str(len(data)))
        for k, v in (headers or {}).items():
            h.send_header(k, v)
        h.end_headers()
        h.wfile.write(data)


class LoreRepo:
    """A lore repository built commit by commit for a test, each commit at a given time."""

    def __init__(self, path):
        self.path = path
        subprocess.run(["git", "init", "-q", "-b", "main", path], check=True)

    def commit(self, files, message, at, branch="main"):
        """files: {path: text, or None to remove}; at: unix time of the commit."""
        import os
        if subprocess.run(["git", "-C", self.path, "rev-parse", "-q", "--verify", "HEAD"],
                          capture_output=True).returncode == 0:
            git(self.path, "checkout", "-q", branch)
        for p, text in files.items():
            full = os.path.join(self.path, p)
            if text is None:
                git(self.path, "rm", "-q", p)
                continue
            os.makedirs(os.path.dirname(full), exist_ok=True)
            with open(full, "w") as f:
                f.write(text)
            git(self.path, "add", p)
        self._commit(message, at)

    def branch(self, name, start="main"):
        git(self.path, "branch", name, start)

    def merge(self, branch, message, at):
        git(self.path, "checkout", "-q", "main")
        env = self._env(at)
        subprocess.run(["git", "-C", self.path, "merge", "-q", "--no-ff", "-m", message, branch],
                       check=True, capture_output=True, env=env)

    def _env(self, at):
        import os
        stamp = f"@{int(at)} +0000"
        return dict(os.environ, GIT_AUTHOR_NAME="fixture", GIT_AUTHOR_EMAIL="fixture@localhost",
                    GIT_COMMITTER_NAME="fixture", GIT_COMMITTER_EMAIL="fixture@localhost",
                    GIT_AUTHOR_DATE=stamp, GIT_COMMITTER_DATE=stamp, GIT_CONFIG_GLOBAL="/dev/null",
                    GIT_CONFIG_NOSYSTEM="1")

    def _commit(self, message, at):
        subprocess.run(["git", "-C", self.path, "commit", "-q", "--allow-empty", "-m", message],
                       check=True, capture_output=True, env=self._env(at))


def parse_findings(out):
    """The [[finding]] tables of a detector's output, or AssertionError if it printed anything else."""
    findings, cur = [], None
    for line in out.splitlines():
        if not line.strip():
            continue
        if line == "[[finding]]":
            cur = {}
            findings.append(cur)
            continue
        assert cur is not None, f"output outside a [[finding]]: {line!r}"
        key, sep, value = line.partition(" = ")
        assert sep and key in ("role", "saw", "evidence") and key not in cur, f"not a finding's key: {line!r}"
        assert value.startswith('"') and value.endswith('"'), f"not a basic string: {line!r}"
        cur[key] = json.loads(value)  # a TOML basic string as written is valid JSON
    for f in findings:
        assert set(f) == {"role", "saw", "evidence"} and all(f.values()), f"incomplete finding: {f}"
    return findings
