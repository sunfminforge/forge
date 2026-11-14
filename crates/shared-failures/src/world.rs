//! What shared-failures does to the world: reads Forgejo, read-only, and prints.

use std::io::{Read, Write};
use std::process::ExitCode;
use std::time::Duration;

use serde_json::Value;

use crate::{findings, render, HELP, ORG, PAGE, REPO};

const NAME: &str = "shared-failures";

/// Every page of a Forgejo list at `path`.
fn api(agent: &ureq::Agent, base: &str, token: &str, path: &str) -> Result<Vec<Value>, String> {
    let mut out = Vec::new();
    let mut page = 1;
    loop {
        let sep = if path.contains('?') { "&" } else { "?" };
        let url = format!("{base}/api/v1{path}{sep}limit={PAGE}&page={page}");
        let mut req = agent.get(&url).set("Accept", "application/json");
        if !token.is_empty() {
            req = req.set("Authorization", &format!("token {token}"));
        }
        let resp = req.call().map_err(|e| e.to_string())?;
        let mut body = Vec::new();
        resp.into_reader()
            .read_to_end(&mut body)
            .map_err(|e| e.to_string())?;
        let items = match serde_json::from_slice::<Value>(&body).map_err(|e| e.to_string())? {
            Value::Array(items) => items,
            other => return Err(format!("not a list: {other}")),
        };
        let n = items.len();
        out.extend(items);
        if n < PAGE {
            return Ok(out);
        }
        page += 1;
    }
}

pub fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{HELP}");
        return ExitCode::SUCCESS;
    }
    if let Some(a) = args.first() {
        eprintln!("{NAME}: takes no arguments, not {a:?}: run it as `{NAME}`");
        return ExitCode::from(2);
    }
    let base = std::env::var("GUILD_FORGEJO_URL").unwrap_or_default();
    let base = base.trim_end_matches('/');
    let token = std::env::var("GUILD_FORGEJO_TOKEN").unwrap_or_default();
    if base.is_empty() {
        eprintln!("{NAME}: no GUILD_FORGEJO_URL: nothing read");
        return ExitCode::SUCCESS;
    }
    // No proxy: a detector reaches nothing beyond the Hub.
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(20))
        .build();
    let read = api(
        &agent,
        base,
        &token,
        &format!("/repos/{ORG}/{REPO}/issues?state=all&type=issues"),
    )
    .and_then(|issues| {
        Ok((
            issues,
            api(
                &agent,
                base,
                &token,
                &format!("/repos/{ORG}/{REPO}/issues/comments"),
            )?,
        ))
    });
    let (issues, comments) = match read {
        Ok(lists) => lists,
        Err(e) => {
            eprintln!("{NAME}: cannot read Forgejo at {base}: {e}: nothing read");
            return ExitCode::SUCCESS;
        }
    };
    let out = render(&findings(&issues, &comments));
    let mut stdout = std::io::stdout().lock();
    if stdout
        .write_all(out.as_bytes())
        .and_then(|()| stdout.flush())
        .is_err()
    {
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
