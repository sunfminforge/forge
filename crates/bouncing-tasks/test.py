"""Fixture tests of the bouncing-tasks detector, run by ./test.

Each case builds an L3 from fixtures/*.sql in a scratch directory, runs
./detect as the scheduler would, with only GUILD_L3 and GUILD_FORGEJO_*
set, and checks what it printed and how it exited.
"""

import json
import os
import re
import sqlite3
import subprocess
import sys
import tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
# The detector's binary, as forge releases it: ./test or `cargo test` names it.
DETECTOR = os.environ["DETECTOR"]
FIXTURES = os.path.join(HERE, "fixtures")


def l3(scratch, *names):
    path = os.path.join(scratch, "l3.sqlite")
    if os.path.exists(path):
        os.remove(path)
    db = sqlite3.connect(path)
    for name in names:
        with open(os.path.join(FIXTURES, name)) as f:
            db.executescript(f.read())
    db.commit()
    db.close()
    return path


def run(path):
    env = {"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "GUILD_L3": path,
           "GUILD_FORGEJO_URL": "http://127.0.0.1:9", "GUILD_FORGEJO_TOKEN": ""}
    p = subprocess.run([DETECTOR], env=env, capture_output=True, text=True, timeout=60)
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
            with tempfile.TemporaryDirectory() as scratch:
                fn(scratch)
            print(f"ok   {name}")
        except AssertionError as e:
            failures.append(name)
            print(f"FAIL {name}: {e}")
        return fn
    return wrap


@case("finds work passed between roles task after task, landed or not")
def _(scratch):
    code, out = run(l3(scratch, "schema.sql", "quiet.sql", "bounce.sql"))
    assert code == 0, f"exited {code}"
    found = parse(out)
    roles = sorted(f["role"] for f in found)
    assert roles == ["handler-writer", "rust-cli-coder", "schema-writer", "test-writer", "web-coder"], roles
    shop = [f for f in found if f["role"] in ("rust-cli-coder", "web-coder")]
    assert shop[0]["saw"] == shop[1]["saw"], "one saw for every role the work passed through"
    assert "(rust-cli-coder, web-coder)" in shop[0]["saw"], shop[0]["saw"]
    assert shop[0]["evidence"] == [
        "goal guild/tasks#40, repo shop: #41 rust-cli-coder -> #42 web-coder -> #43 rust-cli-coder; "
        "2 hand-offs, then it landed"], shop[0]["evidence"]
    api = [f for f in found if f["role"] == "test-writer"][0]
    assert api["evidence"] == [
        "goal guild/tasks#80, repo api: #81 schema-writer -> #82 handler-writer -> #83 test-writer; "
        "2 hand-offs, not landed yet"], api["evidence"]


@case("stays quiet on one hand-off, one role, a landing between hand-offs and the warden's reviews")
def _(scratch):
    code, out = run(l3(scratch, "schema.sql", "quiet.sql"))
    assert (code, out) == (0, ""), (code, out)


@case("stays quiet on an empty L3")
def _(scratch):
    assert run(l3(scratch, "schema.sql")) == (0, "")


@case("stays quiet, exit 0, with no L3 to read")
def _(scratch):
    assert run(os.path.join(scratch, "missing.sqlite")) == (0, "")


@case("reads L3 without writing it")
def _(scratch):
    path = l3(scratch, "schema.sql", "quiet.sql", "bounce.sql")
    os.chmod(path, 0o444)
    os.chmod(scratch, 0o555)
    try:
        code, out = run(path)
    finally:
        os.chmod(scratch, 0o755)
    assert code == 0 and out.count("[[finding]]") == 5, (code, out)


if failures:
    print(f"{len(failures)} failed: {', '.join(failures)}")
    sys.exit(1)
print("every fixture test passed")
