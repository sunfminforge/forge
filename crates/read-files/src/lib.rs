//! read-files: what it decides, without touching the world. The
//! arguments it takes, a range as only its constructor allows it, the
//! header and the numbered line it prints, and what each file's line
//! count makes of a range. `world` opens the files, reads and prints.

pub mod world;

use std::fmt;

pub const NAME: &str = "read-files";

/// Exit code when any argument failed: a file it cannot read, a range
/// past the end, a range it does not take.
pub const EXIT_FAILED: u8 = 1;
/// Exit code of an option read-files does not take, or no path at all.
pub const EXIT_USAGE: u8 = 2;

pub const USAGE: &str = "usage: read-files <path>[:<start>-<end>] …";

/// What `--help` answers, before its exit codes are put in by `help`.
const HELP: &str = concat!(
    "usage: read-files <path>[:<start>-<end>] …\n",
    "       read-files -- <path>[:<start>-<end>] …\n",
    "       read-files --version\n\n",
    "Prints each named file in the order named, in one call instead of cat, cat and sed -n 'a,bp'. A bare <path> prints the whole file; <path>:<start>-<end> prints only its lines <start> to <end>, both counted from 1 and both included. Example: read-files Cargo.toml src/lib.rs src/main.rs:40-80. It only reads: no file it names is written or changed.\n\n",
    "Each file is printed under a header line holding the argument as given, '==> <path> <==' or '==> <path>:<start>-<end> <==', with a blank line before every header but the first. Each line of the file follows as its line number right-aligned in 6 columns, a tab, then the line as it is in the file, bytes unchanged; a last line without a newline gets one. An empty file prints its header and no lines.\n\n",
    "An argument is a range when it ends in ':' then digits '-' digits; any other argument is a path, ':' and all. <start> is 1 or more and <end> is <start> or more.\n\n",
    "options:\n",
    "  --                        end of read-files' options: what follows is paths, even when one starts with -\n",
    "  -h, --help                print this help\n",
    "  -V, --version             print read-files' version\n\n",
    "A failed argument is said on stderr in one line, 'read-files: <argument>: <what failed>; <the fix>', and the others are still printed: a file it cannot open or read prints nothing; a range whose <start> is past the file's last line prints nothing; a range whose <end> is past it prints its header and the lines that are there, then says which lines were not. A range read-files does not take ('x:0-3', 'x:5-2') prints nothing.\n\n",
    "Exits 0 when every argument printed whole. Exits {EXIT_FAILED} when any argument failed, after printing all the others. Exits {EXIT_USAGE} with the usage on stderr, printing nothing, when no path is named or an option is one it does not take.\n",
);

/// What `--help` answers, its exit codes as the constants say.
pub fn help() -> String {
    HELP.replace("{EXIT_FAILED}", &EXIT_FAILED.to_string())
        .replace("{EXIT_USAGE}", &EXIT_USAGE.to_string())
}

/// A line range, both ends counted from 1 and included. Only `new`
/// makes one, so every range has 1 <= start <= end.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    start: u64,
    end: u64,
}

impl Range {
    pub fn new(start: u64, end: u64) -> Option<Range> {
        (1 <= start && start <= end).then_some(Range { start, end })
    }
    pub fn start(self) -> u64 {
        self.start
    }
    pub fn end(self) -> u64 {
        self.end
    }
    /// Whether line `n` (from 1) is in the range.
    pub fn holds(self, n: u64) -> bool {
        self.start <= n && n <= self.end
    }
}

impl fmt::Display for Range {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.start, self.end)
    }
}

/// One argument: the file, and the lines of it asked for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Spec {
    /// The argument as given, as the header and stderr name it.
    pub arg: String,
    pub path: String,
    /// `None`: the whole file.
    pub range: Option<Range>,
}

/// What one argument asks: a file to print, or a range refused before
/// any file is opened, with its reason and fix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    Read(Spec),
    Refused { arg: String, reason: String },
}

/// What the arguments ask.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked {
    Help,
    Version,
    Read(Vec<Item>),
}

/// Splits `digits-digits` into its two numbers; `None` when it is not
/// that shape.
fn two_numbers(s: &str) -> Option<(&str, &str)> {
    let (a, b) = s.split_once('-')?;
    let digits = |t: &str| !t.is_empty() && t.bytes().all(|c| c.is_ascii_digit());
    (digits(a) && digits(b)).then_some((a, b))
}

/// One argument: a range when it ends in `:digits-digits`, else a path.
pub fn item(arg: &str) -> Item {
    let read = |path: &str, range| {
        Item::Read(Spec {
            arg: arg.to_string(),
            path: path.to_string(),
            range,
        })
    };
    let Some((path, suffix)) = arg.rsplit_once(':') else {
        return read(arg, None);
    };
    let Some((a, b)) = two_numbers(suffix) else {
        return read(arg, None);
    };
    if path.is_empty() {
        return Item::Refused {
            arg: arg.to_string(),
            reason: "no path before the range; name the file, as <path>:<start>-<end>".into(),
        };
    }
    let (Ok(start), Ok(end)) = (a.parse::<u64>(), b.parse::<u64>()) else {
        return Item::Refused {
            arg: arg.to_string(),
            reason: format!("line number too large; give lines below {}", u64::MAX),
        };
    };
    match Range::new(start, end) {
        Some(r) => read(path, Some(r)),
        None if start == 0 => Item::Refused {
            arg: arg.to_string(),
            reason: format!("lines count from 1; write {path}:1-{}", end.max(1)),
        },
        None => Item::Refused {
            arg: arg.to_string(),
            reason: format!("start {start} is past end {end}; write {path}:{end}-{start}"),
        },
    }
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
                "no such option: {f}; to read a file named {f}, put -- before it"
            ))
        }
        Some(_) => args,
    };
    if rest.is_empty() {
        return Err("name at least one path after --".into());
    }
    Ok(Asked::Read(rest.iter().map(|a| item(a)).collect()))
}

/// The header line of one argument, newline included.
pub fn header(arg: &str) -> String {
    format!("==> {arg} <==\n")
}

/// The start of line `n` as printed: its number in 6 columns and a tab.
pub fn number(n: u64) -> String {
    format!("{n:>6}\t")
}

/// What a range comes to once the file's line count is known.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    /// Every line asked for is there.
    Whole,
    /// Lines `from` to the range's end are past the last line, `lines`;
    /// the ones before it were printed.
    Short { from: u64, lines: u64 },
    /// The range starts past the last line, `lines`: nothing printed.
    Past { lines: u64 },
}

/// How a range fits a file of `lines` lines.
pub fn fit(range: Range, lines: u64) -> Fit {
    if range.start() > lines {
        Fit::Past { lines }
    } else if range.end() > lines {
        Fit::Short {
            from: lines + 1,
            lines,
        }
    } else {
        Fit::Whole
    }
}

/// The failure a fit says on stderr, `None` when it fits.
pub fn fit_failure(spec_path: &str, range: Range, fit: Fit) -> Option<String> {
    let last = |lines: u64| match lines {
        0 => "it is empty".to_string(),
        1 => "it has 1 line".to_string(),
        n => format!("it has {n} lines"),
    };
    match fit {
        Fit::Whole => None,
        Fit::Past { lines } => Some(format!(
            "range starts past the end, {}; read it whole as {spec_path}{}",
            last(lines),
            match lines {
                0 => String::new(),
                n => format!(", or ask {spec_path}:{}-{n}", range.start().min(n)),
            }
        )),
        Fit::Short { from, lines } => Some(format!(
            "lines {from}-{} are past the end, {}; ask {spec_path}:{}-{lines}",
            range.end(),
            last(lines),
            range.start()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read(arg: &str, path: &str, range: Option<(u64, u64)>) -> Item {
        Item::Read(Spec {
            arg: arg.into(),
            path: path.into(),
            range: range.map(|(a, b)| Range::new(a, b).unwrap()),
        })
    }

    #[test]
    fn a_range_is_only_a_trailing_colon_digits_dash_digits() {
        assert_eq!(item("a.rs"), read("a.rs", "a.rs", None));
        assert_eq!(item("a.rs:3-9"), read("a.rs:3-9", "a.rs", Some((3, 9))));
        assert_eq!(item("a:b:2-2"), read("a:b:2-2", "a:b", Some((2, 2))));
        assert_eq!(item("a.rs:3"), read("a.rs:3", "a.rs:3", None));
        assert_eq!(item("a.rs:3-"), read("a.rs:3-", "a.rs:3-", None));
        assert_eq!(item("a.rs:x-2"), read("a.rs:x-2", "a.rs:x-2", None));
        assert!(matches!(item(":1-2"), Item::Refused { .. }));
    }

    #[test]
    fn a_range_refused_says_the_one_it_takes() {
        let Item::Refused { reason, .. } = item("a:0-3") else {
            panic!()
        };
        assert!(reason.contains("a:1-3"), "{reason}");
        let Item::Refused { reason, .. } = item("a:5-2") else {
            panic!()
        };
        assert!(reason.contains("a:2-5"), "{reason}");
        assert!(matches!(
            item("a:1-99999999999999999999999"),
            Item::Refused { .. }
        ));
        assert_eq!(Range::new(0, 1), None);
        assert_eq!(Range::new(2, 1), None);
    }

    #[test]
    fn options_come_first_and_dash_dash_ends_them() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(parse(&s(&["--help"])), Ok(Asked::Help));
        assert_eq!(parse(&s(&["-V"])), Ok(Asked::Version));
        assert!(parse(&[]).is_err());
        assert!(parse(&s(&["--"])).is_err());
        assert!(parse(&s(&["-x"])).unwrap_err().contains("put -- before it"));
        assert_eq!(
            parse(&s(&["--", "-x"])),
            Ok(Asked::Read(vec![read("-x", "-x", None)]))
        );
        assert_eq!(
            parse(&s(&["a", "--help"])),
            Ok(Asked::Read(vec![
                read("a", "a", None),
                read("--help", "--help", None)
            ]))
        );
    }

    #[test]
    fn a_range_fits_whole_short_or_past() {
        let r = Range::new(3, 5).unwrap();
        assert_eq!(fit(r, 5), Fit::Whole);
        assert_eq!(fit(r, 4), Fit::Short { from: 5, lines: 4 });
        assert_eq!(fit(r, 2), Fit::Past { lines: 2 });
        assert_eq!(fit_failure("a", r, Fit::Whole), None);
        assert!(fit_failure("a", r, fit(r, 4))
            .unwrap()
            .contains("ask a:3-4"));
        assert!(fit_failure("a", r, fit(r, 2))
            .unwrap()
            .contains("ask a:2-2"));
        assert!(fit_failure("a", r, fit(r, 0)).unwrap().contains("empty"));
    }

    #[test]
    fn header_and_number_as_printed() {
        assert_eq!(header("a:1-3"), "==> a:1-3 <==\n");
        assert_eq!(number(7), "     7\t");
        assert!(help().contains("Exits 1 when") && help().contains("Exits 2 with"));
    }
}
