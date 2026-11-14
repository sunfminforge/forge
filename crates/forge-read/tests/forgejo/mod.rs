//! A stand-in Forgejo for forge-read's tests, on 127.0.0.1: the issue,
//! comments, issue-search and pulls calls forge-read makes, with Forgejo's paging
//! (`page`, `limit` capped at 50, `X-Total-Count`), its label match (any
//! of the labels), its sign-in rule and 404. It records every request's
//! method and target, so a test can show forge-read only GETs.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use serde_json::{json, Value};

pub const TOKEN: &str = "stand-in-token";
/// Forgejo's MAX_RESPONSE_ITEMS default.
pub const MAX_LIMIT: usize = 50;
const PREFIX: &str = "/api/v1/repos/guild/tasks/";

/// What the stand-in serves of guild/tasks.
pub struct Data {
    /// Every issue and pull, newest first.
    pub issues: Vec<Value>,
    /// Issue number, then its comments, oldest first.
    pub comments: Vec<(u64, Vec<Value>)>,
    /// Every pull as the pulls endpoint answers it, with head and base, in
    /// the order it is served.
    pub pulls: Vec<Value>,
}

pub struct Fake {
    pub url: String,
    port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    /// Each request as `METHOD target`.
    pub requests: Arc<Mutex<Vec<String>>>,
}

impl Fake {
    pub fn new(data: Data) -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let port = listener.local_addr().unwrap().port();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = requests.clone();
        let thread = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if stopping.load(Ordering::SeqCst) {
                    return;
                }
                if let Ok(stream) = stream {
                    serve(&data, &log, stream);
                }
            }
        });
        Fake {
            url: format!("http://127.0.0.1:{port}/"),
            port,
            stop,
            thread: Some(thread),
            requests,
        }
    }

    pub fn requests(&self) -> Vec<String> {
        self.requests.lock().unwrap().clone()
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn unquote(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                let v =
                    u8::from_str_radix(std::str::from_utf8(&b[i + 1..i + 3]).unwrap(), 16).unwrap();
                out.push(v);
                i += 2;
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8(out).unwrap()
}

fn send(mut stream: TcpStream, code: u16, body: &Value, headers: &[(&str, String)]) {
    let data = serde_json::to_string(body).unwrap();
    let mut out = format!(
        "HTTP/1.0 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        data.len()
    );
    for (k, v) in headers {
        out.push_str(&format!("{k}: {v}\r\n"));
    }
    out.push_str("\r\n");
    out.push_str(&data);
    let _ = stream.write_all(out.as_bytes());
    let _ = stream.flush();
}

/// A page of `all`, as Forgejo pages a list.
fn page(stream: TcpStream, all: &[&Value], q: &[(String, String)]) {
    let num = |k: &str, d: usize| {
        q.iter()
            .find(|(n, _)| n == k)
            .and_then(|(_, v)| v.parse().ok())
            .unwrap_or(d)
    };
    let limit = num("limit", 30).clamp(1, MAX_LIMIT);
    let page = num("page", 1).max(1);
    let chunk: Vec<&Value> = all
        .iter()
        .skip((page - 1) * limit)
        .take(limit)
        .copied()
        .collect();
    send(
        stream,
        200,
        &json!(chunk),
        &[("X-Total-Count", all.len().to_string())],
    );
}

fn serve(data: &Data, log: &Mutex<Vec<String>>, stream: TcpStream) {
    let mut reader = BufReader::new(&stream);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    log.lock().unwrap().push(format!("{method} {target}"));
    let mut auth = None;
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).unwrap_or(0) == 0 || h.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            if k.trim().eq_ignore_ascii_case("authorization") {
                auth = Some(v.trim().to_string());
            }
        }
    }
    let not_found = json!({"message": "The target couldn't be found."});
    if method != "GET" {
        return send(stream, 405, &json!({"message": "not here"}), &[]);
    }
    if auth.as_deref() != Some(&format!("token {TOKEN}")) {
        let body = json!({"message": "Only signed in user is allowed to call APIs."});
        return send(stream, 401, &body, &[]);
    }
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    let q: Vec<(String, String)> = query
        .split('&')
        .filter(|kv| !kv.is_empty())
        .map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            (unquote(k), unquote(v))
        })
        .collect();
    let get = |k: &str| q.iter().find(|(n, _)| n == k).map(|(_, v)| v.as_str());
    let Some(rest) = path.strip_prefix(PREFIX) else {
        return send(stream, 404, &not_found, &[]);
    };
    if rest == "issues" {
        let state = get("state").unwrap_or("open");
        let kind = get("type");
        let labels: Vec<&str> = get("labels")
            .map(|l| l.split(',').filter(|s| !s.is_empty()).collect())
            .unwrap_or_default();
        let all: Vec<&Value> = data
            .issues
            .iter()
            .filter(|i| state == "all" || i["state"] == state)
            .filter(|i| match kind {
                Some("issues") => i.get("pull_request").is_none_or(Value::is_null),
                Some("pulls") => i.get("pull_request").is_some_and(|p| !p.is_null()),
                _ => true,
            })
            .filter(|i| {
                labels.is_empty()
                    || i["labels"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|l| labels.contains(&l["name"].as_str().unwrap()))
            })
            .collect();
        return page(stream, &all, &q);
    }
    if rest == "pulls" {
        // Forgejo's pulls endpoint: state open, closed (merged included) or
        // all; it takes labels only by id, and has no head filter.
        let state = get("state").unwrap_or("open");
        let all: Vec<&Value> = data
            .pulls
            .iter()
            .filter(|p| state == "all" || p["state"] == state)
            .collect();
        return page(stream, &all, &q);
    }
    let Some(rest) = rest.strip_prefix("issues/") else {
        return send(stream, 404, &not_found, &[]);
    };
    let (number, tail) = rest.split_once('/').unwrap_or((rest, ""));
    let Ok(number) = number.parse::<u64>() else {
        return send(stream, 404, &not_found, &[]);
    };
    let Some(issue) = data.issues.iter().find(|i| i["number"] == number) else {
        return send(stream, 404, &not_found, &[]);
    };
    match tail {
        "" => send(stream, 200, issue, &[]),
        "comments" => {
            let all: Vec<&Value> = data
                .comments
                .iter()
                .find(|(n, _)| *n == number)
                .map(|(_, c)| c.iter().collect())
                .unwrap_or_default();
            page(stream, &all, &q)
        }
        _ => send(stream, 404, &not_found, &[]),
    }
}
