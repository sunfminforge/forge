//! peek: what it decides, without touching the world. The arguments it
//! takes, a line range as only its constructor allows it, the header, a
//! numbered line, a file's lines or a range of them and a directory's
//! entries as printed, whether a file's bytes are binary, and what each
//! failure says with its fix. `world` stats, reads and prints.

pub mod world;

use std::io::ErrorKind;

pub const NAME: &str = "peek";

/// Exit code when any argument failed (a path missing or unreadable, a
/// range it cannot print whole), and of a usage error (no path, an
/// option peek does not take).
pub const EXIT_FAILED: u8 = 2;

pub const USAGE: &str = "usage: peek <path>[:<from>-<to>]…";

/// What `--help` answers, before its exit code is put in by `help`.
const HELP: &str = concat!(
    "usage: peek <path>[:<from>-<to>]…\n",
    "       peek -- <path>[:<from>-<to>]…\n",
    "       peek --version\n\n",
    "Does cat, sed -n 'a,bp' and ls in one call: prints each named path in the order named, whole files, line ranges of files and directories mixed. Example: peek crates/peek src/lib.rs:10-20 Cargo.toml. It only reads: nothing it names is written or changed.\n\n",
    "An argument ending in ':' then digits '-' digits is a line range, <path>:<from>-<to>: only lines <from> to <to> of the file, counted from 1, both included. Any other argument is a path, ':' and all; an argument that exists as a path as written is read as that path, even when it looks like a range.\n\n",
    "Each argument is printed under a header line '== <argument>', as given (e.g. '== src/lib.rs:10-20'), with a blank line before every header but the first. Then:\n",
    "  a file          each line as '<line>: <text>', its number counted from 1, the text's bytes unchanged; a last line without a newline gets one. An empty file prints '(empty, 0 bytes)'.\n",
    "  a line range    only the lines <from> to <to>, each as '<line>: <text>' with its real line number. When <to> is past the last line, the lines there are printed, then an 'error:' line naming the range to ask instead.\n",
    "  a binary file   (one holding a NUL byte) prints '(binary, N bytes)', N its size, instead of its content, with a range or without.\n",
    "  a directory     its entries one per line, every entry hidden ones included, sorted by name, each directory (or link to one) with a trailing '/'. An empty directory prints '(empty directory)'. It does not descend.\n",
    "  a failure       one line 'error: <what failed>; <the fix>' under its header, and the other arguments are still printed: a missing or unreadable path; a range starting past the last line, starting at 0 or starting after its end; a range on a directory.\n\n",
    "options:\n",
    "  --              end of peek's options: what follows is paths, even when one starts with -\n",
    "  -h, --help      print this help\n",
    "  -V, --version   print peek's version\n\n",
    "Exits 0 when every argument was read whole. Exits {EXIT_FAILED} when any argument failed (an 'error:' line under its header), after printing all the others. Exits {EXIT_FAILED} too, with the usage on stderr and nothing on stdout, when no path is named or an option is one it does not take.\n",
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

/// A line range, both ends counted from 1 and included. Only `new`
/// makes one, so every range has 1 <= from <= to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    from: u64,
    to: u64,
}

impl Range {
    pub fn new(from: u64, to: u64) -> Option<Range> {
        (1 <= from && from <= to).then_some(Range { from, to })
    }
    pub fn from(self) -> u64 {
        self.from
    }
    pub fn to(self) -> u64 {
        self.to
    }
}

/// An argument shaped `<path>:<from>-<to>`: the file and the lines asked,
/// or the error line of a range peek does not take.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Ranged {
    Lines { path: String, range: Range },
    Refused(String),
}

/// Splits `digits-digits` into its two numbers; `None` when it is not
/// that shape.
fn two_numbers(s: &str) -> Option<(&str, &str)> {
    let (a, b) = s.split_once('-')?;
    let digits = |t: &str| !t.is_empty() && t.bytes().all(|c| c.is_ascii_digit());
    (digits(a) && digits(b)).then_some((a, b))
}

/// What an argument ending in `:digits-digits` asks; `None` when it does
/// not end so, and it is a path, ':' and all.
pub fn ranged(arg: &str) -> Option<Ranged> {
    let (path, suffix) = arg.rsplit_once(':')?;
    let (a, b) = two_numbers(suffix)?;
    let refused =
        |what: &str, fix: String| Some(Ranged::Refused(format!("error: {what}; {fix}\n")));
    if path.is_empty() {
        return refused(
            "no path before the range",
            format!("name the file, as peek <file>:{suffix}"),
        );
    }
    let (Ok(from), Ok(to)) = (a.parse::<u64>(), b.parse::<u64>()) else {
        return refused(
            "line number too large",
            format!("ask lines below {}, or peek {path} whole", u64::MAX),
        );
    };
    match Range::new(from, to) {
        Some(range) => Some(Ranged::Lines {
            path: path.to_string(),
            range,
        }),
        None if from == 0 => refused(
            "lines count from 1, not 0",
            format!("ask peek {path}:1-{}", to.max(1)),
        ),
        None => refused(
            &format!("range starts at {from}, after its end {to}"),
            format!("ask peek {path}:{to}-{from}"),
        ),
    }
}

/// How many lines a file of `n` lines is said to have.
fn has(path: &str, n: u64) -> String {
    match n {
        0 => format!("{path} is empty"),
        1 => format!("{path} has 1 line"),
        n => format!("{path} has {n} lines"),
    }
}

/// What lines `range` of a file's bytes print as, after its header, and
/// whether every line asked was there: the numbered lines, then an
/// error line when the range runs past the end. A binary file prints
/// `(binary, N bytes)` as without a range.
pub fn range_body(path: &str, bytes: &[u8], range: Range) -> (Vec<u8>, bool) {
    if is_binary(bytes) {
        return (file_body(bytes), true);
    }
    let lines: Vec<&[u8]> = bytes.split_inclusive(|&b| b == b'\n').collect();
    let n = lines.len() as u64;
    if range.from() > n {
        let fix = match n {
            0 => format!("there is no line to ask; peek {path} shows it"),
            n => format!("ask peek {path}:1-{n}, or peek {path} whole"),
        };
        let what = format!(
            "range {}-{} starts past the end, {}",
            range.from(),
            range.to(),
            has(path, n)
        );
        return (format!("error: {what}; {fix}\n").into_bytes(), false);
    }
    let mut out = Vec::new();
    let last = range.to().min(n);
    for (i, line) in lines[(range.from() - 1) as usize..last as usize]
        .iter()
        .enumerate()
    {
        out.extend_from_slice(number(range.from() + i as u64).as_bytes());
        out.extend_from_slice(line);
        if line.last() != Some(&b'\n') {
            out.push(b'\n');
        }
    }
    if range.to() <= n {
        return (out, true);
    }
    out.extend_from_slice(
        format!(
            "error: lines {}-{} are past the end, {}; ask peek {path}:{}-{n}\n",
            n + 1,
            range.to(),
            has(path, n),
            range.from()
        )
        .as_bytes(),
    );
    (out, false)
}

/// The error line of a range asked of a directory, newline included.
pub fn range_on_dir(path: &str) -> String {
    format!("error: {path} is a directory, and a line range is a file's; ask peek {path} for its entries\n")
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
        ErrorKind::NotFound if matches!(ranged(path), Some(Ranged::Lines { .. })) => (
            "no such file or directory".to_string(),
            format!("line ranges are peek's: run peek {path}"),
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
        assert!(nf("x.rs:1-9").contains("run peek x.rs:1-9"));
        assert!(!nf("x.rs:1-").contains("line ranges"));
        assert!(!nf(":1-9").contains("line ranges"));
        assert!(error_line("x", ErrorKind::PermissionDenied, "")
            .starts_with("error: permission denied; "));
        assert_eq!(header("a"), "== a\n");
        assert!(help().contains("Exits 2 when any argument failed"));
    }

    #[test]
    fn a_range_is_only_a_trailing_colon_digits_dash_digits() {
        let lines = |p: &str, a, b| {
            Some(Ranged::Lines {
                path: p.into(),
                range: Range::new(a, b).unwrap(),
            })
        };
        assert_eq!(ranged("a.rs"), None);
        assert_eq!(ranged("a.rs:3-9"), lines("a.rs", 3, 9));
        assert_eq!(ranged("a:b:2-2"), lines("a:b", 2, 2));
        assert_eq!(ranged("a.rs:3"), None);
        assert_eq!(ranged("a.rs:3-"), None);
        assert_eq!(ranged("a.rs:x-2"), None);
        assert_eq!(Range::new(0, 1), None);
        assert_eq!(Range::new(2, 1), None);
    }

    #[test]
    fn a_bad_range_is_refused_with_the_range_to_ask() {
        let refused = |arg| match ranged(arg) {
            Some(Ranged::Refused(e)) => e,
            r => panic!("{arg}: {r:?}"),
        };
        assert!(refused("a:0-3").contains("ask peek a:1-3"));
        assert!(refused("a:0-0").contains("ask peek a:1-1"));
        assert!(refused("a:5-2").contains("ask peek a:2-5"));
        assert!(refused(":1-2").contains("peek <file>:1-2"));
        assert!(refused("a:1-99999999999999999999999").contains("too large"));
        assert!(range_on_dir("d").starts_with("error: d is a directory"));
    }

    #[test]
    fn a_range_body_prints_its_lines_and_what_is_past_the_end() {
        let r = |a, b| Range::new(a, b).unwrap();
        assert_eq!(
            range_body("f", b"a\nb\nc", r(2, 3)),
            (b"2: b\n3: c\n".to_vec(), true)
        );
        let (b, ok) = range_body("f", b"a\nb\nc\n", r(2, 9));
        assert!(!ok);
        assert_eq!(
            String::from_utf8(b).unwrap(),
            "2: b\n3: c\nerror: lines 4-9 are past the end, f has 3 lines; ask peek f:2-3\n"
        );
        let (b, ok) = range_body("f", b"a\n", r(4, 9));
        assert!(!ok);
        assert!(String::from_utf8(b).unwrap().contains("ask peek f:1-1"));
        let (b, ok) = range_body("f", b"", r(1, 1));
        assert!(!ok && String::from_utf8(b).unwrap().contains("f is empty"));
        assert_eq!(
            range_body("f", b"\x00a", r(1, 1)),
            (b"(binary, 2 bytes)\n".to_vec(), true)
        );
    }
}
