//! What shot does to the world: reads its arguments, the environment and
//! the file system, starts the browser, talks devtools to it over the
//! pipe, writes the PNG and prints. The one module that runs a browser.

use std::fs::{self, File};
use std::io::{PipeReader, PipeWriter, Read, Write};
use std::os::fd::AsRawFd;
use std::os::raw::c_int;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitCode, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use crate::{
    absolute, base64, browser_args, browser_candidates, call, capture, default_out, file_url,
    measured, media, metrics, page, parse, report, Asked, Page, Scheme, Width, BROWSER_VAR,
    EXIT_FAIL, EXIT_USAGE, HELP, MEASURE, NAME, TIMEOUT_SECS, USAGE, VIEWPORT_HEIGHT,
};

/// Why a shot failed: said on stderr with its fix, exit 1.
struct Fail(String);

impl From<String> for Fail {
    fn from(s: String) -> Fail {
        Fail(s)
    }
}

extern "C" {
    fn fcntl(fd: c_int, cmd: c_int, ...) -> c_int;
    fn dup2(src: c_int, dst: c_int) -> c_int;
    fn close(fd: c_int) -> c_int;
}

/// fcntl's F_DUPFD, 0 on macOS and Linux alike.
const F_DUPFD: c_int = 0;

pub fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse(&args) {
        Ok(Asked::Help) => {
            print!("{HELP}");
            ExitCode::SUCCESS
        }
        Ok(Asked::Version) => {
            println!("{NAME} {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Ok(Asked::Shot {
            target,
            width,
            scheme,
            out,
        }) => match shot(&target, width, scheme, out) {
            Ok(text) => {
                print!("{text}");
                let _ = std::io::stdout().flush();
                ExitCode::SUCCESS
            }
            Err(Fail(why)) => {
                eprintln!("{NAME}: {why}");
                ExitCode::from(EXIT_FAIL)
            }
        },
        Err(why) => {
            eprintln!("{NAME}: {why}\n{USAGE}\nRun `{NAME} --help` for what each argument does.");
            ExitCode::from(EXIT_USAGE)
        }
    }
}

fn cwd() -> Result<String, Fail> {
    std::env::current_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| {
            Fail(format!(
                "cannot read the current directory: {e}: run shot from a directory that exists"
            ))
        })
}

fn shot(target: &str, width: Width, scheme: Scheme, out: Option<String>) -> Result<String, Fail> {
    let cwd = cwd()?;
    let url = match page(target) {
        Page::Url(u) => u,
        Page::File(p) => {
            let abs = absolute(&p, &cwd);
            if !Path::new(&abs).is_file() {
                return Err(Fail(format!(
                    "no file at {abs}: give the path of an existing HTML file, or a URL with its scheme (https://…, file://…)"
                )));
            }
            file_url(&abs)
        }
    };
    let out = absolute(&out.unwrap_or_else(|| default_out(width, scheme)), &cwd);
    let dir = Path::new(&out).parent().unwrap_or(Path::new("/"));
    if !dir.is_dir() {
        return Err(Fail(format!(
            "no directory {} to write {out} in: create it, or give --out a path in a directory that exists",
            dir.display()
        )));
    }
    let browser = find_browser()?;
    let deadline = Instant::now() + Duration::from_secs(TIMEOUT_SECS);
    let mut b = Browser::start(&browser, deadline)?;
    let (png, scroll_width) = b.shoot(&url, width, scheme)?;
    b.close();
    write_whole(&out, &png)?;
    Ok(report(&out, width, scroll_width))
}

fn executable(p: &str) -> bool {
    fs::metadata(p)
        .map(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

/// The browser to run: SHOT_BROWSER when set, else the first candidate
/// that is there.
fn find_browser() -> Result<String, Fail> {
    if let Some(named) = std::env::var_os(BROWSER_VAR).filter(|v| !v.is_empty()) {
        let named = named.to_string_lossy().into_owned();
        return if executable(&named) {
            Ok(named)
        } else {
            Err(Fail(format!(
                "no browser found: {BROWSER_VAR} is {named}, which is not an executable file: set {BROWSER_VAR} to Chrome's or Chromium's executable, or unset it to let shot look for one"
            )))
        };
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let path = std::env::var("PATH").unwrap_or_default();
    browser_candidates(&home, &path)
        .into_iter()
        .find(|c| executable(c))
        .ok_or_else(|| {
            Fail(format!(
                "no browser found: shot needs Google Chrome or Chromium and found neither in /Applications, ~/Applications or on PATH: install Google Chrome (brew install --cask google-chrome), or set {BROWSER_VAR} to a Chrome or Chromium executable"
            ))
        })
}

/// The PNG written to a temp file beside `out` and renamed over it, so
/// a kill leaves the old file or the new one, never half of one.
fn write_whole(out: &str, png: &[u8]) -> Result<(), Fail> {
    let target = PathBuf::from(out);
    let name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "shot.png".into());
    let tmp = target.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let write = || -> std::io::Result<()> {
        let mut f = File::create(&tmp)?;
        f.write_all(png)?;
        f.sync_all()?;
        fs::rename(&tmp, &target)
    };
    write().map_err(|e| {
        let _ = fs::remove_file(&tmp);
        Fail(format!(
            "cannot write {out}: {e}: give --out a path you can write"
        ))
    })
}

/// A headless browser started for one shot, its fresh profile, and the
/// devtools pipe to it. Dropped, it is killed and its profile removed.
struct Browser {
    child: Child,
    profile: PathBuf,
    to: PipeWriter,
    from: Receiver<Value>,
    events: Vec<Value>,
    next: u64,
    deadline: Instant,
}

impl Browser {
    fn start(exe: &str, deadline: Instant) -> Result<Browser, Fail> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let profile = std::env::temp_dir().join(format!("shot-{}-{stamp}", std::process::id()));
        fs::create_dir_all(&profile).map_err(|e| {
            Fail(format!(
                "cannot make a browser profile in {}: {e}: set TMPDIR to a directory you can write",
                profile.display()
            ))
        })?;
        let log = File::create(profile.join("browser.log")).ok();
        let pipes = (|| Ok::<_, std::io::Error>((std::io::pipe()?, std::io::pipe()?)))();
        let ((browser_reads, to), (from, browser_writes)) = match pipes {
            Ok(p) => p,
            Err(e) => {
                let _ = fs::remove_dir_all(&profile);
                return Err(Fail(format!("cannot make a pipe to the browser: {e}")));
            }
        };
        let (r, w) = (browser_reads.as_raw_fd(), browser_writes.as_raw_fd());
        let mut cmd = Command::new(exe);
        cmd.args(browser_args(&profile.to_string_lossy()))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(log.map(Stdio::from).unwrap_or_else(Stdio::null));
        // SAFETY: between fork and exec only async-signal-safe calls are
        // made: fcntl, dup2 and close. The devtools pipe protocol reads
        // the browser's commands on fd 3 and writes its answers on fd 4;
        // each end is first moved above 9 so neither dup2 clobbers the
        // other.
        unsafe {
            cmd.pre_exec(move || {
                let r2 = fcntl(r, F_DUPFD, 10);
                let w2 = fcntl(w, F_DUPFD, 10);
                if r2 < 0 || w2 < 0 || dup2(r2, 3) < 0 || dup2(w2, 4) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                close(r2);
                close(w2);
                Ok(())
            });
        }
        let child = cmd.spawn();
        drop(browser_reads);
        drop(browser_writes);
        let child = match child {
            Ok(c) => c,
            Err(e) => {
                let _ = fs::remove_dir_all(&profile);
                return Err(Fail(format!(
                    "cannot run the browser {exe}: {e}: set {BROWSER_VAR} to a Chrome or Chromium executable that runs"
                )));
            }
        };
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || read_messages(from, tx));
        Ok(Browser {
            child,
            profile,
            to,
            from: rx,
            events: Vec::new(),
            next: 1,
            deadline,
        })
    }

    /// The last lines the browser wrote on stderr, to say why it stopped.
    fn log_tail(&self) -> String {
        let text = fs::read_to_string(self.profile.join("browser.log")).unwrap_or_default();
        let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
        let tail = lines[lines.len().saturating_sub(3)..].join(" | ");
        if tail.is_empty() {
            String::new()
        } else {
            format!(" (browser said: {tail})")
        }
    }

    fn next_message(&mut self, waiting: &str) -> Result<Value, Fail> {
        let left = self.deadline.saturating_duration_since(Instant::now());
        match self.from.recv_timeout(left) {
            Ok(v) => Ok(v),
            Err(RecvTimeoutError::Timeout) => Err(Fail(format!(
                "gave up after {TIMEOUT_SECS} seconds waiting for {waiting}: check the page loads in a browser, or that it does not wait on something that never comes"
            ))),
            Err(RecvTimeoutError::Disconnected) => Err(Fail(format!(
                "the browser stopped while shot waited for {waiting}{}: check the browser runs headless, as `<browser> --headless=new --dump-dom about:blank`",
                self.log_tail()
            ))),
        }
    }

    /// Sends one devtools call and waits for its answer, keeping the
    /// events that come before it.
    fn send(&mut self, method: &str, params: Value, session: Option<&str>) -> Result<Value, Fail> {
        let id = self.next;
        self.next += 1;
        if let Err(e) = self.to.write_all(&call(id, method, params, session)) {
            return Err(Fail(format!(
                "cannot talk to the browser ({method}): {e}{}: check the browser runs headless",
                self.log_tail()
            )));
        }
        loop {
            let m = self.next_message(&format!("the browser's answer to {method}"))?;
            if m.get("id").and_then(Value::as_u64) == Some(id) {
                if let Some(err) = m.get("error") {
                    let msg = err.get("message").and_then(Value::as_str).unwrap_or("");
                    return Err(Fail(format!(
                        "the browser refused {method}: {msg}: use a recent Google Chrome or Chromium"
                    )));
                }
                return Ok(m.get("result").cloned().unwrap_or(Value::Null));
            }
            if m.get("method").is_some() {
                self.events.push(m);
            }
        }
    }

    /// Waits for the first event that `matches`, kept or still to come.
    fn wait(&mut self, waiting: &str, matches: impl Fn(&Value) -> bool) -> Result<Value, Fail> {
        if let Some(i) = self.events.iter().position(&matches) {
            return Ok(self.events.remove(i));
        }
        loop {
            let m = self.next_message(waiting)?;
            if matches(&m) {
                return Ok(m);
            }
        }
    }

    /// Opens a page at `width` and `scheme`, loads `url`, measures it and
    /// captures it whole: the PNG's bytes and the document's scrollWidth.
    fn shoot(&mut self, url: &str, width: Width, scheme: Scheme) -> Result<(Vec<u8>, u64), Fail> {
        let target = self.send("Target.createTarget", json!({"url": "about:blank"}), None)?;
        let target_id = target["targetId"].as_str().unwrap_or_default().to_string();
        let attached = self.send(
            "Target.attachToTarget",
            json!({"targetId": target_id, "flatten": true}),
            None,
        )?;
        let session = attached["sessionId"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        let s = Some(session.as_str());
        self.send(
            "Emulation.setDeviceMetricsOverride",
            metrics(width, VIEWPORT_HEIGHT),
            s,
        )?;
        self.send("Emulation.setEmulatedMedia", media(scheme), s)?;
        self.send("Page.enable", json!({}), s)?;
        self.send(
            "Page.setLifecycleEventsEnabled",
            json!({"enabled": true}),
            s,
        )?;
        self.events.clear();
        let nav = self.send("Page.navigate", json!({"url": url}), s)?;
        if let Some(err) = nav
            .get("errorText")
            .and_then(Value::as_str)
            .filter(|e| !e.is_empty())
        {
            return Err(Fail(format!(
                "the page failed to load: {url}: {err}: check the URL or file, and that its server is up"
            )));
        }
        let loader = nav
            .get("loaderId")
            .and_then(Value::as_str)
            .map(str::to_string);
        let in_session = |m: &Value| m.get("sessionId").and_then(Value::as_str) == s;
        self.wait("the page to load", |m| {
            in_session(m)
                && match &loader {
                    Some(l) => {
                        m["method"] == "Page.lifecycleEvent"
                            && m["params"]["name"] == "load"
                            && m["params"]["loaderId"].as_str() == Some(l)
                    }
                    None => m["method"] == "Page.loadEventFired",
                }
        })?;
        let eval = self.send(
            "Runtime.evaluate",
            json!({"expression": MEASURE, "returnByValue": true}),
            s,
        )?;
        let m = measured(&eval).ok_or_else(|| {
            Fail(format!(
                "the page loaded but its size could not be read: {url}: check it is an HTML page"
            ))
        })?;
        let shot = self.send("Page.captureScreenshot", capture(width, m), s)?;
        let png = shot["data"]
            .as_str()
            .and_then(base64)
            .filter(|b| b.starts_with(b"\x89PNG"))
            .ok_or_else(|| {
                Fail("the browser answered no PNG: use a recent Google Chrome or Chromium".into())
            })?;
        Ok((png, m.scroll_width))
    }

    /// Asks the browser to close, and gives it a moment before Drop kills it.
    fn close(&mut self) {
        let _ = self
            .to
            .write_all(&call(self.next, "Browser.close", json!({}), None));
        let until = Instant::now() + Duration::from_secs(3);
        while Instant::now() < until {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        if let Ok(None) = self.child.try_wait() {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.profile);
    }
}

/// Reads the browser's NUL-ended messages until the pipe closes, handing
/// each one parsed on.
fn read_messages(mut from: PipeReader, tx: mpsc::Sender<Value>) {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        match from.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                for msg in crate::frames(&mut buf) {
                    if let Ok(v) = serde_json::from_slice::<Value>(&msg) {
                        if tx.send(v).is_err() {
                            return;
                        }
                    }
                }
            }
        }
    }
}
