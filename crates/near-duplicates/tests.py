"""Fixture tests of near-duplicates: it finds entries saying nearly the same within a role
or across roles, and stays quiet on entries that only share a topic."""

import os
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

DISTINCT = """# distinct

You review the work handed to you and nothing else.

- Run the tests and the acceptance commands before you read the change.
- Send back what fails, naming each item and why it fails.
- Pass only what you ran yourself and saw pass, with what it showed as evidence.
- Never approve your own work, and never land a pull request.
- Ask the foreman when the brief leaves the scope unclear.

```sh
guild task review <task> --pass --evidence "<what you ran and what it showed>"
```
"""

ON_TOPIC = """# on-topic

- Read the tests first: they say what the change must do, before any code.
- When a review sends work back, fix every item named, not only the first.
- Keep a pull request to one change a reviewer can read in one sitting.

```sh
guild task review <task> --pass --evidence "<what you ran and what it showed>"
```
"""

SAID = "- Run the tests and the acceptance commands yourself before you read the change or its description."
SAID_AGAIN = "- Before reading the change or its description, run the acceptance commands and the tests yourself."
CROSS = "- Never approve your own work, and never land a pull request: the warden does."
CROSS_AGAIN = "- You never land a pull request and never approve your own work; that is the warden's."


def lore_with(roles):
    tmp = tempfile.mkdtemp(prefix="near-duplicates-")
    repo = LoreRepo(os.path.join(tmp, "lore"))
    files = {}
    for role, (core, skills) in roles.items():
        files[f"{role}/role.toml"] = f'name = "{role}"\n'
        files[f"{role}/core.md"] = core
        for name, text in skills.items():
            files[f"{role}/skills/{name}"] = text
    repo.commit(files, "Lore", time.time() - 3600)
    return repo


def run(fake, token=TOKEN):
    env = {"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "GUILD_FORGEJO_URL": fake.url,
           "GUILD_FORGEJO_TOKEN": token, "HOME": tempfile.gettempdir()}
    return subprocess.run([DETECTOR], capture_output=True, text=True, env=env)


class NearDuplicates(unittest.TestCase):
    def setUp(self):
        self.fakes = []

    def tearDown(self):
        for f in self.fakes:
            f.close()

    def serve(self, roles):
        fake = Fake(lore_with(roles).path)
        self.fakes.append(fake)
        return fake

    def test_finds_a_near_duplicate_within_a_role(self):
        fake = self.serve({
            "reviewer": (DISTINCT.replace("- Run the tests and the acceptance commands before you read the change.", SAID),
                         {"review.md": f"# review\n\n{SAID_AGAIN}\n- Read the diff last.\n"}),
            "coder": (ON_TOPIC, {}),
        })
        r = run(fake)
        self.assertEqual(r.returncode, 0, r.stderr)
        findings = parse_findings(r.stdout)
        self.assertEqual(len(findings), 1, r.stdout)
        f = findings[0]
        self.assertEqual(f["role"], "reviewer")
        self.assertIn("2 entries say nearly the same within the role", f["saw"])
        self.assertIn("reviewer/core.md:5", f["evidence"])
        self.assertIn("reviewer/skills/review.md:3", f["evidence"])

    def test_finds_a_near_duplicate_across_roles_once(self):
        fake = self.serve({
            "alpha": (f"# alpha\n\n{CROSS}\n", {}),
            "beta": (f"# beta\n\n{CROSS_AGAIN}\n", {}),
            "gamma": (ON_TOPIC, {"x.md": f"# x\n\n{CROSS}\n"}),
        })
        findings = parse_findings(run(fake).stdout)
        self.assertEqual(len(findings), 1, findings)
        f = findings[0]
        self.assertIn("3 entries say nearly the same across the roles alpha, beta, gamma", f["saw"])
        self.assertIn("lift what they share", f["saw"])
        for where in ("alpha/core.md:3", "beta/core.md:3", "gamma/skills/x.md:3"):
            self.assertIn(where, f["evidence"])

    def test_finds_a_short_entry_said_again_inside_a_longer_one(self):
        longer = ("- Every command prints exactly one JSON object on stdout, warnings go to stderr, "
                  "and a failure exits non-zero naming each fault with its field and its fix.")
        shorter = "- Every command prints exactly one JSON object on stdout; warnings go to stderr, a failure exits non-zero."
        fake = self.serve({"cli": (f"# cli\n\n{longer}\n", {"shape.md": f"# shape\n\n{shorter}\n"})})
        findings = parse_findings(run(fake).stdout)
        self.assertEqual([f["role"] for f in findings], ["cli"])

    def test_quiet_on_entries_sharing_only_a_topic(self):
        fake = self.serve({"reviewer": (DISTINCT, {}), "coder": (ON_TOPIC, {"more.md": "# more\n\n- Commit small.\n"})})
        r = run(fake)
        self.assertEqual((r.returncode, r.stdout, r.stderr), (0, "", ""))

    def test_quiet_on_short_entries_and_example_blocks(self):
        # Identical short lines and identical example blocks are not entries said twice.
        fake = self.serve({"a": ("# a\n\n- Ask the foreman.\n\n```sh\nguild task review <task> --pass --evidence x y z w v u t\n```\n", {}),
                           "b": ("# b\n\n- Ask the foreman.\n\n```sh\nguild task review <task> --pass --evidence x y z w v u t\n```\n", {})})
        r = run(fake)
        self.assertEqual((r.returncode, r.stdout, r.stderr), (0, "", ""))

    def test_quiet_without_a_token(self):
        fake = self.serve({"a": (f"# a\n\n{CROSS}\n", {}), "b": (f"# b\n\n{CROSS}\n", {})})
        r = run(fake, token="")
        self.assertEqual((r.returncode, r.stdout, r.stderr), (0, "", ""))


if __name__ == "__main__":
    unittest.main()
