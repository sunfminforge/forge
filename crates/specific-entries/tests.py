"""Fixture tests of specific-entries: it finds entries naming a specific repo, path, issue
or URL, and stays quiet on general ones."""

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

ROLE_TOML = 'name = "{0}"\n'

GENERAL = """# general

You do the work of a general role.

- Read the brief, then `skills/hand-in.md` before you hand in.
- Hand work to `guild/tasks`; a task's lore lives in `guild/lore`.
- Run the repo's own checks, `<repo>/scripts/check`, and the one in `skills/{name}.md`.
- Keep I/O at the edges; read/write only what the task names, and/or ask.
- Never run what the task did not hand you.

```sh
guild task check <task> <item> --evidence "<what shows it>"
```
"""

SPECIFIC = """# specific

- Before handing in, run `./verify.sh` at the repo's root.
- The flaky test was fixed in sunfmin/control#722, so retry it once.
- Read https://example.com/guide before you start.
- Install the toolchain from `acme/tap`, as in issue #41.

A general paragraph that names nothing in particular.

- Format with the config at src/config/rustfmt.toml.
"""


def lore_with(roles):
    tmp = tempfile.mkdtemp(prefix="specific-entries-")
    repo = LoreRepo(os.path.join(tmp, "lore"))
    files = {"README.md": "# lore\nsee https://example.com and src/main.rs\n"}
    for role, (core, skills) in roles.items():
        files[f"{role}/role.toml"] = ROLE_TOML.format(role)
        # prohibitions are not memory: what they name is never a finding of this detector
        files[f"{role}/prohibitions.toml"] = '[[prohibition]]\ntext = "never touch ~/.ssh/id_rsa"\n'
        files[f"{role}/core.md"] = core
        for name, text in skills.items():
            files[f"{role}/skills/{name}"] = text
    repo.commit(files, "Lore", time.time() - 3600)
    return repo


def run(fake, token=TOKEN):
    env = {"PATH": "/usr/bin:/bin:/usr/sbin:/sbin", "GUILD_FORGEJO_URL": fake.url,
           "GUILD_FORGEJO_TOKEN": token, "HOME": tempfile.gettempdir()}
    return subprocess.run([DETECTOR], capture_output=True, text=True, env=env)


class SpecificEntries(unittest.TestCase):
    def setUp(self):
        self.fakes = []

    def tearDown(self):
        for f in self.fakes:
            f.close()

    def serve(self, roles):
        fake = Fake(lore_with(roles).path)
        self.fakes.append(fake)
        return fake

    def test_finds_entries_naming_a_repo_path_issue_or_url(self):
        fake = self.serve({"general": (GENERAL, {}), "narrow": ("# narrow\n\nGeneral.\n", {"hand-in.md": SPECIFIC})})
        r = run(fake)
        self.assertEqual(r.returncode, 0, r.stderr)
        findings = parse_findings(r.stdout)
        self.assertEqual([f["role"] for f in findings], ["narrow"], r.stdout)
        f = findings[0]
        self.assertIn("5 entries name a specific repo, path or issue", f["saw"])
        for line in [
            "narrow/skills/hand-in.md:3 names the path `./verify.sh`",
            "narrow/skills/hand-in.md:4 names the issue `sunfmin/control#722`",
            "narrow/skills/hand-in.md:5 names the URL `https://example.com/guide`",
            "narrow/skills/hand-in.md:6 names the repo `acme/tap`",
            "narrow/skills/hand-in.md:6 names the issue `#41`",
            "narrow/skills/hand-in.md:10 names the path `src/config/rustfmt.toml`",
        ]:
            self.assertIn(line, f["evidence"])

    def test_quiet_on_general_entries(self):
        fake = self.serve({"general": (GENERAL, {"hand-in.md": GENERAL}), "other": (GENERAL, {})})
        r = run(fake)
        self.assertEqual((r.returncode, r.stdout, r.stderr), (0, "", ""))

    def test_one_finding_per_role(self):
        fake = self.serve({"a": (SPECIFIC, {}), "b": (GENERAL, {"x.md": SPECIFIC}), "c": (GENERAL, {})})
        findings = parse_findings(run(fake).stdout)
        self.assertEqual([f["role"] for f in findings], ["a", "b"])

    def test_quiet_without_a_token(self):
        # As under `guild detector try`: no token, lore unread, nothing seen.
        fake = self.serve({"narrow": (SPECIFIC, {})})
        r = run(fake, token="")
        self.assertEqual((r.returncode, r.stdout, r.stderr), (0, "", ""))

    def test_fails_loudly_when_its_token_is_refused(self):
        fake = self.serve({"narrow": (SPECIFIC, {})})
        r = run(fake, token="wrong")
        self.assertEqual((r.returncode, r.stdout), (1, ""))
        self.assertIn("Forgejo answered 401", r.stderr)


if __name__ == "__main__":
    unittest.main()
