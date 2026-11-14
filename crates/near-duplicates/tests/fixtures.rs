//! Fixture tests of near-duplicates, against the binary forge releases: it
//! finds entries saying nearly the same within a role or across roles, and
//! stays quiet on entries that only share a topic.

mod forgejo;

use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};

use forgejo::{parse_findings, Fake, LoreRepo, Scratch, TOKEN};

const DETECTOR: &str = env!("CARGO_BIN_EXE_near-duplicates");

const DISTINCT: &str = r#"# distinct

You review the work handed to you and nothing else.

- Run the tests and the acceptance commands before you read the change.
- Send back what fails, naming each item and why it fails.
- Pass only what you ran yourself and saw pass, with what it showed as evidence.
- Never approve your own work, and never land a pull request.
- Ask the foreman when the brief leaves the scope unclear.

```sh
guild task review <task> --pass --evidence "<what you ran and what it showed>"
```
"#;

const ON_TOPIC: &str = r#"# on-topic

- Read the tests first: they say what the change must do, before any code.
- When a review sends work back, fix every item named, not only the first.
- Keep a pull request to one change a reviewer can read in one sitting.

```sh
guild task review <task> --pass --evidence "<what you ran and what it showed>"
```
"#;

const SAID: &str = "- Run the tests and the acceptance commands yourself before you read the change or its description.";
const SAID_AGAIN: &str =
    "- Before reading the change or its description, run the acceptance commands and the tests yourself.";
const CROSS: &str =
    "- Never approve your own work, and never land a pull request: the warden does.";
const CROSS_AGAIN: &str =
    "- You never land a pull request and never approve your own work; that is the warden's.";

/// A role: its core.md, and its skills by name.
type Role<'a> = (&'a str, String, Vec<(&'a str, String)>);

/// Lore of these roles, served by a fake Forgejo.
struct Served {
    fake: Fake,
    _dir: Scratch,
}

fn serve(roles: &[Role]) -> Served {
    let dir = Scratch::new("near-duplicates");
    let repo = LoreRepo::new(dir.0.join("lore"));
    let mut files: Vec<(String, String)> = Vec::new();
    for (role, core, skills) in roles {
        files.push((format!("{role}/role.toml"), format!("name = \"{role}\"\n")));
        files.push((format!("{role}/core.md"), core.clone()));
        for (name, text) in skills {
            files.push((format!("{role}/skills/{name}"), text.clone()));
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

fn within() -> Vec<Role<'static>> {
    let core = DISTINCT.replace(
        "- Run the tests and the acceptance commands before you read the change.",
        SAID,
    );
    vec![
        (
            "reviewer",
            core,
            vec![(
                "review.md",
                format!("# review\n\n{SAID_AGAIN}\n- Read the diff last.\n"),
            )],
        ),
        ("coder", ON_TOPIC.into(), vec![]),
    ]
}

fn across() -> Vec<Role<'static>> {
    vec![
        ("alpha", format!("# alpha\n\n{CROSS}\n"), vec![]),
        ("beta", format!("# beta\n\n{CROSS_AGAIN}\n"), vec![]),
        (
            "gamma",
            ON_TOPIC.into(),
            vec![("x.md", format!("# x\n\n{CROSS}\n"))],
        ),
    ]
}

fn contained() -> Vec<Role<'static>> {
    let longer =
        "- Every command prints exactly one JSON object on stdout, warnings go to stderr, \
                  and a failure exits non-zero naming each fault with its field and its fix.";
    let shorter =
        "- Every command prints exactly one JSON object on stdout; warnings go to stderr, a failure exits non-zero.";
    vec![(
        "cli",
        format!("# cli\n\n{longer}\n"),
        vec![("shape.md", format!("# shape\n\n{shorter}\n"))],
    )]
}

#[test]
fn finds_a_near_duplicate_within_a_role() {
    let r = run(&serve(&within()), TOKEN);
    assert_eq!(
        r.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&r.stderr)
    );
    let out = stdout(&r);
    let findings = parse_findings(&out);
    assert_eq!(findings.len(), 1, "{out}");
    let f = &findings[0];
    assert_eq!(f["role"], "reviewer");
    assert!(
        f["saw"].contains("2 entries say nearly the same within the role"),
        "{}",
        f["saw"]
    );
    assert!(
        f["evidence"].contains("reviewer/core.md:5"),
        "{}",
        f["evidence"]
    );
    assert!(
        f["evidence"].contains("reviewer/skills/review.md:3"),
        "{}",
        f["evidence"]
    );
    assert_eq!(
        out,
        include_str!("expected/within.txt"),
        "byte for byte what the Python detector printed"
    );
}

#[test]
fn finds_a_near_duplicate_across_roles_once() {
    let out = stdout(&run(&serve(&across()), TOKEN));
    let findings = parse_findings(&out);
    assert_eq!(findings.len(), 1, "{findings:?}");
    let f = &findings[0];
    assert!(
        f["saw"].contains("3 entries say nearly the same across the roles alpha, beta, gamma"),
        "{}",
        f["saw"]
    );
    assert!(f["saw"].contains("lift what they share"), "{}", f["saw"]);
    for place in ["alpha/core.md:3", "beta/core.md:3", "gamma/skills/x.md:3"] {
        assert!(f["evidence"].contains(place), "{place}: {}", f["evidence"]);
    }
    assert_eq!(
        out,
        include_str!("expected/across.txt"),
        "byte for byte what the Python detector printed"
    );
}

#[test]
fn finds_a_short_entry_said_again_inside_a_longer_one() {
    let out = stdout(&run(&serve(&contained()), TOKEN));
    let roles: Vec<String> = parse_findings(&out)
        .into_iter()
        .map(|f| f["role"].clone())
        .collect();
    assert_eq!(roles, ["cli"]);
    assert_eq!(
        out,
        include_str!("expected/contained.txt"),
        "byte for byte what the Python detector printed"
    );
}

#[test]
fn quiet_on_entries_sharing_only_a_topic() {
    let served = serve(&[
        ("reviewer", DISTINCT.into(), vec![]),
        (
            "coder",
            ON_TOPIC.into(),
            vec![("more.md", "# more\n\n- Commit small.\n".into())],
        ),
    ]);
    quiet(&run(&served, TOKEN));
}

#[test]
fn quiet_on_short_entries_and_example_blocks() {
    // Identical short lines and identical example blocks are not entries said twice.
    let same = "- Ask the foreman.\n\n```sh\nguild task review <task> --pass --evidence x y z w v u t\n```\n";
    let served = serve(&[
        ("a", format!("# a\n\n{same}"), vec![]),
        ("b", format!("# b\n\n{same}"), vec![]),
    ]);
    quiet(&run(&served, TOKEN));
}

#[test]
fn quiet_without_a_token() {
    let served = serve(&[
        ("a", format!("# a\n\n{CROSS}\n"), vec![]),
        ("b", format!("# b\n\n{CROSS}\n"), vec![]),
    ]);
    quiet(&run(&served, ""));
}

#[test]
fn answers_help_with_its_usage() {
    let r = Command::new(DETECTOR)
        .arg("--help")
        .env_clear()
        .output()
        .unwrap();
    assert_eq!(r.status.code(), Some(0));
    assert!(stdout(&r).starts_with("usage: near-duplicates\n"));
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
    assert!(String::from_utf8_lossy(&r.stderr).contains("run it as `near-duplicates`"));
}
