//! A stand-in Forgejo for forge-pull's tests, on 127.0.0.1: the calls
//! forge-pull makes (an owner's repos, a repo's pulls, a pull's reviews,
//! the pull from a head into a base, and creating a pull), with Forgejo's
//! paging (`page`, `limit` capped at 50, `X-Total-Count`), its sign-in
//! rule, its 404, 409 and 422 and their `message`. It records every
//! request's method, target and body, so a test can show what was sent.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use serde_json::{json, Value};

pub const TOKEN: &str = "stand-in-token";
/// Forgejo's MAX_RESPONSE_ITEMS default.
pub const MAX_LIMIT: usize = 50;
/// A branch the stand-in says is not pushed, as Forgejo does with 404.
pub const UNPUSHED: &str = "not-pushed";
/// A repo whose pulls Forgejo fails to list.
pub const BROKEN: &str = "broken";

/// What the stand-in serves of the owner `guild`.
pub struct Data {
    /// Each repo, then its pulls (Forgejo's pull objects, newest first).
    pub repos: Vec<(String, Vec<Value>)>,
    /// Repo, pull number, then its reviews, oldest first.
    pub reviews: Vec<(String, u64, Vec<Value>)>,
}

/// One request: its method, target and body.
#[derive(Clone, Debug)]
pub struct Request {
    pub method: String,
    pub target: String,
    pub body: String,
}

pub struct Fake {
    pub url: String,
    port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    requests: Arc<Mutex<Vec<Request>>>,
}

impl Fake {
    pub fn new(data: Data) -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let port = listener.local_addr().unwrap().port();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = requests.clone();
        let url = format!("http://127.0.0.1:{port}");
        let base = url.clone();
        let thread = std::thread::spawn(move || {
            let mut data = data;
            for stream in listener.incoming() {
                if stopping.load(Ordering::SeqCst) {
                    return;
                }
                if let Ok(stream) = stream {
                    serve(&mut data, &base, &log, stream);
                }
            }
        });
        Fake {
            url: format!("{url}/"),
            port,
            stop,
            thread: Some(thread),
            requests,
        }
    }

    pub fn requests(&self) -> Vec<Request> {
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
fn page(stream: TcpStream, all: &[Value], q: &[(String, String)]) {
    let num = |k: &str, d: usize| {
        q.iter()
            .find(|(n, _)| n == k)
            .and_then(|(_, v)| v.parse().ok())
            .unwrap_or(d)
    };
    let limit = num("limit", 30).clamp(1, MAX_LIMIT);
    let page = num("page", 1).max(1);
    let chunk: Vec<&Value> = all.iter().skip((page - 1) * limit).take(limit).collect();
    send(
        stream,
        200,
        &json!(chunk),
        &[("X-Total-Count", all.len().to_string())],
    );
}

fn message(stream: TcpStream, code: u16, m: &str) {
    send(
        stream,
        code,
        &json!({"message": m, "url": "http://x/api/swagger"}),
        &[],
    );
}

fn serve(data: &mut Data, base: &str, log: &Mutex<Vec<Request>>, stream: TcpStream) {
    let mut reader = BufReader::new(&stream);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or("").to_string();
    let target = parts.next().unwrap_or("/").to_string();
    let mut auth = None;
    let mut length = 0usize;
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).unwrap_or(0) == 0 || h.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            let k = k.trim();
            if k.eq_ignore_ascii_case("authorization") {
                auth = Some(v.trim().to_string());
            } else if k.eq_ignore_ascii_case("content-length") {
                length = v.trim().parse().unwrap_or(0);
            }
        }
    }
    let mut raw = vec![0; length];
    let _ = reader.read_exact(&mut raw);
    let body = String::from_utf8_lossy(&raw).to_string();
    log.lock().unwrap().push(Request {
        method: method.clone(),
        target: target.clone(),
        body: body.clone(),
    });
    if auth.as_deref() != Some(&format!("token {TOKEN}")) {
        return message(stream, 401, "Only signed in user is allowed to call APIs.");
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
    let not_found = "The target couldn't be found.";
    let path = unquote(path);
    if method == "GET" && path == "/api/v1/users/guild/repos" {
        let all: Vec<Value> = data
            .repos
            .iter()
            .enumerate()
            .map(|(i, (name, _))| {
                json!({"id": i + 1, "name": name, "full_name": format!("guild/{name}"),
                       "has_pull_requests": name != "tasks"})
            })
            .collect();
        return page(stream, &all, &q);
    }
    let Some(rest) = path.strip_prefix("/api/v1/repos/guild/") else {
        return message(stream, 404, not_found);
    };
    let (repo, rest) = rest.split_once('/').unwrap_or((rest, ""));
    let Some(i) = data.repos.iter().position(|(n, _)| n == repo) else {
        return message(stream, 404, not_found);
    };
    if repo == "tasks" && rest.starts_with("pulls") {
        return message(stream, 404, not_found);
    }
    match (method.as_str(), rest) {
        ("GET", "pulls") => {
            if repo == BROKEN {
                return message(stream, 500, "database is locked");
            }
            let state = q
                .iter()
                .find(|(k, _)| k == "state")
                .map(|(_, v)| v.as_str());
            let all: Vec<Value> = data.repos[i]
                .1
                .iter()
                .filter(|p| state == Some("all") || p["state"] == state.unwrap_or("open"))
                .cloned()
                .collect();
            page(stream, &all, &q)
        }
        ("POST", "pulls") => {
            let Ok(req) = serde_json::from_str::<Value>(&body) else {
                return message(stream, 422, "[]: invalid JSON");
            };
            let (head, base_b) = (
                req["head"].as_str().unwrap_or(""),
                req["base"].as_str().unwrap_or(""),
            );
            if head == UNPUSHED {
                return message(stream, 404, "The target couldn't be found.");
            }
            if head == base_b {
                return message(
                    stream,
                    422,
                    "Invalid PullRequest: There are no changes between the head and the base",
                );
            }
            let exists = data.repos[i].1.iter().any(|p| {
                p["state"] == "open" && p["head"]["ref"] == head && p["base"]["ref"] == base_b
            });
            if exists {
                return message(stream, 409, "pull request already exists for these targets");
            }
            let number = data
                .repos
                .iter()
                .map(|(_, ps)| ps.len() as u64)
                .sum::<u64>()
                + 100;
            let pull = json!({
                "id": 9000 + number,
                "number": number,
                "state": "open",
                "merged": false,
                "title": req["title"],
                "body": req["body"],
                "html_url": format!("{base}/guild/{repo}/pulls/{number}"),
                "head": {"ref": head, "sha": "0000000000000000000000000000000000000001"},
                "base": {"ref": base_b},
            });
            data.repos[i].1.insert(0, pull.clone());
            send(stream, 201, &pull, &[])
        }
        ("GET", rest) => {
            let Some(rest) = rest.strip_prefix("pulls/") else {
                return message(stream, 404, not_found);
            };
            if let Some(n) = rest.strip_suffix("/reviews") {
                let n: u64 = n.parse().unwrap_or(0);
                let all = data
                    .reviews
                    .iter()
                    .find(|(r, m, _)| r == repo && *m == n)
                    .map(|(_, _, rs)| rs.clone())
                    .unwrap_or_default();
                return page(stream, &all, &q);
            }
            // pulls/{base}/{head}: the newest pull from head into base.
            let Some((b, head)) = rest.split_once('/') else {
                return message(stream, 404, not_found);
            };
            match data.repos[i]
                .1
                .iter()
                .find(|p| p["head"]["ref"] == head && p["base"]["ref"] == b)
            {
                Some(p) => send(stream, 200, p, &[]),
                None => message(stream, 404, not_found),
            }
        }
        _ => message(stream, 405, "method not allowed"),
    }
}
