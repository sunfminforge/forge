//! shot's binary on local HTML fixtures with the machine's real Chrome:
//! overflow and fit at 390px, the dark and light schemes as the fixture's
//! prefers-color-scheme CSS paints them, the PNG's width and full height,
//! and each refusal with its fix. Offline: only file:// and a closed local
//! port are loaded.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_shot");

fn fixture(name: &str) -> String {
    format!("{}/fixtures/{name}", env!("CARGO_MANIFEST_DIR"))
}

/// A directory of the test's own under the target dir, emptied first.
fn scratch(name: &str) -> PathBuf {
    let d = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("shot-{name}"));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn run(args: &[&str], cwd: &Path) -> Output {
    Command::new(BIN)
        .args(args)
        .current_dir(cwd)
        .env_remove("SHOT_BROWSER")
        .output()
        .unwrap()
}

fn stdout(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).into_owned()
}

fn stderr(o: &Output) -> String {
    String::from_utf8_lossy(&o.stderr).into_owned()
}

/// The value of `key: value` on stdout.
fn line<'a>(out: &'a str, key: &str) -> &'a str {
    out.lines()
        .find_map(|l| l.strip_prefix(&format!("{key}: ")))
        .unwrap_or_else(|| panic!("no {key} line in {out:?}"))
}

/// The PNG's width, height and the RGB of pixel (x, y).
fn pixel(png: &Path, x: u32, y: u32) -> (u32, u32, [u8; 3]) {
    let mut reader = png::Decoder::new(std::io::BufReader::new(std::fs::File::open(png).unwrap()))
        .read_info()
        .unwrap();
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).unwrap();
    let channels = info.color_type.samples();
    assert_eq!(info.bit_depth, png::BitDepth::Eight);
    let at = (y as usize * info.width as usize + x as usize) * channels;
    (info.width, info.height, [buf[at], buf[at + 1], buf[at + 2]])
}

#[test]
fn overflow_a_page_wider_than_390_says_yes() {
    let dir = scratch("overflow");
    let o = run(
        &[&fixture("wide.html"), "--width", "390", "--out", "wide.png"],
        &dir,
    );
    assert!(o.status.success(), "{}", stderr(&o));
    let out = stdout(&o);
    assert_eq!(out.lines().count(), 4, "{out}");
    assert_eq!(line(&out, "png"), dir.join("wide.png").to_str().unwrap());
    assert_eq!(line(&out, "width"), "390");
    // wide.html holds a 600px row.
    assert_eq!(line(&out, "scrollWidth"), "600");
    assert_eq!(line(&out, "overflow"), "yes");
}

#[test]
fn fitting_a_page_within_390_says_no() {
    let dir = scratch("fitting");
    let o = run(
        &[&fixture("fit.html"), "--width", "390", "--out", "fit.png"],
        &dir,
    );
    assert!(o.status.success(), "{}", stderr(&o));
    let out = stdout(&o);
    assert_eq!(line(&out, "width"), "390");
    assert_eq!(line(&out, "scrollWidth"), "390");
    assert_eq!(line(&out, "overflow"), "no");
}

#[test]
fn dark_paints_the_dark_background() {
    let dir = scratch("dark");
    let o = run(
        &[
            &fixture("scheme.html"),
            "--width",
            "390",
            "--dark",
            "--out",
            "d.png",
        ],
        &dir,
    );
    assert!(o.status.success(), "{}", stderr(&o));
    // scheme.html: html is #000000 under prefers-color-scheme: dark.
    let (_, _, rgb) = pixel(&dir.join("d.png"), 10, 10);
    assert_eq!(rgb, [0, 0, 0]);
}

#[test]
fn light_paints_the_light_background() {
    let dir = scratch("light");
    let o = run(
        &[
            &fixture("scheme.html"),
            "--width",
            "390",
            "--light",
            "--out",
            "l.png",
        ],
        &dir,
    );
    assert!(o.status.success(), "{}", stderr(&o));
    // scheme.html: html is #ffffff otherwise.
    let (_, _, rgb) = pixel(&dir.join("l.png"), 10, 10);
    assert_eq!(rgb, [255, 255, 255]);
}

#[test]
fn width_the_png_is_the_asked_width_and_the_whole_page() {
    let dir = scratch("width");
    // Wider than the ~500px a headless window is clamped to.
    let o = run(&[&fixture("scheme.html"), "--width", "1280"], &dir);
    assert!(o.status.success(), "{}", stderr(&o));
    let out = stdout(&o);
    let png = dir.join("shot-1280-light.png");
    assert_eq!(line(&out, "png"), png.to_str().unwrap());
    let (w, h, _) = pixel(&png, 0, 0);
    assert_eq!(w, 1280);
    // scheme.html is 2000px tall, past the 800px viewport.
    assert_eq!(h, 2000);
    let o = run(
        &[
            &fixture("wide.html"),
            "--width",
            "390",
            "--dark",
            "--out",
            "n.png",
        ],
        &dir,
    );
    assert!(o.status.success(), "{}", stderr(&o));
    assert_eq!(pixel(&dir.join("n.png"), 0, 0).0, 390);
}

#[test]
fn rewriting_the_same_png_replaces_it_whole() {
    let dir = scratch("again");
    for _ in 0..2 {
        let o = run(
            &[&fixture("fit.html"), "--width", "200", "--out", "a.png"],
            &dir,
        );
        assert!(o.status.success(), "{}", stderr(&o));
    }
    let names: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, vec![std::ffi::OsString::from("a.png")]);
    assert_eq!(pixel(&dir.join("a.png"), 0, 0).0, 200);
}

#[test]
fn no_browser_is_refused_with_its_fix() {
    let dir = scratch("nobrowser");
    let o = Command::new(BIN)
        .args([&fixture("fit.html"), "--width", "390"])
        .current_dir(&dir)
        .env("SHOT_BROWSER", "/nonexistent/chrome")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(1));
    assert!(stdout(&o).is_empty());
    let err = stderr(&o);
    assert!(err.contains("no browser found"), "{err}");
    assert!(err.contains("set SHOT_BROWSER"), "{err}");
    assert!(std::fs::read_dir(&dir).unwrap().next().is_none());
}

#[test]
fn a_missing_file_is_refused_with_its_fix() {
    let dir = scratch("nofile");
    let o = run(&["nothere.html", "--width", "390"], &dir);
    assert_eq!(o.status.code(), Some(1));
    let err = stderr(&o);
    assert!(err.contains("no file at"), "{err}");
    assert!(
        err.contains("give the path of an existing HTML file"),
        "{err}"
    );
}

#[test]
fn a_page_that_fails_to_load_is_refused_with_its_fix() {
    let dir = scratch("noload");
    // A loopback port just bound and let go: nothing listens on it.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let url = format!("http://127.0.0.1:{port}/");
    let o = run(&[&url, "--width", "390"], &dir);
    assert_eq!(o.status.code(), Some(1), "{}", stdout(&o));
    let err = stderr(&o);
    assert!(err.contains("the page failed to load"), "{err}");
    assert!(err.contains("ERR_CONNECTION_REFUSED"), "{err}");
    assert!(err.contains("check the URL"), "{err}");
    assert!(std::fs::read_dir(&dir).unwrap().next().is_none());
}

#[test]
fn an_out_directory_that_is_not_there_is_refused_with_its_fix() {
    let dir = scratch("nodir");
    let o = run(
        &[
            &fixture("fit.html"),
            "--width",
            "390",
            "--out",
            "no/such/a.png",
        ],
        &dir,
    );
    assert_eq!(o.status.code(), Some(1));
    assert!(
        stderr(&o).contains("create it, or give --out a path"),
        "{}",
        stderr(&o)
    );
}

#[test]
fn bad_arguments_exit_2_with_the_usage() {
    let dir = scratch("usage");
    for args in [
        &["page.html"][..],
        &["page.html", "--width", "0"],
        &["page.html", "--width", "390", "--dark", "--light"],
        &["page.html", "--width", "390", "--window-size"],
    ] {
        let o = run(args, &dir);
        assert_eq!(o.status.code(), Some(2), "{args:?}");
        assert!(
            stderr(&o).contains("usage: shot <url|file> --width <px>"),
            "{args:?}"
        );
    }
}

#[test]
fn help_and_version_answer_on_stdout() {
    let dir = scratch("help");
    let o = run(&["--help"], &dir);
    assert!(o.status.success());
    let out = stdout(&o);
    assert!(out.starts_with("usage: shot <url|file> --width <px>"));
    for word in [
        "--dark",
        "--light",
        "--out",
        "scrollWidth",
        "overflow: yes|no",
        "SHOT_BROWSER",
    ] {
        assert!(out.contains(word), "help lacks {word}");
    }
    let o = run(&["--version"], &dir);
    assert_eq!(stdout(&o), format!("shot {}\n", env!("CARGO_PKG_VERSION")));
}
