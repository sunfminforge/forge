//! shot: what it decides, without touching the world. The arguments it
//! takes, the URL a file argument names, where the browser may be, the
//! devtools messages it sends, how it reads Chrome's answers, and the lines
//! it prints. `world` starts the browser, talks to it and writes the PNG.

pub mod world;

use serde_json::{json, Value};

pub const NAME: &str = "shot";

/// Exit code of a shot that failed: no browser, the page did not load, or
/// the PNG could not be written.
pub const EXIT_FAIL: u8 = 1;
/// Exit code of an argument shot does not take.
pub const EXIT_USAGE: u8 = 2;

/// The narrowest and widest viewport shot emulates.
pub const MIN_WIDTH: u32 = 1;
pub const MAX_WIDTH: u32 = 10_000;

/// The viewport's height while the page loads and is measured; the PNG is
/// the full page's height whatever this is.
pub const VIEWPORT_HEIGHT: u32 = 800;

/// No PNG is taller than this many pixels: past it the page is cut, so an
/// endless page cannot make an endless image.
pub const MAX_HEIGHT: u64 = 20_000;

/// Seconds the whole shot may take, browser start to PNG.
pub const TIMEOUT_SECS: u64 = 30;

/// The environment variable that names the browser to run, before any
/// place shot looks for one itself.
pub const BROWSER_VAR: &str = "SHOT_BROWSER";

pub const USAGE: &str = "usage: shot <url|file> --width <px> [--dark|--light] [--out <png>]";

/// What `--help` answers.
pub const HELP: &str = concat!(
    "usage: shot <url|file> --width <px> [--dark|--light] [--out <png>]\n",
    "       shot --version\n\n",
    "Loads a page in headless Chrome with the viewport emulated at <px> CSS pixels wide through the devtools protocol (no window, so no ~500px window clamp) and prefers-color-scheme emulated as asked, saves a full-page PNG <px> pixels wide, and prints the page's document scrollWidth beside the width, so horizontal overflow shows. Needs Google Chrome or Chromium; nothing else.\n\n",
    "arguments:\n",
    "  <url|file>     an http(s)://, file://, data: or about: URL, or a path to a local HTML file\n",
    "  --width <px>   the viewport's width in CSS pixels, 1 to 10000; the PNG is this many pixels wide (device scale 1)\n",
    "  --dark         emulate prefers-color-scheme: dark\n",
    "  --light        emulate prefers-color-scheme: light (the default)\n",
    "  --out <png>    where to write the PNG (default: shot-<px>-<dark|light>.png in the current directory); written whole, replacing any file there\n\n",
    "prints, one line each:\n",
    "  png: <absolute path of the PNG>\n",
    "  width: <px>\n",
    "  scrollWidth: <document.documentElement.scrollWidth>\n",
    "  overflow: yes|no   (yes when scrollWidth is over the width)\n\n",
    "The browser is SHOT_BROWSER when set, else the first of Google Chrome, Chromium, Microsoft Edge or Brave found in /Applications or ~/Applications, else google-chrome, google-chrome-stable, chromium or chromium-browser on PATH. It runs with a fresh profile, removed afterwards. A page taller than 20000 pixels is cut there. The whole shot gives up after 30 seconds.\n\n",
    "Exits 0 with the four lines on stdout. Exits 1 with the cause and its fix on stderr when no browser is found, the browser fails, the file is missing, the page fails to load or the PNG cannot be written; exits 2 with the usage on stderr on an argument it does not take.\n",
);

/// The colour scheme the page is told the reader prefers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scheme {
    Light,
    Dark,
}

impl Scheme {
    pub fn name(self) -> &'static str {
        match self {
            Scheme::Light => "light",
            Scheme::Dark => "dark",
        }
    }
}

/// A viewport width shot can emulate: only `Width::new` makes one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Width(u32);

impl Width {
    pub fn new(px: u32) -> Option<Width> {
        (MIN_WIDTH..=MAX_WIDTH).contains(&px).then_some(Width(px))
    }
    pub fn px(self) -> u32 {
        self.0
    }
}

/// What the arguments ask for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked {
    Help,
    Version,
    Shot {
        target: String,
        width: Width,
        scheme: Scheme,
        out: Option<String>,
    },
}

/// The arguments after the program's name, or why they are refused.
pub fn parse(args: &[String]) -> Result<Asked, String> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        return Ok(Asked::Help);
    }
    if args.len() == 1 && (args[0] == "--version" || args[0] == "-V") {
        return Ok(Asked::Version);
    }
    let mut target = None;
    let mut width = None;
    let mut scheme = None;
    let mut out = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--width" => {
                let v = it.next().ok_or("--width needs a number of pixels")?;
                let px = v
                    .parse::<u32>()
                    .ok()
                    .and_then(Width::new)
                    .ok_or_else(|| {
                        format!(
                            "--width {v}: give a whole number of CSS pixels from {MIN_WIDTH} to {MAX_WIDTH}"
                        )
                    })?;
                if width.replace(px).is_some() {
                    return Err("--width given twice: give it once".into());
                }
            }
            "--dark" | "--light" => {
                let s = if a == "--dark" {
                    Scheme::Dark
                } else {
                    Scheme::Light
                };
                if scheme.replace(s).is_some() {
                    return Err("--dark or --light given twice: give one of them once".into());
                }
            }
            "--out" => {
                let v = it
                    .next()
                    .ok_or("--out needs the path of the PNG to write")?;
                if v.is_empty() {
                    return Err("--out needs the path of the PNG to write".into());
                }
                if out.replace(v.clone()).is_some() {
                    return Err("--out given twice: give it once".into());
                }
            }
            s if s.starts_with('-') && s.len() > 1 => {
                return Err(format!("{s}: not an option shot takes"));
            }
            s => {
                if s.is_empty() {
                    return Err("the page is empty: give a URL or the path of an HTML file".into());
                }
                if target.replace(s.to_string()).is_some() {
                    return Err("more than one page given: give one URL or file".into());
                }
            }
        }
    }
    let target = target.ok_or("no page given: give a URL or the path of an HTML file")?;
    let width = width.ok_or("no --width given: give the viewport's width, as --width 390")?;
    Ok(Asked::Shot {
        target,
        width,
        scheme: scheme.unwrap_or(Scheme::Light),
        out,
    })
}

/// Where the page is: a URL as given, or a local file to turn into one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Page {
    Url(String),
    File(String),
}

/// A target with a scheme of its own (`http://`, `file://`, `data:`,
/// `about:`) is a URL; anything else is a path.
pub fn page(target: &str) -> Page {
    let lower = target.to_ascii_lowercase();
    let url = ["http://", "https://", "file://", "data:", "about:"]
        .iter()
        .any(|p| lower.starts_with(p));
    if url {
        Page::Url(target.to_string())
    } else {
        Page::File(target.to_string())
    }
}

/// `path` made absolute against `cwd`, both as the effect crate read them.
pub fn absolute(path: &str, cwd: &str) -> String {
    if path.starts_with('/') {
        path.to_string()
    } else {
        format!("{}/{}", cwd.trim_end_matches('/'), path)
    }
}

/// The file:// URL of an absolute path, each byte outside a path's safe
/// set percent-encoded.
pub fn file_url(abs: &str) -> String {
    let mut out = String::from("file://");
    for b in abs.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'_' | b'.' | b'-' | b'~' => {
                out.push(b as char)
            }
            b => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The PNG's name when `--out` is not given.
pub fn default_out(width: Width, scheme: Scheme) -> String {
    format!("shot-{}-{}.png", width.px(), scheme.name())
}

/// The places a browser may be, in the order they are tried: the macOS
/// apps under `/Applications` and `home`'s Applications, then the
/// commands on each directory of `path` (PATH as read).
pub fn browser_candidates(home: &str, path: &str) -> Vec<String> {
    const APPS: [(&str, &str); 5] = [
        ("Google Chrome.app", "Google Chrome"),
        ("Chromium.app", "Chromium"),
        ("Google Chrome for Testing.app", "Google Chrome for Testing"),
        ("Microsoft Edge.app", "Microsoft Edge"),
        ("Brave Browser.app", "Brave Browser"),
    ];
    const COMMANDS: [&str; 4] = [
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
    ];
    let mut out = Vec::new();
    let mut roots = vec!["/Applications".to_string()];
    if !home.is_empty() {
        roots.push(format!("{}/Applications", home.trim_end_matches('/')));
    }
    for root in &roots {
        for (app, bin) in APPS {
            out.push(format!("{root}/{app}/Contents/MacOS/{bin}"));
        }
    }
    for cmd in COMMANDS {
        for dir in path.split(':').filter(|d| !d.is_empty()) {
            out.push(format!("{}/{cmd}", dir.trim_end_matches('/')));
        }
    }
    out
}

/// The flags the browser runs with: headless, talking devtools over the
/// pipe on fds 3 and 4, with a profile of its own. No --window-size:
/// the width is emulated by `Emulation.setDeviceMetricsOverride`.
pub fn browser_args(profile: &str) -> Vec<String> {
    [
        "--headless=new",
        "--remote-debugging-pipe",
        "--no-first-run",
        "--no-default-browser-check",
        "--disable-extensions",
        "--disable-background-networking",
        "--disable-sync",
        "--hide-scrollbars",
        "--mute-audio",
    ]
    .iter()
    .map(|s| s.to_string())
    .chain([format!("--user-data-dir={profile}"), "about:blank".into()])
    .collect()
}

/// One devtools call: its id, method and params, sent to the browser or,
/// with `session`, to the page attached as that session.
pub fn call(id: u64, method: &str, params: Value, session: Option<&str>) -> Vec<u8> {
    let mut m = json!({"id": id, "method": method, "params": params});
    if let Some(s) = session {
        m["sessionId"] = json!(s);
    }
    let mut bytes = m.to_string().into_bytes();
    // The pipe protocol ends each message with a NUL byte.
    bytes.push(0);
    bytes
}

/// The params that emulate `width` × `height` CSS pixels at scale 1.
pub fn metrics(width: Width, height: u32) -> Value {
    json!({"width": width.px(), "height": height, "deviceScaleFactor": 1, "mobile": false})
}

/// The params that emulate `scheme` as prefers-color-scheme.
pub fn media(scheme: Scheme) -> Value {
    json!({"features": [{"name": "prefers-color-scheme", "value": scheme.name()}]})
}

/// What shot evaluates in the page once it has loaded: the document's
/// scrollWidth and scrollHeight.
pub const MEASURE: &str =
    "JSON.stringify([document.documentElement.scrollWidth, Math.max(document.documentElement.scrollHeight, document.body ? document.body.scrollHeight : 0)])";

/// The page's size as `MEASURE` answered it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Measured {
    pub scroll_width: u64,
    pub scroll_height: u64,
}

/// Reads `Runtime.evaluate`'s answer to `MEASURE`.
pub fn measured(result: &Value) -> Option<Measured> {
    let s = result.pointer("/result/value")?.as_str()?;
    let v: Value = serde_json::from_str(s).ok()?;
    Some(Measured {
        scroll_width: v.get(0)?.as_u64()?,
        scroll_height: v.get(1)?.as_u64()?,
    })
}

/// The params of the full-page capture: the asked width, the page's
/// height up to `MAX_HEIGHT`, beyond the viewport, scale 1.
pub fn capture(width: Width, m: Measured) -> Value {
    let height = m.scroll_height.clamp(1, MAX_HEIGHT);
    json!({
        "format": "png",
        "captureBeyondViewport": true,
        "fromSurface": true,
        "clip": {"x": 0, "y": 0, "width": width.px(), "height": height, "scale": 1}
    })
}

/// Splits complete NUL-ended messages off the front of `buf`, leaving any
/// partial one in it.
pub fn frames(buf: &mut Vec<u8>) -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    while let Some(i) = buf.iter().position(|&b| b == 0) {
        let rest = buf.split_off(i + 1);
        let mut msg = std::mem::replace(buf, rest);
        msg.pop();
        out.push(msg);
    }
    out
}

/// Whether the page is wider than the viewport.
pub fn overflows(width: Width, scroll_width: u64) -> bool {
    scroll_width > u64::from(width.px())
}

/// The four lines shot prints.
pub fn report(png: &str, width: Width, scroll_width: u64) -> String {
    format!(
        "png: {png}\nwidth: {}\nscrollWidth: {scroll_width}\noverflow: {}\n",
        width.px(),
        if overflows(width, scroll_width) {
            "yes"
        } else {
            "no"
        }
    )
}

/// Standard base64 (with or without padding) decoded, or None on a byte
/// outside its alphabet.
pub fn base64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() / 4 * 3);
    let mut acc: u32 = 0;
    let mut bits = 0;
    for b in s.bytes() {
        let v = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => break,
            b'\n' | b'\r' => continue,
            _ => return None,
        };
        acc = (acc << 6) | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &[&str]) -> Vec<String> {
        s.iter().map(|a| a.to_string()).collect()
    }

    #[test]
    fn parses_a_shot() {
        assert_eq!(
            parse(&args(&[
                "page.html",
                "--width",
                "390",
                "--dark",
                "--out",
                "a.png"
            ])),
            Ok(Asked::Shot {
                target: "page.html".into(),
                width: Width::new(390).unwrap(),
                scheme: Scheme::Dark,
                out: Some("a.png".into()),
            })
        );
        assert_eq!(
            parse(&args(&["--width", "1", "https://x.test/"])),
            Ok(Asked::Shot {
                target: "https://x.test/".into(),
                width: Width::new(1).unwrap(),
                scheme: Scheme::Light,
                out: None,
            })
        );
    }

    #[test]
    fn refuses_bad_arguments() {
        for bad in [
            &["page.html"][..],
            &["--width", "390"],
            &["page.html", "--width", "0"],
            &["page.html", "--width", "10001"],
            &["page.html", "--width", "-3"],
            &["page.html", "--width", "39.5"],
            &["page.html", "--width"],
            &["page.html", "--width", "390", "--dark", "--light"],
            &["page.html", "--width", "390", "--out"],
            &["page.html", "--width", "390", "--wide"],
            &["a.html", "b.html", "--width", "390"],
        ] {
            assert!(parse(&args(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn tells_urls_from_files() {
        assert_eq!(
            page("https://a.test/x"),
            Page::Url("https://a.test/x".into())
        );
        assert_eq!(
            page("FILE:///tmp/a.html"),
            Page::Url("FILE:///tmp/a.html".into())
        );
        assert_eq!(page("about:blank"), Page::Url("about:blank".into()));
        assert_eq!(
            page("fixtures/a.html"),
            Page::File("fixtures/a.html".into())
        );
        assert_eq!(absolute("f/a.html", "/w/"), "/w/f/a.html");
        assert_eq!(absolute("/x/a.html", "/w"), "/x/a.html");
        assert_eq!(file_url("/a b/c#d.html"), "file:///a%20b/c%23d.html");
    }

    #[test]
    fn reports_overflow_only_past_the_width() {
        let w = Width::new(390).unwrap();
        assert_eq!(
            report("/t/a.png", w, 600),
            "png: /t/a.png\nwidth: 390\nscrollWidth: 600\noverflow: yes\n"
        );
        assert!(report("/t/a.png", w, 390).ends_with("overflow: no\n"));
    }

    #[test]
    fn splits_frames_keeping_the_partial_one() {
        let mut buf = b"{\"id\":1}\0{\"id\":2}\0{\"id\"".to_vec();
        assert_eq!(
            frames(&mut buf),
            vec![b"{\"id\":1}".to_vec(), b"{\"id\":2}".to_vec()]
        );
        assert_eq!(buf, b"{\"id\"".to_vec());
    }

    #[test]
    fn decodes_base64() {
        assert_eq!(base64("aGVsbG8="), Some(b"hello".to_vec()));
        assert_eq!(base64("aGVsbG8h"), Some(b"hello!".to_vec()));
        assert_eq!(base64("iVBORw0KGgo="), Some(b"\x89PNG\r\n\x1a\n".to_vec()));
        assert_eq!(base64("a*"), None);
    }

    #[test]
    fn caps_the_capture_height() {
        let w = Width::new(390).unwrap();
        let c = capture(
            w,
            Measured {
                scroll_width: 390,
                scroll_height: 99_999,
            },
        );
        assert_eq!(c["clip"]["height"], json!(MAX_HEIGHT));
        assert_eq!(c["clip"]["width"], json!(390));
    }

    #[test]
    fn looks_for_browsers_in_order() {
        let c = browser_candidates("/Users/u", "/usr/bin:/opt/bin");
        assert_eq!(
            c[0],
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"
        );
        assert!(
            c.contains(&"/Users/u/Applications/Chromium.app/Contents/MacOS/Chromium".to_string())
        );
        assert_eq!(c[c.len() - 1], "/opt/bin/chromium-browser");
    }
}
