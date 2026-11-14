//! Fixture tests of memory-growth, against the binary forge releases: it
//! finds a role whose memory grew faster than the tasks it settled over the
//! window, and stays quiet on growth the work pays for, growth before the
//! window, and a new role's first draft.

mod forgejo;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use forgejo::{parse_findings, Fake, LoreRepo, Scratch, TOKEN};

const DETECTOR: &str = env!("CARGO_BIN_EXE_memory-growth");
const DAY: f64 = 86400.0;

/// `n` bytes of markdown.
fn text(n: usize) -> String {
    let line = format!("{}\n", "- practice ".repeat(8));
    line.repeat(n / line.len() + 1)[..n].to_string()
}

fn role(name: &str, core: usize) -> Vec<(String, String)> {
    vec![
        (format!("{name}/role.toml"), format!("name = \"{name}\"\n")),
        (format!("{name}/core.md"), text(core)),
    ]
}

fn commit(repo: &LoreRepo, files: &[(String, String)], message: &str, at: f64, branch: &str) {
    let files: Vec<(&str, Option<&str>)> = files
        .iter()
        .map(|(p, t)| (p.as_str(), Some(t.as_str())))
        .collect();
    repo.commit(&files, message, at, branch);
}

fn one(path: &str, text: String) -> Vec<(String, String)> {
    vec![(path.to_string(), text)]
}

/// Lore over 30 days; the detector's window is its default 14.
fn history(dir: &Path, now: f64) -> LoreRepo {
    let repo = LoreRepo::new(dir.join("lore"));
    let mut first = one("README.md", "# lore\n".into());
    for r in ["grower", "busy", "steady", "foreman"] {
        first.extend(role(r, 200));
    }
    first.push(("grower/prohibitions.toml".into(), "x".repeat(50)));
    commit(&repo, &first, "Lore", now - 30.0 * DAY, "main");
    commit(
        &repo,
        &one("steady/skills/old.md", text(5000)),
        "Grow steady before the window",
        now - 20.0 * DAY,
        "main",
    );
    // In the window:
    commit(
        &repo,
        &one("grower/skills/a.md", text(3000)),
        "Grow grower",
        now - 10.0 * DAY,
        "main",
    );
    commit(
        &repo,
        &one("busy/skills/a.md", text(3000)),
        "Grow busy",
        now - 10.0 * DAY + 60.0,
        "main",
    );
    commit(
        &repo,
        &one("foreman/skills/split.md", text(3000)),
        "Grow foreman",
        now - 9.0 * DAY,
        "main",
    );
    commit(
        &repo,
        &one("grower/prohibitions.toml", "x".repeat(9000)),
        "Prohibitions are not memory",
        now - 8.0 * DAY,
        "main",
    );
    let mut newbie = role("newbie", 6000);
    newbie.push(("newbie/skills/s.md".into(), text(1000)));
    commit(&repo, &newbie, "Add newbie", now - 5.0 * DAY, "main");
    commit(
        &repo,
        &one("newbie/core.md", text(6100)),
        "Touch newbie",
        now - 4.0 * DAY,
        "main",
    );
    // A draft on a side branch, landed by a merge: lore as it landed counts it once.
    repo.branch("draft");
    commit(
        &repo,
        &one("grower/skills/b.md", text(1000)),
        "Draft grower",
        now - 3.0 * DAY,
        "draft",
    );
    repo.merge("draft", "Change the role grower (#9)", now - 2.0 * DAY);
    repo
}

fn l3(dir: &Path, now: f64, goal_tasks: bool) -> PathBuf {
    let path = dir.join("l3.sqlite");
    let db = rusqlite::Connection::open(&path).unwrap();
    db.execute_batch(
        "CREATE TABLE settlements (id INTEGER PRIMARY KEY, task TEXT NOT NULL, role TEXT NOT NULL,
             at INTEGER NOT NULL, tokens INTEGER, seconds INTEGER NOT NULL);
         CREATE TABLE flow (id INTEGER PRIMARY KEY, task INTEGER NOT NULL, at INTEGER NOT NULL,
             record TEXT NOT NULL);",
    )
    .unwrap();
    let mut rows: Vec<(String, &str, f64)> = vec![
        ("guild/tasks#1".into(), "grower", now - 20.0 * DAY), // before the window: not counted
        ("guild/tasks#2".into(), "grower", now - 6.0 * DAY),
        ("guild/tasks#2".into(), "grower", now - 6.0 * DAY + 60.0), // the same task twice: one task
    ];
    rows.extend((10..15).map(|n| {
        (
            format!("guild/tasks#{n}"),
            "busy",
            now - n as f64 * DAY / 2.0,
        )
    }));
    rows.push(("guild/tasks#10".into(), "warden", now - 4.0 * DAY));
    for (task, role, at) in rows {
        db.execute(
            "INSERT INTO settlements (task, role, at, seconds) VALUES (?1, ?2, ?3, 60)",
            rusqlite::params![task, role, at as i64],
        )
        .unwrap();
    }
    for n in [2, 10, 11] {
        let mut rec = serde_json::json!({"task": n, "phase": "landed", "at": now as i64});
        if goal_tasks {
            rec["goal"] = 6.into();
        }
        db.execute(
            "INSERT INTO flow (task, at, record) VALUES (?1, ?2, ?3)",
            rusqlite::params![n, now as i64, rec.to_string()],
        )
        .unwrap();
    }
    db.close().unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444)).unwrap(); // the detector only reads it
    path
}

/// Lore and two L3s, made once for every case, as the Python tests did.
struct World {
    fake: Fake,
    l3: PathBuf,
    l3_no_goals: PathBuf,
    _dirs: (Scratch, Scratch),
}

fn world() -> &'static World {
    static WORLD: OnceLock<World> = OnceLock::new();
    WORLD.get_or_init(|| {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs_f64();
        let (a, b) = (Scratch::new("memory-growth"), Scratch::new("memory-growth"));
        let fake = Fake::new(&history(&a.0, now).path);
        let l3_goals = l3(&a.0, now, true);
        let l3_no_goals = l3(&b.0, now, false);
        World {
            fake,
            l3: l3_goals,
            l3_no_goals,
            _dirs: (a, b),
        }
    })
}

fn run(l3: &Path, token: &str, args: &[&str]) -> Output {
    Command::new(DETECTOR)
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("GUILD_FORGEJO_URL", &world().fake.url)
        .env("GUILD_FORGEJO_TOKEN", token)
        .env("GUILD_L3", l3)
        .env("HOME", std::env::temp_dir())
        .output()
        .expect("the detector runs")
}

fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}

fn roles(out: &str) -> Vec<String> {
    parse_findings(out)
        .into_iter()
        .map(|f| f["role"].clone())
        .collect()
}

#[test]
fn finds_memory_growing_faster_than_the_work() {
    let r = run(&world().l3, TOKEN, &[]);
    assert_eq!(
        r.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&r.stderr)
    );
    let out = stdout(&r);
    let findings = parse_findings(&out);
    // busy's growth is paid for by its 5 tasks, steady grew before the window, newbie's
    // first draft is no growth, the foreman is credited with its goals' 3 tasks.
    assert_eq!(roles(&out), ["grower"], "{out}");
    let f = &findings[0];
    assert!(
        f["saw"].contains(
            "grew 4000 bytes, 200→4200, over the last 14 days while it settled 1 task \
             (4000 bytes per task; at most 1024 per task)"
        ),
        "{}",
        f["saw"]
    );
    let ev = &f["evidence"];
    assert!(
        ev.contains("\"Grow grower\": skills/a.md added (3000 bytes)"),
        "{ev}"
    );
    assert!(
        ev.contains("\"Change the role grower (#9)\": skills/b.md added (1000 bytes)"),
        "{ev}"
    );
    assert!(!ev.contains("Draft grower"), "{ev}");
    assert!(!ev.contains("prohibitions"), "{ev}");
    assert!(ev.ends_with(": guild/tasks#2"), "{ev}");
}

#[test]
fn the_foreman_is_judged_by_its_goals_tasks() {
    let out = stdout(&run(&world().l3_no_goals, TOKEN, &[]));
    let findings = parse_findings(&out);
    assert_eq!(roles(&out), ["foreman", "grower"]);
    assert!(
        findings[0]["saw"].contains("with no task settled"),
        "{}",
        findings[0]["saw"]
    );
}

#[test]
fn the_window_and_bars_are_options() {
    // Over 25 days steady's growth counts too; at 500 bytes a task, busy's and the
    // foreman's 3000 are too much for their 5 and 3 tasks; above 5000, nothing is.
    let out = stdout(&run(&world().l3, TOKEN, &["--days", "25"]));
    let findings = parse_findings(&out);
    assert_eq!(roles(&out), ["grower", "steady"]);
    assert!(
        findings[1]["evidence"].contains("since the window opened"),
        "{}",
        findings[1]["evidence"]
    );
    assert_eq!(
        roles(&stdout(&run(
            &world().l3,
            TOKEN,
            &["--bytes-per-task", "500"]
        ))),
        ["busy", "foreman", "grower"]
    );
    assert_eq!(
        stdout(&run(&world().l3, TOKEN, &["--min-bytes", "5000"])),
        ""
    );
}

#[test]
fn quiet_when_nothing_grew_in_the_window() {
    let r = run(&world().l3, TOKEN, &["--days", "1"]);
    assert_eq!(
        (r.status.code(), r.stdout.as_slice(), r.stderr.as_slice()),
        (Some(0), &b""[..], &b""[..])
    );
}

#[test]
fn quiet_without_a_token() {
    let r = run(&world().l3, "", &[]);
    assert_eq!(
        (r.status.code(), r.stdout.as_slice(), r.stderr.as_slice()),
        (Some(0), &b""[..], &b""[..])
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
    assert!(stdout(&r)
        .starts_with("usage: memory-growth [--days N] [--bytes-per-task B] [--min-bytes M]\n"));
}

#[test]
fn refuses_an_option_it_does_not_take_with_its_usage() {
    let r = run(&world().l3, TOKEN, &["--weeks", "2"]);
    assert_eq!(r.status.code(), Some(2));
    assert!(r.stdout.is_empty());
    assert!(String::from_utf8_lossy(&r.stderr).starts_with("usage: memory-growth [--days N]"));
}
