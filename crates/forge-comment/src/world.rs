//! What forge-comment does to the world: reads its arguments, the body
//! file and the environment, sends the one request `request` decided, and
//! prints. With --dry-run it reads no environment and sends nothing.

use std::io::{Read, Write};
use std::process::ExitCode;
use std::time::Duration;

use serde_json::Value;

use crate::{
    error_message, fix, missing, parse, render_comment, render_dry_run, request, Asked, Body,
    Comment, Request, Target, EXIT_FAIL, EXIT_USAGE, HELP, NAME, USAGE,
};

/// Why a run failed: said on stderr with its fix, and the exit code.
struct Fail(u8, String);

/// What went wrong reaching Forgejo, without the URL ureq puts first.
fn cause(t: &ureq::Transport) -> String {
    let mut out = t.kind().to_string();
    if let Some(m) = t.message() {
        out = format!("{out}: {m}");
    }
    if let Some(source) = std::error::Error::source(t) {
        out = format!("{out}: {source}");
    }
    out
}

fn body(b: &Body) -> Result<String, Fail> {
    let text = match b {
        Body::Text(t) => t.clone(),
        Body::File(f) => std::fs::read_to_string(f).map_err(|e| {
            Fail(
                EXIT_FAIL,
                format!("the body file {f} could not be read: {e}: write the comment's body to that file, then run it again"),
            )
        })?,
    };
    if text.trim().is_empty() {
        return Err(Fail(
            EXIT_FAIL,
            "the comment's body is empty: give the text to say with --text or in the --body-file"
                .into(),
        ));
    }
    Ok(text)
}

/// Sends the request to Forgejo as the instance, and reads the comment it
/// answers.
fn send(r: &Request, target: &Target) -> Result<Value, Fail> {
    let var = |k: &str| std::env::var(k).unwrap_or_default();
    let (base, token) = (var("GUILD_FORGEJO_URL"), var("GUILD_FORGEJO_TOKEN"));
    let unset = missing(&base, &token);
    if !unset.is_empty() {
        return Err(Fail(
            EXIT_USAGE,
            format!(
                "{} not set: run forge-comment inside a Guild instance, which is given its Forgejo's URL in GUILD_FORGEJO_URL and its own account's token in GUILD_FORGEJO_TOKEN; or add --dry-run to see the request without sending it",
                unset.join(" and ")
            ),
        ));
    }
    let url = format!("{}/api/v1/{}", base.trim().trim_end_matches('/'), r.path);
    let shown = format!("{} /api/v1/{}", r.method, r.path);
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(30))
        .build();
    let sent = agent
        .request(r.method, &url)
        .set("Accept", "application/json")
        .set("Authorization", &format!("token {}", token.trim()))
        .set("Content-Type", "application/json")
        .send_string(&r.json.to_string());
    let resp = match sent {
        Ok(resp) => resp,
        Err(ureq::Error::Status(code, resp)) => {
            let message = error_message(&resp.into_string().unwrap_or_default());
            return Err(Fail(
                EXIT_FAIL,
                format!(
                    "Forgejo answered {code} to {shown} for {target}: {message}: {}",
                    fix(code, target)
                ),
            ));
        }
        Err(ureq::Error::Transport(t)) => {
            return Err(Fail(
                EXIT_FAIL,
                format!(
                    "Forgejo could not be reached for {shown}: {}: check GUILD_FORGEJO_URL, then run it again",
                    cause(&t)
                ),
            ))
        }
    };
    let mut raw = Vec::new();
    resp.into_reader()
        .read_to_end(&mut raw)
        .map_err(|e| e.to_string())
        .and_then(|_| serde_json::from_slice::<Value>(&raw).map_err(|e| e.to_string()))
        .map_err(|e| {
            Fail(
                EXIT_FAIL,
                format!("Forgejo's answer to {shown} for {target} could not be read: {e}: see the comment with forge-read before running it again"),
            )
        })
}

fn comment(c: &Comment) -> Result<String, Fail> {
    let text = body(&c.body)?;
    let r = request(&c.target, &text);
    if c.dry_run {
        eprintln!("{NAME}: --dry-run: nothing was sent");
        return Ok(render_dry_run(&r));
    }
    let answer = send(&r, &c.target)?;
    Ok(render_comment(&answer))
}

pub fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let c = match parse(&args) {
        Ok(Asked::Help) => {
            print!("{HELP}");
            return ExitCode::SUCCESS;
        }
        Ok(Asked::Version) => {
            println!("{NAME} {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Ok(Asked::Comment(c)) => c,
        Err(e) => {
            eprintln!("{USAGE}\n{NAME}: error: {e}; see {NAME} --help");
            return ExitCode::from(EXIT_USAGE);
        }
    };
    let out = match comment(&c) {
        Ok(out) => out,
        Err(Fail(code, e)) => {
            eprintln!("{NAME}: {e}");
            return ExitCode::from(code);
        }
    };
    let mut stdout = std::io::stdout().lock();
    if stdout
        .write_all(out.as_bytes())
        .and_then(|()| stdout.flush())
        .is_err()
    {
        return ExitCode::from(EXIT_FAIL);
    }
    ExitCode::SUCCESS
}
