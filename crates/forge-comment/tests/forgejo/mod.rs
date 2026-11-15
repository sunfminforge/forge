//! A stand-in Forgejo for forge-comment's tests, on 127.0.0.1: the two
//! calls forge-comment makes, creating a comment on an issue and editing a
//! comment, with Forgejo's sign-in rule (401), its 403 for a comment
//! another account wrote, its 404 for an issue or comment not there and
//! its 422 for an empty or too long body, each with its `message`. It records every
//! request's method, target, Authorization and body.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use serde_json::{json, Value};

pub const TOKEN: &str = "stand-in-token";
/// The account TOKEN signs in as.
pub const ME: &str = "foreman-1";
/// The longest body the stand-in takes; longer, it answers 422.
pub const MAX_BODY: usize = 1000;

/// What the stand-in serves of the repo guild/tasks.
pub struct Data {
    /// The issue numbers that exist.
    pub issues: Vec<u64>,
    /// Each comment: id, issue number, author's login, body.
    pub comments: Vec<(u64, u64, String, String)>,
}

/// One request: its method, target, Authorization header and body.
#[derive(Clone, Debug)]
pub struct Request {
    pub method: String,
    pub target: String,
    pub auth: Option<String>,
    pub content_type: Option<String>,
    pub body: String,
}

pub struct Fake {
    pub url: String,
    port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    requests: Arc<Mutex<Vec<Request>>>,
    data: Arc<Mutex<Data>>,
}

impl Fake {
    pub fn new(data: Data) -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let port = listener.local_addr().unwrap().port();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let log = requests.clone();
        let data = Arc::new(Mutex::new(data));
        let shared = data.clone();
        let url = format!("http://127.0.0.1:{port}");
        let base = url.clone();
        let thread = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if stopping.load(Ordering::SeqCst) {
                    return;
                }
                if let Ok(stream) = stream {
                    serve(&mut shared.lock().unwrap(), &base, &log, stream);
                }
            }
        });
        Fake {
            url: format!("{url}/"),
            port,
            stop,
            thread: Some(thread),
            requests,
            data,
        }
    }

    pub fn requests(&self) -> Vec<Request> {
        self.requests.lock().unwrap().clone()
    }

    /// The body of comment `id` as the stand-in holds it now.
    pub fn comment_body(&self, id: u64) -> Option<String> {
        let d = self.data.lock().unwrap();
        d.comments.iter().find(|c| c.0 == id).map(|c| c.3.clone())
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

fn send(mut stream: TcpStream, code: u16, body: &Value) {
    let data = serde_json::to_string(body).unwrap();
    let out = format!(
        "HTTP/1.0 {code} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{data}",
        data.len()
    );
    let _ = stream.write_all(out.as_bytes());
    let _ = stream.flush();
}

fn message(stream: TcpStream, code: u16, m: &str) {
    send(
        stream,
        code,
        &json!({"message": m, "url": "http://x/api/swagger"}),
    );
}

fn comment_json(base: &str, c: &(u64, u64, String, String)) -> Value {
    json!({
        "id": c.0,
        "html_url": format!("{base}/guild/tasks/issues/{}#issuecomment-{}", c.1, c.0),
        "issue_url": format!("{base}/guild/tasks/issues/{}", c.1),
        "user": {"login": c.2},
        "body": c.3,
    })
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
    let (mut auth, mut content_type) = (None, None);
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
            } else if k.eq_ignore_ascii_case("content-type") {
                content_type = Some(v.trim().to_string());
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
        auth: auth.clone(),
        content_type,
        body: body.clone(),
    });
    if auth.as_deref() != Some(&format!("token {TOKEN}")) {
        return message(stream, 401, "Only signed in user is allowed to call APIs.");
    }
    let not_found = "The target couldn't be found.";
    let Some(rest) = target.strip_prefix("/api/v1/repos/guild/tasks/issues/") else {
        return message(stream, 404, not_found);
    };
    let text = match serde_json::from_str::<Value>(&body) {
        Ok(v) => v["body"].as_str().unwrap_or("").to_string(),
        Err(_) => return message(stream, 422, "[]: invalid JSON"),
    };
    if text.trim().is_empty() {
        return message(stream, 422, "[Body]: Required");
    }
    if text.len() > MAX_BODY {
        return message(stream, 422, "[Body]: MaxSize");
    }
    match (
        method.as_str(),
        rest.split('/').collect::<Vec<_>>().as_slice(),
    ) {
        ("POST", [n, "comments"]) => {
            let n: u64 = n.parse().unwrap_or(0);
            if !data.issues.contains(&n) {
                return message(stream, 404, not_found);
            }
            let id = data.comments.iter().map(|c| c.0).max().unwrap_or(0) + 1;
            let c = (id, n, ME.to_string(), text);
            data.comments.push(c.clone());
            send(stream, 201, &comment_json(base, &c))
        }
        ("PATCH", ["comments", id]) => {
            let id: u64 = id.parse().unwrap_or(0);
            let Some(c) = data.comments.iter_mut().find(|c| c.0 == id) else {
                return message(stream, 404, not_found);
            };
            if c.2 != ME {
                return send(
                    stream,
                    403,
                    &json!({"message": "", "url": "http://x/api/swagger"}),
                );
            }
            c.3 = text;
            let c = c.clone();
            send(stream, 200, &comment_json(base, &c))
        }
        _ => message(stream, 405, "method not allowed"),
    }
}
