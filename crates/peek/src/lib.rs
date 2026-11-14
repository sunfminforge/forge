//! peek: what it decides, without touching the world. The arguments it
//! takes, the header, a numbered line, a directory's entries as printed,
//! whether a file's bytes are binary, and what each failure says with its
//! fix. `world` stats, reads and prints.

pub mod world;

use std::io::ErrorKind;

pub const NAME: &str = "peek";

/// Exit code when any path was missing or unreadable, and of a usage
/// error (no path, an option peek does not take).
pub const EXIT_FAILED: u8 = 2;

pub const USAGE: &str = "usage: peek <path>…";

/// What `--help` answers, before its exit code is put in by `help`.
const HELP: &str = concat!(
    "usage: peek <path>…\n",
    "       peek -- <path>…\n",
    "       peek --version\n\n",
    "Does cat, ls and cat in one call: prints each named path in the order named, files and directories mixed. Example: peek crates/peek crates/peek/tool.toml src/lib.rs. It only reads: nothing it names is written or changed.\n\n",
    "Each path is printed under a header line '== <path>', the path as given, with a blank line before every header but the first. Then:\n",
    "  a file          each line as '<line>: <text>', its number counted from 1, the text's bytes unchanged; a last line without a newline gets one. An empty file prints '(empty, 0 bytes)'.\n",
    "  a binary file   (one holding a NUL byte) prints '(binary, N bytes)', N its size, instead of its content.\n",
    "  a directory     its entries one per line, every entry hidden ones included, sorted by name, each directory (or link to one) with a trailing '/'. An empty directory prints '(empty directory)'. It does not descend.\n",
    "  a failure       a missing or unreadable path prints one line 'error: <what failed>; <the fix>' under its header, and the other paths are still printed.\n\n",
    "Line ranges are grep-around's, not peek's: peek takes 'x.rs:10-20' as a path named so; for lines 10 to 20 of x.rs run grep-around <file>:<from>-<to>.\n\n",
    "options:\n",
    "  --              end of peek's options: what follows is paths, even when one starts with -\n",
    "  -h, --help      print this help\n",
    "  -V, --version   print peek's version\n\n",
    "Exits 0 when every path was read. Exits {EXIT_FAILED} when any path was missing or unreadable, after printing all the others. Exits {EXIT_FAILED} too, with the usage on stderr and nothing on stdout, when no path is named or an option is one it does not take.\n",
);

/// What `--help` answers, its exit code as the constant says.
pub fn help() -> String {
    HELP.replace("{EXIT_FAILED}", &EXIT_FAILED.to_string())
}

/// What the arguments ask.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked {
    Help,
    Version,
    Peek(Vec<String>),
}

/// The arguments, after the program's name. Options come first; after
/// `--` or the first path, every argument is a path.
pub fn parse(args: &[String]) -> Result<Asked, String> {
    let rest = match args.first().map(String::as_str) {
        None => return Err("name at least one path".into()),
        Some("-h" | "--help") => return Ok(Asked::Help),
        Some("-V" | "--version") if args.len() == 1 => return Ok(Asked::Version),
        Some("--") => &args[1..],
        Some(f) if f.starts_with('-') && f.len() > 1 => {
            return Err(format!(
                "no such option: {f}; to peek at a path named {f}, put -- before it"
            ))
        }
        Some(_) => args,
    };
    if rest.is_empty() {
        return Err("name at least one path after --".into());
    }
    Ok(Asked::Peek(rest.to_vec()))
}

/// The header line of one path, newline included.
pub fn header(path: &str) -> String {
    format!("== {path}\n")
}

/// The start of line `n` as printed.
pub fn number(n: u64) -> String {
    format!("{n}: ")
}

/// Whether a file's bytes are binary: they hold a NUL byte.
pub fn is_binary(bytes: &[u8]) -> bool {
    bytes.contains(&0)
}

/// What a file's bytes print as, after its header: the numbered lines,
/// `(binary, N bytes)` or `(empty, 0 bytes)`.
pub fn file_body(bytes: &[u8]) -> Vec<u8> {
    if bytes.is_empty() {
        return b"(empty, 0 bytes)\n".to_vec();
    }
    if is_binary(bytes) {
        return format!("(binary, {} bytes)\n", bytes.len()).into_bytes();
    }
    let mut out = Vec::with_capacity(bytes.len() + bytes.len() / 8);
    for (i, line) in bytes.split_inclusive(|&b| b == b'\n').enumerate() {
        out.extend_from_slice(number(i as u64 + 1).as_bytes());
        out.extend_from_slice(line);
        if line.last() != Some(&b'\n') {
            out.push(b'\n');
        }
    }
    out
}

/// One entry of a directory: its name, and whether it is (or links to)
/// a directory.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
}

/// What a directory's entries print as, after its header: sorted by
/// name, one per line, each directory with a trailing '/'.
pub fn dir_body(mut entries: Vec<Entry>) -> String {
    if entries.is_empty() {
        return "(empty directory)\n".into();
    }
    entries.sort();
    entries
        .iter()
        .map(|e| format!("{}{}\n", e.name, if e.dir { "/" } else { "" }))
        .collect()
}

/// Whether `path` ends in `:digits-digits`, a range grep-around takes.
fn looks_like_range(path: &str) -> bool {
    let Some((file, suffix)) = path.rsplit_once(':') else {
        return false;
    };
    let Some((a, b)) = suffix.split_once('-') else {
        return false;
    };
    let digits = |t: &str| !t.is_empty() && t.bytes().all(|c| c.is_ascii_digit());
    !file.is_empty() && digits(a) && digits(b)
}

/// The directory to list to find `path`'s name.
fn parent(path: &str) -> &str {
    match path.trim_end_matches('/').rsplit_once('/') {
        Some(("", _)) => "/",
        Some((p, _)) => p,
        None => ".",
    }
}

/// The error line printed under a path's header, newline included:
/// what failed and the fix.
pub fn error_line(path: &str, kind: ErrorKind, detail: &str) -> String {
    let (what, fix) = match kind {
        ErrorKind::NotFound if looks_like_range(path) => (
            "no such file or directory".to_string(),
            format!("line ranges are grep-around's: run grep-around {path}"),
        ),
        ErrorKind::NotFound => (
            "no such file or directory".to_string(),
            format!(
                "check the name with peek {}, relative to the current directory or absolute",
                parent(path)
            ),
        ),
        ErrorKind::PermissionDenied => (
            "permission denied".to_string(),
            "name a path you may read, or ask its owner for read permission".to_string(),
        ),
        _ => (
            detail.to_string(),
            "name a file or directory you may read".to_string(),
        ),
    };
    format!("error: {what}; {fix}\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn options_come_first_and_dash_dash_ends_them() {
        assert_eq!(parse(&s(&["--help"])), Ok(Asked::Help));
        assert_eq!(parse(&s(&["-V"])), Ok(Asked::Version));
        assert!(parse(&[]).is_err());
        assert!(parse(&s(&["--"])).is_err());
        assert!(parse(&s(&["-x"])).unwrap_err().contains("put -- before it"));
        assert_eq!(parse(&s(&["--", "-x"])), Ok(Asked::Peek(s(&["-x"]))));
        assert_eq!(
            parse(&s(&["a", "--help"])),
            Ok(Asked::Peek(s(&["a", "--help"])))
        );
    }

    #[test]
    fn a_file_body_is_numbered_lines_binary_or_empty() {
        assert_eq!(file_body(b"a\nb"), b"1: a\n2: b\n");
        assert_eq!(file_body(b"a\n\n"), b"1: a\n2: \n");
        assert_eq!(file_body(b"\x00\x01"), b"(binary, 2 bytes)\n");
        assert_eq!(file_body(b""), b"(empty, 0 bytes)\n");
        assert_eq!(file_body(b"\xff\xfe\n"), b"1: \xff\xfe\n");
    }

    #[test]
    fn a_dir_body_is_sorted_with_slashes() {
        let e = |n: &str, dir| Entry {
            name: n.into(),
            dir,
        };
        assert_eq!(
            dir_body(vec![e("tests", true), e("a.rs", false), e("src", true)]),
            "a.rs\nsrc/\ntests/\n"
        );
        assert_eq!(dir_body(vec![]), "(empty directory)\n");
    }

    #[test]
    fn an_error_line_names_its_fix() {
        let nf = |p| error_line(p, ErrorKind::NotFound, "");
        assert!(nf("a/b/c.rs").contains("peek a/b"), "{}", nf("a/b/c.rs"));
        assert!(nf("c.rs").contains("peek ."));
        assert!(nf("/c.rs").contains("peek /"));
        assert!(nf("a/b/").contains("peek a"));
        assert!(nf("x.rs:1-9").contains("grep-around x.rs:1-9"));
        assert!(!nf("x.rs:1-").contains("grep-around"));
        assert!(!nf(":1-9").contains("grep-around"));
        assert!(error_line("x", ErrorKind::PermissionDenied, "")
            .starts_with("error: permission denied; "));
        assert_eq!(header("a"), "== a\n");
        assert!(help().contains("Exits 2 when any path"));
    }
}
