//! Fixture tests of specific-entries, against the binary forge releases: it
//! finds entries naming a specific repo, path, issue or URL, and stays quiet
//! on general ones.

mod forgejo;

use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use forgejo::{parse_findings, Fake, LoreRepo, Scratch, TOKEN};

const DETECTOR: &str = env!("CARGO_BIN_EXE_specific-entries");

const GENERAL: &str = r#"# general

You do the work of a general role.

- Read the brief, then `skills/hand-in.md` before you hand in.
- Hand work to `guild/tasks`; a task's lore lives in `guild/lore`.
- Run the repo's own checks, `<repo>/scripts/check`, and the one in `skills/{name}.md`.
- Keep I/O at the edges; read/write only what the task names, and/or ask.
- Never run what the task did not hand you.

```sh
guild task check <task> <item> --evidence "<what shows it>"
```
"#;

const SPECIFIC: &str = "# specific

- Before handing in, run `./verify.sh` at the repo's root.
- The flaky test was fixed in sunfmin/control#722, so retry it once.
- Read https://example.com/guide before you start.
- Install the toolchain from `acme/tap`, as in issue #41.

A general paragraph that names nothing in particular.

- Format with the config at src/config/rustfmt.toml.
";

/// A role: its core.md, and its skills by name.
type Role<'a> = (&'a str, &'a str, Vec<(&'a str, &'a str)>);

/// Lore of these roles, served by a fake Forgejo.
struct Served {
    fake: Fake,
    _dir: Scratch,
}

fn serve(roles: &[Role]) -> Served {
    let dir = Scratch::new("specific-entries");
    let repo = LoreRepo::new(dir.0.join("lore"));
    let mut files: Vec<(String, String)> = vec![(
        "README.md".into(),
        "# lore\nsee https://example.com and src/main.rs\n".into(),
    )];
    for (role, core, skills) in roles {
        files.push((format!("{role}/role.toml"), format!("name = \"{role}\"\n")));
        // prohibitions are not memory: what they name is never a finding of this detector
        files.push((
            format!("{role}/prohibitions.toml"),
            "[[prohibition]]\ntext = \"never touch ~/.ssh/id_rsa\"\n".into(),
        ));
        files.push((format!("{role}/core.md"), core.to_string()));
        for (name, text) in skills {
            files.push((format!("{role}/skills/{name}"), text.to_string()));
        }
    }
    let files: Vec<(&str, Option<&str>)> = files
        .iter()
        .map(|(p, t)| (p.as_str(), Some(t.as_str())))
        .collect();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    repo.commit(&files, "Lore", now - 3600.0, "main");
    Served {
        fake: Fake::new(&repo.path),
        _dir: dir,
    }
}

fn run(served: &Served, token: &str) -> Output {
    Command::new(DETECTOR)
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("GUILD_FORGEJO_URL", &served.fake.url)
        .env("GUILD_FORGEJO_TOKEN", token)
        .env("HOME", std::env::temp_dir())
        .output()
        .expect("the detector runs")
}

fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}

fn quiet(o: &Output) {
    assert_eq!(
        (
            o.status.code(),
            stdout(o),
            String::from_utf8_lossy(&o.stderr).into_owned()
        ),
        (Some(0), String::new(), String::new())
    );
}

#[test]
fn finds_entries_naming_a_repo_path_issue_or_url() {
    let served = serve(&[
        ("general", GENERAL, vec![]),
        (
            "narrow",
            "# narrow\n\nGeneral.\n",
            vec![("hand-in.md", SPECIFIC)],
        ),
    ]);
    let r = run(&served, TOKEN);
    assert_eq!(
        r.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&r.stderr)
    );
    let out = stdout(&r);
    let findings = parse_findings(&out);
    let roles: Vec<&str> = findings.iter().map(|f| f["role"].as_str()).collect();
    assert_eq!(roles, ["narrow"], "{out}");
    let f = &findings[0];
    assert!(
        f["saw"].contains("5 entries name a specific repo, path or issue"),
        "{}",
        f["saw"]
    );
    for line in [
        "narrow/skills/hand-in.md:3 names the path `./verify.sh`",
        "narrow/skills/hand-in.md:4 names the issue `sunfmin/control#722`",
        "narrow/skills/hand-in.md:5 names the URL `https://example.com/guide`",
        "narrow/skills/hand-in.md:6 names the repo `acme/tap`",
        "narrow/skills/hand-in.md:6 names the issue `#41`",
        "narrow/skills/hand-in.md:10 names the path `src/config/rustfmt.toml`",
    ] {
        assert!(f["evidence"].contains(line), "{line}: {}", f["evidence"]);
    }
    assert_eq!(
        out,
        include_str!("expected/specific.txt"),
        "byte for byte what the Python detector printed"
    );
}

#[test]
fn quiet_on_general_entries() {
    quiet(&run(
        &serve(&[
            ("general", GENERAL, vec![("hand-in.md", GENERAL)]),
            ("other", GENERAL, vec![]),
        ]),
        TOKEN,
    ));
}

#[test]
fn one_finding_per_role() {
    let served = serve(&[
        ("a", SPECIFIC, vec![]),
        ("b", GENERAL, vec![("x.md", SPECIFIC)]),
        ("c", GENERAL, vec![]),
    ]);
    let out = stdout(&run(&served, TOKEN));
    let roles: Vec<String> = parse_findings(&out)
        .into_iter()
        .map(|f| f["role"].clone())
        .collect();
    assert_eq!(roles, ["a", "b"]);
    assert_eq!(
        out,
        include_str!("expected/per-role.txt"),
        "byte for byte what the Python detector printed"
    );
}

#[test]
fn quiet_without_a_token() {
    // As under `guild detector try`: no token, lore unread, nothing seen.
    quiet(&run(&serve(&[("narrow", SPECIFIC, vec![])]), ""));
}

#[test]
fn fails_loudly_when_its_token_is_refused() {
    let r = run(&serve(&[("narrow", SPECIFIC, vec![])]), "wrong");
    assert_eq!((r.status.code(), stdout(&r)), (Some(1), String::new()));
    assert!(
        String::from_utf8_lossy(&r.stderr).contains("Forgejo answered 401"),
        "{}",
        String::from_utf8_lossy(&r.stderr)
    );
}

#[test]
fn answers_help_with_its_usage() {
    let r = Command::new(DETECTOR)
        .arg("--help")
        .env_clear()
        .output()
        .unwrap();
    assert_eq!(r.status.code(), Some(0));
    assert!(stdout(&r).starts_with("usage: specific-entries\n"));
}

#[test]
fn refuses_an_argument_naming_how_to_run_it() {
    let r = Command::new(DETECTOR)
        .arg("x")
        .env_clear()
        .output()
        .unwrap();
    assert_eq!(r.status.code(), Some(2));
    assert!(r.stdout.is_empty());
    assert!(String::from_utf8_lossy(&r.stderr).contains("run it as `specific-entries`"));
}
