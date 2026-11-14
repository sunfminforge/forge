//! A fixture Forgejo for a detector's tests: the few API calls a detector
//! makes on lore, answered on 127.0.0.1 from a local git repository, with
//! Forgejo's paging and sign-in rule; and the lore repository, built commit
//! by commit, each at a given time. The same in each detector that reads
//! lore.

use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

use base64::Engine;
use serde_json::{json, Value};

pub const TOKEN: &str = "fixture-token";
/// Small, so a detector's tests show it reads every page of a tree.
pub const TREE_PAGE_MAX: usize = 3;
const PREFIX: &str = "/api/v1/repos/guild/lore/";

/// `git -C repo args…`, its stdout; `None` when git fails.
fn git(repo: &Path, args: &[&str]) -> Option<Vec<u8>> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .expect("git runs");
    out.status.success().then_some(out.stdout)
}

/// A scratch directory of a test, removed when dropped.
pub struct Scratch(pub PathBuf);

impl Scratch {
    pub fn new(name: &str) -> Scratch {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let dir = std::env::temp_dir().join(format!("{name}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A lore repository built commit by commit for a test.
pub struct LoreRepo {
    pub path: PathBuf,
}

impl LoreRepo {
    pub fn new(path: PathBuf) -> LoreRepo {
        let ok = Command::new("git")
            .args(["init", "-q", "-b", "main"])
            .arg(&path)
            .status()
            .unwrap();
        assert!(ok.success(), "git init");
        LoreRepo { path }
    }

    fn env(at: f64) -> Vec<(&'static str, String)> {
        let stamp = format!("@{} +0000", at as i64);
        vec![
            ("GIT_AUTHOR_NAME", "fixture".into()),
            ("GIT_AUTHOR_EMAIL", "fixture@localhost".into()),
            ("GIT_COMMITTER_NAME", "fixture".into()),
            ("GIT_COMMITTER_EMAIL", "fixture@localhost".into()),
            ("GIT_AUTHOR_DATE", stamp.clone()),
            ("GIT_COMMITTER_DATE", stamp),
            ("GIT_CONFIG_GLOBAL", "/dev/null".into()),
            ("GIT_CONFIG_NOSYSTEM", "1".into()),
        ]
    }

    fn git_at(&self, at: f64, args: &[&str]) {
        let out = Command::new("git")
            .arg("-C")
            .arg(&self.path)
            .args(args)
            .envs(Self::env(at))
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// files: path, then its text, or `None` to remove it; at: the Unix time
    /// of the commit.
    pub fn commit(&self, files: &[(&str, Option<&str>)], message: &str, at: f64, branch: &str) {
        if git(&self.path, &["rev-parse", "-q", "--verify", "HEAD"]).is_some() {
            git(&self.path, &["checkout", "-q", branch]).expect("git checkout");
        }
        for (p, text) in files {
            match text {
                None => {
                    git(&self.path, &["rm", "-q", p]).expect("git rm");
                }
                Some(text) => {
                    let full = self.path.join(p);
                    std::fs::create_dir_all(full.parent().unwrap()).unwrap();
                    std::fs::write(&full, text).unwrap();
                    git(&self.path, &["add", p]).expect("git add");
                }
            }
        }
        self.git_at(at, &["commit", "-q", "--allow-empty", "-m", message]);
    }

    pub fn branch(&self, name: &str) {
        git(&self.path, &["branch", name, "main"]).expect("git branch");
    }

    pub fn merge(&self, branch: &str, message: &str, at: f64) {
        git(&self.path, &["checkout", "-q", "main"]).expect("git checkout");
        self.git_at(at, &["merge", "-q", "--no-ff", "-m", message, branch]);
    }
}

/// Forgejo, serving lore from a repository on disk; stopped when dropped.
pub struct Fake {
    pub url: String,
    port: u16,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Fake {
    pub fn new(repo: &Path) -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let port = listener.local_addr().unwrap().port();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let repo = repo.to_path_buf();
        let thread = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if stopping.load(Ordering::SeqCst) {
                    return;
                }
                if let Ok(stream) = stream {
                    serve(&repo, stream);
                }
            }
        });
        Fake {
            url: format!("http://127.0.0.1:{port}"),
            port,
            stop,
            thread: Some(thread),
        }
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

/// `%XX` and `+` of a query, decoded.
fn unquote(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < b.len() => {
                let hex = std::str::from_utf8(&b[i + 1..i + 3]).ok();
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(v) => {
                        out.push(v);
                        i += 2;
                    }
                    None => out.push(b'%'),
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
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

fn serve(repo: &Path, stream: TcpStream) {
    let mut reader = BufReader::new(&stream);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let target = line.split_whitespace().nth(1).unwrap_or("/").to_string();
    let mut headers = HashMap::new();
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).unwrap_or(0) == 0 || h.trim().is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
        }
    }
    let not_found = json!({"message": "not found"});
    if headers.get("authorization").map(String::as_str) != Some(&format!("token {TOKEN}")) {
        let body = json!({"message": "Only signed in user is allowed to call APIs."});
        return send(stream, 401, &body, &[]);
    }
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    let q: HashMap<String, String> = query
        .split('&')
        .filter(|kv| !kv.is_empty())
        .map(|kv| {
            let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
            (unquote(k), unquote(v))
        })
        .collect();
    let num = |k: &str, d: usize| q.get(k).and_then(|v| v.parse().ok()).unwrap_or(d);
    let Some(rest) = path.strip_prefix(PREFIX) else {
        return send(stream, 404, &not_found, &[]);
    };
    if rest == "commits" {
        let sha = q.get("sha").map(String::as_str).unwrap_or("main");
        let Some(out) = git(repo, &["log", "--format=%H%x00%P%x00%cI%x00%B%x01", sha]) else {
            return send(stream, 404, &not_found, &[]);
        };
        let all: Vec<Value> = String::from_utf8(out)
            .unwrap()
            .split('\x01')
            .map(|r| r.trim_matches('\n'))
            .filter(|r| !r.is_empty())
            .map(|r| {
                let f: Vec<&str> = r.split('\x00').collect();
                let parents: Vec<Value> =
                    f[1].split_whitespace().map(|p| json!({"sha": p})).collect();
                json!({"sha": f[0], "created": f[2],
                       "commit": {"message": f[3], "committer": {"date": f[2]}},
                       "parents": parents})
            })
            .collect();
        let (limit, page) = (num("limit", 30), num("page", 1));
        let chunk: Vec<&Value> = all.iter().skip((page - 1) * limit).take(limit).collect();
        let more = page * limit < all.len();
        let headers = [
            ("X-Total", all.len().to_string()),
            ("X-HasMore", more.to_string()),
        ];
        return send(stream, 200, &json!(chunk), &headers);
    }
    if let Some(git_ref) = rest.strip_prefix("git/trees/") {
        let Some(sha) = git(repo, &["rev-parse", &format!("{git_ref}^{{tree}}")]) else {
            return send(stream, 404, &not_found, &[]);
        };
        let sha = String::from_utf8(sha).unwrap().trim().to_string();
        let listing = git(repo, &["ls-tree", "-r", "-t", "-l", &sha]).unwrap();
        let entries: Vec<Value> = String::from_utf8(listing)
            .unwrap()
            .lines()
            .map(|line| {
                let (meta, path) = line.split_once('\t').unwrap();
                let m: Vec<&str> = meta.split_whitespace().collect();
                let size: u64 = if m[3] == "-" {
                    0
                } else {
                    m[3].parse().unwrap()
                };
                json!({"path": path, "type": m[1], "sha": m[2], "size": size})
            })
            .collect();
        let per = num("per_page", TREE_PAGE_MAX).min(TREE_PAGE_MAX);
        let page = num("page", 1);
        let chunk: Vec<&Value> = entries.iter().skip((page - 1) * per).take(per).collect();
        let body = json!({"sha": sha, "tree": chunk, "truncated": page * per < entries.len(),
                          "page": page, "total_count": entries.len()});
        return send(stream, 200, &body, &[]);
    }
    if let Some(sha) = rest.strip_prefix("git/blobs/") {
        let Some(raw) = git(repo, &["cat-file", "blob", sha]) else {
            return send(stream, 404, &not_found, &[]);
        };
        let content = base64::engine::general_purpose::STANDARD.encode(&raw);
        let body = json!({"sha": sha, "size": raw.len(), "encoding": "base64", "content": content});
        return send(stream, 200, &body, &[]);
    }
    send(stream, 404, &not_found, &[])
}

/// A finding as printed: its role, saw and evidence.
pub type Finding = BTreeMap<String, String>;

/// The `[[finding]]` tables of a detector's output, failing if it printed
/// anything else.
pub fn parse_findings(out: &str) -> Vec<Finding> {
    let mut findings: Vec<Finding> = Vec::new();
    for line in out.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if line == "[[finding]]" {
            findings.push(Finding::new());
            continue;
        }
        let cur = findings
            .last_mut()
            .unwrap_or_else(|| panic!("output outside a [[finding]]: {line:?}"));
        let (key, value) = line
            .split_once(" = ")
            .unwrap_or_else(|| panic!("not a finding's key: {line:?}"));
        assert!(
            ["role", "saw", "evidence"].contains(&key) && !cur.contains_key(key),
            "not a finding's key: {line:?}"
        );
        assert!(
            value.starts_with('"') && value.ends_with('"'),
            "not a basic string: {line:?}"
        );
        // A TOML basic string as written is valid JSON.
        let value: String = serde_json::from_str(value).unwrap_or_else(|e| panic!("{line:?}: {e}"));
        cur.insert(key.to_string(), value);
    }
    for f in &findings {
        assert!(
            f.len() == 3 && f.values().all(|v| !v.is_empty()),
            "incomplete finding: {f:?}"
        );
    }
    findings
}
