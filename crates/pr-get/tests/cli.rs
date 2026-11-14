//! The real binary against a stand-in for `curl` (examples/curl.rs), first
//! and alone on the PATH: its stdout, one JSON object, and its exit code.
//! No test reaches a network.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::{Value, json};

const TOKEN: &str = "s3cret-token";

/// A directory of one test's own: its PATH, its state, its calls.
struct Forge {
    dir: PathBuf,
    env: Vec<(String, String)>,
    path: String,
}

static NEXT: AtomicU32 = AtomicU32::new(0);

fn pull() -> Value {
    json!({
        "id": 11,
        "number": 1,
        "title": "Detectors: one failure across several roles",
        "state": "closed",
        "merged": true,
        "merged_at": null,
        "user": {"login": "rust-cli-coder", "id": 3},
        "head": {"ref": "guild/tasks-24", "sha": "de01906"},
        "base": {"ref": "main"},
        "html_url": "http://forge.test/guild/forge/pulls/1"
    })
}

impl Forge {
    /// A forge holding `guild/forge` pull request 1, that lets in [`TOKEN`].
    fn new() -> Forge {
        Forge::with(json!({"token": TOKEN, "pulls": {"guild/forge/1": pull()}}))
    }

    fn with(state: Value) -> Forge {
        let dir = std::env::temp_dir().join(format!(
            "pr-get-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let bin = dir.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::os::unix::fs::symlink(stand_in(), bin.join("curl")).unwrap();
        let state_path = dir.join("state.json");
        std::fs::write(&state_path, state.to_string()).unwrap();
        Forge {
            env: vec![
                ("GUILD_FORGEJO_URL".into(), "http://forge.test/".into()),
                ("GUILD_FORGEJO_TOKEN".into(), TOKEN.into()),
                ("PR_GET_STANDIN".into(), state_path.display().to_string()),
            ],
            path: bin.display().to_string(),
            dir,
        }
    }

    fn set(mut self, name: &str, value: &str) -> Forge {
        self.env.retain(|(k, _)| k != name);
        self.env.push((name.into(), value.into()));
        self
    }

    fn unset(mut self, name: &str) -> Forge {
        self.env.retain(|(k, _)| k != name);
        self
    }

    /// A PATH with no curl on it.
    fn no_curl(mut self) -> Forge {
        let empty = self.dir.join("empty");
        std::fs::create_dir_all(&empty).unwrap();
        self.path = empty.display().to_string();
        self
    }

    /// What `pr-get <args>` printed, as JSON, and its exit code.
    fn run(&self, args: &[&str]) -> (Value, i32) {
        let out = Command::new(env!("CARGO_BIN_EXE_pr-get"))
            .args(args)
            .env_clear()
            .env("PATH", &self.path)
            .envs(self.env.iter().map(|(k, v)| (k, v)))
            .output()
            .unwrap();
        let stdout = String::from_utf8(out.stdout).unwrap();
        assert!(
            out.stderr.is_empty(),
            "stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert_eq!(stdout.lines().count(), 1, "one line of JSON: {stdout}");
        assert!(!stdout.contains(TOKEN), "the token printed: {stdout}");
        let value: Value =
            serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("{e}: {stdout}"));
        assert!(value.is_object(), "{stdout}");
        (value, out.status.code().unwrap())
    }

    /// The answer of `pr-get <args>`, after exit 0.
    fn answer(&self, args: &[&str]) -> Value {
        let (value, code) = self.run(args);
        assert_eq!(code, 0, "{args:?} refused: {value}");
        value
    }

    /// The faults of `pr-get <args>`, after exit 1, each `(code, fix)`.
    fn refused(&self, args: &[&str]) -> Vec<(String, String, String)> {
        let (value, code) = self.run(args);
        assert_eq!(code, 1, "{args:?} answered: {value}");
        let object = value.as_object().unwrap();
        assert_eq!(object.len(), 2, "{value}");
        assert!(object["error"].is_string(), "{value}");
        object["faults"]
            .as_array()
            .unwrap()
            .iter()
            .map(|f| {
                (
                    f["code"].as_str().unwrap().to_string(),
                    f["message"].as_str().unwrap().to_string(),
                    f["fix"].as_str().unwrap().to_string(),
                )
            })
            .collect()
    }

    /// Every call the stand-in saw.
    fn calls(&self) -> Vec<Value> {
        let path = self.dir.join("state.json.calls");
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}

impl Drop for Forge {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// The stand-in cargo built beside the binary.
fn stand_in() -> PathBuf {
    let bin = Path::new(env!("CARGO_BIN_EXE_pr-get"));
    let path = bin.parent().unwrap().join("examples").join("curl");
    assert!(
        path.exists(),
        "the curl stand-in is built: {}",
        path.display()
    );
    path
}

fn codes(faults: &[(String, String, String)]) -> Vec<&str> {
    faults.iter().map(|(code, _, _)| code.as_str()).collect()
}

#[test]
fn prints_the_whole_pull_request() {
    let forge = Forge::new();
    assert_eq!(forge.answer(&["guild/forge", "1"]), pull());
    let calls = forge.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0]["url"],
        "http://forge.test/api/v1/repos/guild/forge/pulls/1"
    );
}

#[test]
fn prints_the_fields_named_in_their_order() {
    let forge = Forge::new();
    let answer = forge.answer(&["guild/forge", "1", "--field", "title,state"]);
    assert_eq!(
        answer,
        json!({"title": "Detectors: one failure across several roles", "state": "closed"})
    );
    let keys: Vec<&String> = answer.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["title", "state"]);
    let answer = forge.answer(&[
        "guild/forge",
        "1",
        "--field",
        "head.ref",
        "--field",
        "merged_at,user.login,state",
        "--field",
        "state",
    ]);
    assert_eq!(
        answer.to_string(),
        r#"{"head.ref":"guild/tasks-24","merged_at":null,"user.login":"rust-cli-coder","state":"closed"}"#
    );
}

#[test]
fn the_token_goes_on_stdin_never_on_the_command_line() {
    let forge = Forge::new();
    forge.answer(&["guild/forge", "1"]);
    let call = &forge.calls()[0];
    assert_eq!(call["args"], json!(["-q", "--config", "-"]));
    assert_eq!(call["authorization"], format!("token {TOKEN}"));
    assert!(!call["args"].to_string().contains(TOKEN));
}

#[test]
fn asks_another_forge_with_another_token() {
    let forge = Forge::new()
        .set("GITHUB_TOKEN", TOKEN)
        .unset("GUILD_FORGEJO_TOKEN");
    forge.answer(&[
        "--api",
        "https://api.github.com/",
        "--token-env",
        "GITHUB_TOKEN",
        "guild/forge",
        "1",
    ]);
    assert_eq!(
        forge.calls()[0]["url"],
        "https://api.github.com/repos/guild/forge/pulls/1"
    );
}

#[test]
fn not_found() {
    let forge = Forge::new();
    let faults = forge.refused(&["guild/forge", "2", "--field", "title,state"]);
    assert_eq!(codes(&faults), ["not_found"]);
    assert!(
        faults[0]
            .1
            .contains("guild/forge pull request 2 not found (HTTP 404)"),
        "{faults:?}"
    );
    assert!(
        faults[0].2.contains("check the repository and the number"),
        "{faults:?}"
    );
}

#[test]
fn unauthorized() {
    let forge = Forge::new().set("GUILD_FORGEJO_TOKEN", "wrong");
    let faults = forge.refused(&["guild/forge", "1"]);
    assert_eq!(codes(&faults), ["unauthorized"]);
    assert!(
        faults[0]
            .2
            .starts_with("export GUILD_FORGEJO_TOKEN as a token that may read guild/forge")
    );
}

#[test]
fn unknown_fields_are_named_each_with_the_keys_there_are() {
    let forge = Forge::new();
    let faults = forge.refused(&["guild/forge", "1", "--field", "title,titel,head.branch"]);
    assert_eq!(codes(&faults), ["unknown_field", "unknown_field"]);
    assert!(
        faults[0].2.starts_with("drop `titel` from --field"),
        "{faults:?}"
    );
    assert!(faults[0].2.contains("`title`, `state`"), "{faults:?}");
    assert!(faults[1].2.contains("`head.ref`, `head.sha`"), "{faults:?}");
}

#[test]
fn every_fault_of_the_arguments_at_once_and_no_request() {
    let forge = Forge::new();
    let faults = forge.refused(&["guild", "0", "--field", "title,,head."]);
    assert_eq!(
        codes(&faults),
        ["bad_repo", "bad_number", "bad_field", "bad_field"]
    );
    assert!(
        faults[0]
            .2
            .contains("`<owner>/<name>`, such as `guild/forge`")
    );
    assert!(faults[1].2.contains("such as `pr-get guild/forge 1`"));
    assert!(faults[2].2.contains("keys joined by `.`"));
    assert!(forge.calls().is_empty());
    for repo in ["a/b/c", "../x", "a b/c", "/x", "x/"] {
        assert_eq!(codes(&forge.refused(&[repo, "1"])), ["bad_repo"], "{repo}");
    }
    for number in ["+1", "1.0", "99999999999999999999999"] {
        assert_eq!(
            codes(&forge.refused(&["guild/forge", number])),
            ["bad_number"],
            "{number}"
        );
    }
}

#[test]
fn every_fault_of_the_environment_at_once() {
    let forge = Forge::new()
        .unset("GUILD_FORGEJO_URL")
        .unset("GUILD_FORGEJO_TOKEN")
        .no_curl();
    let faults = forge.refused(&["guild/forge", "1"]);
    assert_eq!(codes(&faults), ["no_api", "no_token", "no_curl"]);
    assert!(faults[0].2.starts_with("export GUILD_FORGEJO_URL="));
    assert!(faults[1].2.starts_with("export GUILD_FORGEJO_TOKEN="));
    assert!(faults[2].2.contains("/usr/bin/curl"));
}

#[test]
fn a_bad_api_or_token() {
    let forge = Forge::new().set("GUILD_FORGEJO_TOKEN", "a\"b");
    let faults = forge.refused(&["--api", "forge.test", "guild/forge", "1"]);
    assert_eq!(codes(&faults), ["bad_api", "bad_token"]);
    assert!(faults[0].2.starts_with("set --api to the forge's URL"));
    assert!(
        faults[1]
            .2
            .starts_with("set GUILD_FORGEJO_TOKEN to the token alone")
    );
    assert!(!faults[1].1.contains("a\"b"));
    assert!(forge.calls().is_empty());
}

#[test]
fn unreachable() {
    let forge = Forge::with(json!({"fail": "(6) Could not resolve host: forge.test"}));
    let faults = forge.refused(&["guild/forge", "1"]);
    assert_eq!(codes(&faults), ["unreachable"]);
    assert!(
        faults[0].1.contains("Could not resolve host: forge.test"),
        "{faults:?}"
    );
    assert!(faults[0].2.contains("`pr-get doctor`"));
}

#[test]
fn another_status() {
    let forge =
        Forge::with(json!({"status": 500, "body": "{\"message\": \"database is locked\"}"}));
    let faults = forge.refused(&["guild/forge", "1"]);
    assert_eq!(codes(&faults), ["http_error"]);
    assert!(
        faults[0].1.contains("HTTP 500: database is locked"),
        "{faults:?}"
    );
    assert!(faults[0].2.starts_with("run the command again"));
}

#[test]
fn a_web_page_rather_than_the_api() {
    let forge = Forge::with(json!({"status": 200, "body": "<html>sign in</html>"}));
    let faults = forge.refused(&["guild/forge", "1"]);
    assert_eq!(codes(&faults), ["not_json"]);
    assert!(faults[0].2.contains("`https://<forgejo host>/api/v1`"));
}

#[test]
fn usage() {
    let forge = Forge::new();
    let faults = forge.refused(&["guild/forge"]);
    assert_eq!(codes(&faults), ["usage"]);
    assert_eq!(faults[0].2, "run `pr-get --help` to see how to write it");
    let faults = forge.refused(&["doctor", "--nope"]);
    assert_eq!(
        faults[0].2,
        "run `pr-get doctor --help` to see how to write it"
    );
}

#[test]
fn doctor_finds_nothing_wrong_and_asks_nothing() {
    let forge = Forge::new();
    let answer = forge.answer(&["doctor"]);
    assert_eq!(answer["api"], "http://forge.test/api/v1");
    assert_eq!(answer["token_env"], "GUILD_FORGEJO_TOKEN");
    assert_eq!(answer["token"], true);
    assert!(answer["curl"].as_str().unwrap().ends_with("/bin/curl"));
    assert_eq!(answer["findings"], json!([]));
    assert!(
        answer["pr_get"]
            .as_str()
            .unwrap()
            .starts_with(env!("CARGO_PKG_VERSION"))
    );
    assert!(forge.calls().is_empty());
}

#[test]
fn doctor_names_each_fix() {
    let forge = Forge::new().unset("GUILD_FORGEJO_URL").no_curl();
    let answer = forge.answer(&["doctor", "--token-env", "GITHUB_TOKEN"]);
    assert_eq!(answer["api"], Value::Null);
    assert_eq!(answer["token"], false);
    assert_eq!(answer["curl"], Value::Null);
    let fixes: Vec<&str> = answer["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["fix"].as_str().unwrap())
        .collect();
    assert_eq!(fixes.len(), 3, "{answer}");
    assert!(fixes[0].starts_with("export GUILD_FORGEJO_URL="));
    assert!(fixes[1].starts_with("export GITHUB_TOKEN="));
    assert!(fixes[2].contains("/usr/bin/curl"));
}

/// What `pr-get <args>` printed as text, after exit 0.
fn text(args: &[&str]) -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_pr-get"))
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "{args:?}");
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn help_and_version() {
    let index = text(&["--help"]);
    assert!(index.starts_with("# `pr-get`"), "{index}");
    assert!(index.contains("pr-get doctor"));
    assert_eq!(index, text(&["-h"]));
    assert_eq!(index, text(&["help"]));
    let errors = text(&["help", "errors"]);
    for code in [
        "`not_found`",
        "`unauthorized`",
        "`unknown_field`",
        "`no_token`",
    ] {
        assert!(errors.contains(code), "{code}: {errors}");
    }
    assert!(text(&["help", "all"]).contains("## Fields both have"));
    assert!(text(&["doctor", "--help"]).contains("## What it answers"));
    assert!(text(&["--version"]).starts_with(concat!("pr-get ", env!("CARGO_PKG_VERSION"))));
}
