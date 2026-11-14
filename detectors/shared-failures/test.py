"""Fixture tests of the shared-failures detector, run by ./test.

Each case serves a fixture as Forgejo's API (issues and comments of
guild/tasks, paged as Forgejo pages them) on loopback, runs ./detect as
the scheduler would, with only GUILD_L3 and GUILD_FORGEJO_* set, and
checks what it printed and how it exited.
"""

import importlib.util
import json
import os
import re
import subprocess
import sys
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

HERE = os.path.dirname(os.path.abspath(__file__))
FIXTURES = os.path.join(HERE, "fixtures")


def load(name):
    with open(os.path.join(FIXTURES, name)) as f:
        data = json.load(f)
    comments, n = [], 1000
    for c in data["comments"]:
        if "filler" in c:  # scheduler comments that say no reason, to page past
            for _ in range(c["filler"]):
                n += 1
                comments.append({"id": n, "issue": 10, "created_at": "2026-11-03T20:00:00+09:00",
                                 "body": f"<!--guild:checked id={n}-->\nCheck accepted."})
            continue
        comments.append(c)
    for c in comments:
        c["issue_url"] = f"http://forgejo.test/guild/tasks/issues/{c.pop('issue')}"
    return data["issues"], comments


def serve(issues, comments, status=200):
    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            url = urlparse(self.path)
            q = parse_qs(url.query)
            limit, page = int(q.get("limit", ["30"])[0]), int(q.get("page", ["1"])[0])
            if status != 200:
                items = None
            elif url.path == "/api/v1/repos/guild/tasks/issues/comments":
                items = comments
            elif url.path == "/api/v1/repos/guild/tasks/issues":
                items = issues
            else:
                items = None
            if items is None:
                self.send_response(status if status != 200 else 404)
                self.end_headers()
                self.wfile.write(b'{"message":"no"}')
                return
            body = json.dumps(items[(page - 1) * limit: page * limit]).encode()
            self.send_response(200)
            self.send_header("Content-Type", "application/json")
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *args):
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    return server


def run(url):
    env = {"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "GUILD_L3": "/nonexistent/l3.sqlite",
           "GUILD_FORGEJO_URL": url, "GUILD_FORGEJO_TOKEN": ""}
    p = subprocess.run([os.path.join(HERE, "detect")], env=env, capture_output=True, text=True, timeout=60)
    return p.returncode, p.stdout


FINDING = re.compile(
    r'\[\[finding\]\]\nrole = (".*")\nsaw = (".*")\nevidence = \[\n((?:  ".*",\n)+)\]\n'
)


def parse(out):
    """The findings printed, failing on anything else in the output."""
    found, rest = [], out
    for m in FINDING.finditer(out):
        found.append({"role": json.loads(m.group(1)), "saw": json.loads(m.group(2)),
                      "evidence": [json.loads(l.strip().rstrip(",")) for l in m.group(3).splitlines()]})
        rest = rest.replace(m.group(0), "", 1)
    assert rest.strip() == "", f"output beside findings: {rest!r}"
    return found


failures = []


def case(name):
    def wrap(fn):
        try:
            fn()
            print(f"ok   {name}")
        except AssertionError as e:
            failures.append(name)
            print(f"FAIL {name}: {e}")
        return fn
    return wrap


@case("finds a send-back reason and a failure cause shared by several roles, across pages")
def _():
    server = serve(*load("shared.json"))
    try:
        code, out = run(f"http://127.0.0.1:{server.server_port}")
    finally:
        server.shutdown()
    assert code == 0, f"exited {code}"
    found = parse(out)
    got = sorted((f["role"], f["saw"].split(" reason ")[0]) for f in found)
    assert got == [
        ("rust-cli-coder", "the same failure"), ("rust-cli-coder", "the same send-back"),
        ("trainer", "the same failure"), ("trainer", "the same send-back"),
    ], got
    sendback = [f for f in found if "send-back" in f["saw"]]
    assert sendback[0]["saw"] == sendback[1]["saw"], "one saw for every role of the reason"
    assert "(rust-cli-coder, trainer)" in sendback[0]["saw"], sendback[0]["saw"]
    assert "permission denied" in sendback[0]["saw"], sendback[0]["saw"]
    ev = sendback[0]["evidence"]
    assert len(ev) == 2 and ev[0].startswith("guild/tasks#10 (rust-cli-coder") and ev[1].startswith("guild/tasks#11 (trainer"), ev
    failure = [f for f in found if "failure" in f["saw"]][0]
    assert "cause=start" in failure["saw"], failure["saw"]
    assert [e.split(" ")[0] for e in failure["evidence"]] == ["guild/tasks#12", "guild/tasks#13"], \
        "a label-less issue and a pull request are no task of a role"
    assert not any("web-coder" in f["saw"] or f["role"] == "web-coder" for f in found), "web-coder shares nothing"


@case("stays quiet on reasons one role repeats and reasons no two roles share")
def _():
    server = serve(*load("quiet.json"))
    try:
        code, out = run(f"http://127.0.0.1:{server.server_port}")
    finally:
        server.shutdown()
    assert code == 0, f"exited {code}"
    assert out == "", f"printed {out!r}"


@case("stays quiet, exit 0, when Forgejo refuses it")
def _():
    server = serve([], [], status=403)
    try:
        code, out = run(f"http://127.0.0.1:{server.server_port}")
    finally:
        server.shutdown()
    assert (code, out) == (0, ""), (code, out)


@case("stays quiet, exit 0, when Forgejo is not there")
def _():
    server = serve([], [])
    port = server.server_port
    server.shutdown()
    server.server_close()
    assert run(f"http://127.0.0.1:{port}") == (0, "")


@case("reads a steward's requeue reason as a failure reason")
def _():
    spec = importlib.util.spec_from_file_location("detect", os.path.join(HERE, "detect.py"))
    detect = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(detect)
    said = list(detect.reasons("<!--guild:requeue-->\nNo fault outside the task: it ended without marking its items."))
    assert said == [("failure", "No fault outside the task: it ended without marking its items.", False)], said
    assert list(detect.reasons("<!--guild:checked id=1-->\nCheck accepted.")) == []


if failures:
    print(f"{len(failures)} failed: {', '.join(failures)}")
    sys.exit(1)
print("every fixture test passed")
