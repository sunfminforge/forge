"""Fixture tests of memory-growth: it finds a role whose memory grew faster than the tasks it
settled over the window, and stays quiet on growth the work pays for, growth before the window,
and a new role's first draft."""

import json
import os
import sqlite3
import subprocess
import sys
import tempfile
import time
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
# The detector's binary, as forge releases it: ./test or `cargo test` names it.
DETECTOR = os.environ["DETECTOR"]
sys.path.insert(0, os.path.join(HERE, "fixtures"))
from forgejo import TOKEN, Fake, LoreRepo, parse_findings  # noqa: E402

DAY = 86400
NOW = time.time()


def text(n, word="practice"):
    """n bytes of markdown."""
    line = f"- {word} " * 8 + "\n"
    return (line * (n // len(line) + 1))[:n]


def role(name, core=200):
    return {f"{name}/role.toml": f'name = "{name}"\n', f"{name}/core.md": text(core)}


def history(tmp):
    """Lore over 30 days; the detector's window is its default 14."""
    repo = LoreRepo(os.path.join(tmp, "lore"))
    first = {"README.md": "# lore\n"}
    for r in ("grower", "busy", "steady", "foreman"):
        first.update(role(r))
    first["grower/prohibitions.toml"] = "x" * 50
    repo.commit(first, "Lore", NOW - 30 * DAY)
    repo.commit({"steady/skills/old.md": text(5000)}, "Grow steady before the window", NOW - 20 * DAY)
    # In the window:
    repo.commit({"grower/skills/a.md": text(3000)}, "Grow grower", NOW - 10 * DAY)
    repo.commit({"busy/skills/a.md": text(3000)}, "Grow busy", NOW - 10 * DAY + 60)
    repo.commit({"foreman/skills/split.md": text(3000)}, "Grow foreman", NOW - 9 * DAY)
    repo.commit({"grower/prohibitions.toml": "x" * 9000}, "Prohibitions are not memory", NOW - 8 * DAY)
    repo.commit({**role("newbie", core=6000), "newbie/skills/s.md": text(1000)}, "Add newbie", NOW - 5 * DAY)
    repo.commit({"newbie/core.md": text(6100)}, "Touch newbie", NOW - 4 * DAY)
    # A draft on a side branch, landed by a merge: lore as it landed counts it once.
    repo.branch("draft")
    repo.commit({"grower/skills/b.md": text(1000)}, "Draft grower", NOW - 3 * DAY, branch="draft")
    repo.merge("draft", "Change the role grower (#9)", NOW - 2 * DAY)
    return repo


def l3(tmp, goal_tasks=True):
    path = os.path.join(tmp, "l3.sqlite")
    db = sqlite3.connect(path)
    db.executescript("""
        CREATE TABLE settlements (id INTEGER PRIMARY KEY, task TEXT NOT NULL, role TEXT NOT NULL,
            at INTEGER NOT NULL, tokens INTEGER, seconds INTEGER NOT NULL);
        CREATE TABLE flow (id INTEGER PRIMARY KEY, task INTEGER NOT NULL, at INTEGER NOT NULL,
            record TEXT NOT NULL);
    """)
    rows = [("guild/tasks#1", "grower", NOW - 20 * DAY),   # before the window: not counted
            ("guild/tasks#2", "grower", NOW - 6 * DAY),
            ("guild/tasks#2", "grower", NOW - 6 * DAY + 60)]  # the same task twice: one task
    rows += [(f"guild/tasks#{n}", "busy", NOW - n * DAY / 2) for n in range(10, 15)]
    rows += [("guild/tasks#10", "warden", NOW - 4 * DAY)]
    db.executemany("INSERT INTO settlements (task, role, at, seconds) VALUES (?, ?, ?, 60)",
                   [(t, r, int(a)) for t, r, a in rows])
    for n in (2, 10, 11):
        rec = {"task": n, "phase": "landed", "at": int(NOW)}
        if goal_tasks:
            rec["goal"] = 6
        db.execute("INSERT INTO flow (task, at, record) VALUES (?, ?, ?)", (n, int(NOW), json.dumps(rec)))
    db.commit()
    db.close()
    os.chmod(path, 0o444)  # the detector only reads it
    return path


def run(fake, l3_path, token=TOKEN, *args):
    env = {"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "GUILD_FORGEJO_URL": fake.url,
           "GUILD_FORGEJO_TOKEN": token, "GUILD_L3": l3_path, "HOME": tempfile.gettempdir()}
    return subprocess.run([DETECTOR, *args], capture_output=True, text=True, env=env)


class MemoryGrowth(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.mkdtemp(prefix="memory-growth-")
        cls.fake = Fake(history(cls.tmp).path)
        cls.l3 = l3(cls.tmp)
        cls.l3_no_goals = l3(tempfile.mkdtemp(prefix="memory-growth-"), goal_tasks=False)

    @classmethod
    def tearDownClass(cls):
        cls.fake.close()

    def test_finds_memory_growing_faster_than_the_work(self):
        r = run(self.fake, self.l3)
        self.assertEqual(r.returncode, 0, r.stderr)
        findings = parse_findings(r.stdout)
        # busy's growth is paid for by its 5 tasks, steady grew before the window, newbie's
        # first draft is no growth, the foreman is credited with its goals' 3 tasks.
        self.assertEqual([f["role"] for f in findings], ["grower"], r.stdout)
        f = findings[0]
        self.assertIn("grew 4000 bytes, 200→4200, over the last 14 days while it settled 1 task "
                      "(4000 bytes per task; at most 1024 per task)", f["saw"])
        self.assertIn('"Grow grower": skills/a.md added (3000 bytes)', f["evidence"])
        self.assertIn('"Change the role grower (#9)": skills/b.md added (1000 bytes)', f["evidence"])
        self.assertNotIn("Draft grower", f["evidence"])
        self.assertNotIn("prohibitions", f["evidence"])
        self.assertTrue(f["evidence"].endswith(": guild/tasks#2"), f["evidence"])

    def test_the_foreman_is_judged_by_its_goals_tasks(self):
        findings = parse_findings(run(self.fake, self.l3_no_goals).stdout)
        self.assertEqual([f["role"] for f in findings], ["foreman", "grower"])
        self.assertIn("with no task settled", findings[0]["saw"])

    def test_the_window_and_bars_are_options(self):
        # Over 25 days steady's growth counts too; at 500 bytes a task, busy's and the
        # foreman's 3000 are too much for their 5 and 3 tasks; above 5000, nothing is.
        findings = parse_findings(run(self.fake, self.l3, TOKEN, "--days", "25").stdout)
        self.assertEqual([f["role"] for f in findings], ["grower", "steady"])
        self.assertIn("since the window opened", findings[1]["evidence"])
        findings = parse_findings(run(self.fake, self.l3, TOKEN, "--bytes-per-task", "500").stdout)
        self.assertEqual([f["role"] for f in findings], ["busy", "foreman", "grower"])
        self.assertEqual(run(self.fake, self.l3, TOKEN, "--min-bytes", "5000").stdout, "")

    def test_quiet_when_nothing_grew_in_the_window(self):
        r = run(self.fake, self.l3, TOKEN, "--days", "1")
        self.assertEqual((r.returncode, r.stdout, r.stderr), (0, "", ""))

    def test_quiet_without_a_token(self):
        r = run(self.fake, self.l3, "")
        self.assertEqual((r.returncode, r.stdout, r.stderr), (0, "", ""))


if __name__ == "__main__":
    unittest.main()
