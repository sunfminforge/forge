//! What `pr-get` decides, with no I/O: what each argument and variable read
//! means, which URL to ask, what the forge's answer means, and which fields
//! to print. The effect crate reads the world and passes it in.

use agent_help::{Help, Table};
use flux_rs::attrs::*;
use serde_json::{Map, Value};

/// Every refusal of `pr-get`, by its code: what it means and what fixes it. The answer's `fix` names the command, flag or variable to change.
#[derive(Help, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// A command line `pr-get` cannot read, such as an unknown flag or a missing argument: the fix names the `--help` that shows how to write it.
    Usage,
    /// The repository is not `<owner>/<name>`, each part letters, digits, `-`, `_` or `.`.
    BadRepo,
    /// The pull request's number is not a whole number from 1.
    BadNumber,
    /// A `--field` is empty, or a part of its path between `.`s is.
    BadField,
    /// No forge to ask: neither `--api` nor `GUILD_FORGEJO_URL` is set.
    NoApi,
    /// `--api` or `GUILD_FORGEJO_URL` is not an `http://` or `https://` URL with no spaces, quotes or backslashes.
    BadApi,
    /// The variable that holds the token, `GUILD_FORGEJO_TOKEN` or the one `--token-env` names, is unset or empty.
    NoToken,
    /// The token holds a space, quote, backslash or control character, which no token has.
    BadToken,
    /// No `curl` on the PATH, which `pr-get` asks the forge through.
    NoCurl,
    /// `curl` could not reach the forge (a name not resolved, a refused connection, TLS, a timeout): the message holds what curl said.
    Unreachable,
    /// The forge refused the token (HTTP 401 or 403).
    Unauthorized,
    /// The forge has no such pull request (HTTP 404); a private repository the token cannot read is answered the same.
    NotFound,
    /// The forge answered another status than success, 401, 403 or 404.
    HttpError,
    /// The forge's answer is not a JSON object, as when `--api` names a web page rather than the API's root.
    NotJson,
    /// A `--field` names a key the pull request does not have: the fix lists the keys it has.
    UnknownField,
}

/// One fault of a refusal: its code, what is wrong, and what fixes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fault {
    pub code: Refusal,
    pub message: String,
    pub fix: String,
}

impl Fault {
    pub fn new(code: Refusal, message: impl Into<String>, fix: impl Into<String>) -> Fault {
        Fault {
            code,
            message: message.into(),
            fix: fix.into(),
        }
    }

    /// The code as the answer prints it, such as `not_found`.
    pub fn name(&self) -> &'static str {
        self.code.entry().name
    }

    /// `{"code", "message", "fix"}`.
    pub fn to_json(&self) -> Value {
        object(vec![
            ("code", Value::from(self.name())),
            ("message", Value::from(self.message.as_str())),
            ("fix", Value::from(self.fix.as_str())),
        ])
    }
}

/// A JSON object of `pairs`, in their order.
pub fn object(pairs: Vec<(&str, Value)>) -> Value {
    let mut map = Map::new();
    for (key, value) in pairs {
        map.insert(key.to_string(), value);
    }
    Value::Object(map)
}

/// The answer of a refusal: `{"error", "faults"}`, `error` summing up the first fault.
pub fn refusal(faults: &[Fault]) -> Value {
    let summary = match faults.first() {
        None => "refused".to_string(),
        Some(first) if faults.len() == 1 => first.message.clone(),
        Some(first) => format!(
            "{} (and {} more)",
            first.message,
            faults.len().saturating_sub(1)
        ),
    };
    object(vec![
        ("error", Value::from(summary)),
        (
            "faults",
            Value::Array(faults.iter().map(Fault::to_json).collect()),
        ),
    ])
}

/// A character of an owner's or a repository's name.
fn name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.'
}

fn name_part(part: &str) -> bool {
    !part.is_empty() && part != "." && part != ".." && part.chars().all(name_char)
}

/// A repository, `<owner>/<name>`: made only by [`Repo::parse`], so it holds
/// nothing a URL's path would read otherwise.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Repo {
    owner: String,
    name: String,
}

impl Repo {
    pub fn parse(text: &str) -> Result<Repo, Fault> {
        match text.split_once('/') {
            Some((owner, name)) if name_part(owner) && name_part(name) => Ok(Repo {
                owner: owner.to_string(),
                name: name.to_string(),
            }),
            _ => Err(Fault::new(
                Refusal::BadRepo,
                format!("`{text}` is not a repository, `<owner>/<name>`"),
                "write the repository as `<owner>/<name>`, such as `guild/forge`",
            )),
        }
    }

    /// `<owner>/<name>`.
    pub fn full(&self) -> String {
        format!("{}/{}", self.owner, self.name)
    }
}

/// A pull request's number, from 1: made only by [`Number::parse`].
#[refined_by(n: int)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Number {
    #[field({u64[n] | n >= 1})]
    n: u64,
}

impl Number {
    #[spec(fn(&str) -> Result<Number, Fault>)]
    pub fn parse(text: &str) -> Result<Number, Fault> {
        let digits = !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
        match text.parse::<u64>() {
            Ok(n) if digits && n >= 1 => Ok(Number { n }),
            _ => Err(Fault::new(
                Refusal::BadNumber,
                format!("`{text}` is not a pull request's number, a whole number from 1"),
                "give the pull request's number, such as `pr-get guild/forge 1`",
            )),
        }
    }

    #[spec(fn(&Number[@n]) -> u64{v: v == n && v >= 1})]
    pub fn get(&self) -> u64 {
        self.n
    }
}

/// A `--field`: a key of the pull request, or keys joined by `.` into the
/// objects it holds, such as `head.ref`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    text: String,
    path: Vec<String>,
}

impl Field {
    pub fn parse(text: &str) -> Result<Field, Fault> {
        let path: Vec<String> = text.split('.').map(str::to_string).collect();
        if text.is_empty() || path.iter().any(String::is_empty) {
            return Err(Fault::new(
                Refusal::BadField,
                format!("`{text}` is not a field: a key, or keys joined by `.`"),
                "write each field as a key of the pull request, such as `title`, or keys joined by `.`, such as `head.ref`; separate fields with `,`",
            ));
        }
        Ok(Field {
            text: text.to_string(),
            path,
        })
    }

    pub fn text(&self) -> &str {
        &self.text
    }
}

/// Every `--field` given, read; every fault of every one.
pub fn fields(texts: &[String]) -> Result<Vec<Field>, Vec<Fault>> {
    let mut out = Vec::new();
    let mut faults = Vec::new();
    for text in texts {
        match Field::parse(text) {
            Ok(field) => out.push(field),
            Err(fault) => faults.push(fault),
        }
    }
    if faults.is_empty() {
        Ok(out)
    } else {
        Err(faults)
    }
}

/// The variable that names Guild's Forgejo, as `https://<host>/`.
pub const FORGEJO_URL: &str = "GUILD_FORGEJO_URL";
/// The variable that holds the token, unless `--token-env` names another.
pub const FORGEJO_TOKEN: &str = "GUILD_FORGEJO_TOKEN";
/// Where a Forgejo's API is, below its URL.
pub const FORGEJO_API: &str = "api/v1";

/// A character that may not stand in a URL or a header `pr-get` hands curl.
fn unsafe_char(c: char) -> bool {
    c.is_whitespace() || c.is_control() || c == '"' || c == '\\'
}

/// The API's root, without a trailing `/`: `--api` as given, else
/// `GUILD_FORGEJO_URL` with `api/v1` below it.
pub fn api_root(flag: Option<&str>, forgejo_url: Option<&str>) -> Result<String, Fault> {
    let (root, from) = match (flag, forgejo_url) {
        (Some(api), _) => (api.trim_end_matches('/').to_string(), "--api"),
        (None, Some(url)) if !url.is_empty() => (
            format!("{}/{FORGEJO_API}", url.trim_end_matches('/')),
            FORGEJO_URL,
        ),
        _ => {
            return Err(Fault::new(
                Refusal::NoApi,
                format!("no forge to ask: neither --api nor {FORGEJO_URL} is set"),
                format!(
                    "export {FORGEJO_URL}=https://<forgejo host>/, or pass `--api https://<forgejo host>/api/v1` (`--api https://api.github.com` for GitHub)"
                ),
            ));
        }
    };
    let scheme = root.starts_with("https://") || root.starts_with("http://");
    if !scheme || root.chars().any(unsafe_char) {
        return Err(Fault::new(
            Refusal::BadApi,
            format!(
                "{from}, `{root}`, is not an http(s) URL with no spaces, quotes or backslashes"
            ),
            format!(
                "set {from} to the forge's URL, such as `https://<forgejo host>/api/v1` for --api"
            ),
        ));
    }
    Ok(root)
}

/// A token, made only by [`token`]: nothing in it can break the header it
/// goes in, and its `Debug` never shows it.
#[derive(Clone, PartialEq, Eq)]
pub struct Token(String);

impl Token {
    pub fn secret(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(..)")
    }
}

/// The token variable `name` holds, `value` as read.
pub fn token(name: &str, value: Option<&str>) -> Result<Token, Fault> {
    match value {
        None | Some("") => Err(Fault::new(
            Refusal::NoToken,
            format!("{name} is unset or empty"),
            format!(
                "export {name}=<a token that may read the repository>, or name the variable that holds one with `--token-env <NAME>`"
            ),
        )),
        Some(text) if text.chars().any(unsafe_char) => Err(Fault::new(
            Refusal::BadToken,
            format!("{name} holds a space, quote, backslash or control character"),
            format!(
                "set {name} to the token alone, with no spaces, quotes, backslashes or newlines"
            ),
        )),
        Some(text) => Ok(Token(text.to_string())),
    }
}

/// The URL of pull request `number` of `repo` under API root `api`.
pub fn pull_url(api: &str, repo: &Repo, number: Number) -> String {
    format!(
        "{api}/repos/{}/{}/pulls/{}",
        repo.owner,
        repo.name,
        number.get()
    )
}

/// An HTTP status, from 100 to 599: made only by [`Status::parse`].
#[refined_by(s: int)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    #[field({u16[s] | 100 <= s && s <= 599})]
    code: u16,
}

impl Status {
    #[spec(fn(&str) -> Option<Status>)]
    pub fn parse(text: &str) -> Option<Status> {
        match text.trim().parse::<u16>() {
            Ok(code @ 100..=599) => Some(Status { code }),
            _ => None,
        }
    }

    #[spec(fn(&Status[@s]) -> u16{v: v == s && 100 <= v && v <= 599})]
    pub fn code(&self) -> u16 {
        self.code
    }
}

/// curl's stdout, the body and then, on its last line, the status
/// `write-out` adds: the two apart, or `None` when it is not that.
pub fn split_response(out: &str) -> Option<(Status, &str)> {
    let (body, status) = out.rsplit_once('\n')?;
    Some((Status::parse(status)?, body))
}

/// The `message` a forge puts in an error's JSON body, when it has one.
fn forge_message(body: &str) -> String {
    match serde_json::from_str::<Value>(body) {
        Ok(Value::Object(map)) => match map.get("message") {
            Some(Value::String(text)) => format!(": {text}"),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

/// What the request named, for a refusal to say.
pub struct Asked<'a> {
    pub api: &'a str,
    pub repo: &'a Repo,
    pub number: Number,
    pub token_env: &'a str,
}

/// What the forge's answer means: the pull request, or the refusal.
pub fn interpret(
    status: Status,
    body: &str,
    asked: &Asked<'_>,
) -> Result<Map<String, Value>, Fault> {
    let code = status.code();
    let which = format!("{} pull request {}", asked.repo.full(), asked.number.get());
    if (200..=299).contains(&code) {
        return match serde_json::from_str::<Value>(body) {
            Ok(Value::Object(pull)) => Ok(pull),
            _ => Err(Fault::new(
                Refusal::NotJson,
                format!("{} answered {which} with no JSON object", asked.api),
                "pass --api the API's root, `https://<forgejo host>/api/v1` for Forgejo or `https://api.github.com` for GitHub, not the web site's",
            )),
        };
    }
    let said = forge_message(body);
    Err(match code {
        401 | 403 => Fault::new(
            Refusal::Unauthorized,
            format!(
                "{} refused the token in {} (HTTP {code}){said}",
                asked.api, asked.token_env
            ),
            format!(
                "export {} as a token that may read {}, then run the command again",
                asked.token_env,
                asked.repo.full()
            ),
        ),
        404 => Fault::new(
            Refusal::NotFound,
            format!("{which} not found (HTTP 404){said}"),
            format!(
                "check the repository and the number; a private repository the token in {} cannot read is answered not found too",
                asked.token_env
            ),
        ),
        _ => Fault::new(
            Refusal::HttpError,
            format!("{} answered {which} with HTTP {code}{said}", asked.api),
            "run the command again; if it answers the same, run `pr-get doctor` and check the forge",
        ),
    })
}

/// The value at `path` in `value`, if every key of it is there.
fn at<'a>(value: &'a Value, path: &[String]) -> Option<&'a Value> {
    let mut here = value;
    for key in path {
        here = here.as_object()?.get(key)?;
    }
    Some(here)
}

/// What is printed of `pull`: all of it with no fields, else one key per
/// field, as written, in the order given, each once; a refusal naming every
/// field it does not have.
pub fn select(pull: Map<String, Value>, fields: &[Field]) -> Result<Value, Vec<Fault>> {
    if fields.is_empty() {
        return Ok(Value::Object(pull));
    }
    let whole = Value::Object(pull);
    let mut out = Map::new();
    let mut faults = Vec::new();
    for field in fields {
        match at(&whole, &field.path) {
            Some(value) => {
                out.insert(field.text.clone(), value.clone());
            }
            None => faults.push(Fault::new(
                Refusal::UnknownField,
                format!("the pull request has no field `{}`", field.text),
                format!(
                    "drop `{}` from --field, or choose among the keys the pull request has: {}",
                    field.text,
                    keys(&whole, &field.path)
                ),
            )),
        }
    }
    if faults.is_empty() {
        Ok(Value::Object(out))
    } else {
        Err(faults)
    }
}

/// The keys of the deepest object `path` reaches, written as fields.
fn keys(whole: &Value, path: &[String]) -> String {
    let mut here = whole;
    let mut prefix = String::new();
    for key in path {
        match here.as_object().and_then(|map| map.get(key)) {
            Some(next) if next.is_object() => {
                here = next;
                prefix.push_str(key);
                prefix.push('.');
            }
            _ => break,
        }
    }
    match here.as_object() {
        Some(map) => map
            .keys()
            .map(|k| format!("`{prefix}{k}`"))
            .collect::<Vec<_>>()
            .join(", "),
        None => String::new(),
    }
}

/// One finding of `pr-get doctor`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub finding: String,
    pub fix: String,
}

/// What `pr-get doctor` finds wrong, from what the effect crate read.
pub fn findings(
    api: &Result<String, Fault>,
    token: &Result<Token, Fault>,
    curl: bool,
) -> Vec<Finding> {
    let mut out = Vec::new();
    for fault in [api.as_ref().err(), token.as_ref().err()]
        .into_iter()
        .flatten()
    {
        out.push(Finding {
            finding: fault.message.clone(),
            fix: fault.fix.clone(),
        });
    }
    if !curl {
        out.push(no_curl_finding());
    }
    out
}

fn no_curl_finding() -> Finding {
    let fault = no_curl();
    Finding {
        finding: fault.message,
        fix: fault.fix,
    }
}

/// The refusal of a PATH with no `curl` on it.
pub fn no_curl() -> Fault {
    Fault::new(
        Refusal::NoCurl,
        "no `curl` on the PATH",
        "put curl on the PATH: macOS has it at /usr/bin/curl, `export PATH=/usr/bin:$PATH`",
    )
}

/// The refusal of a curl that could not reach the forge.
pub fn unreachable(api: &str, said: &str) -> Fault {
    Fault::new(
        Refusal::Unreachable,
        format!("curl could not reach {api}: {}", said.trim()),
        "check that the forge is reachable from here with `pr-get doctor`, then run the command again",
    )
}
