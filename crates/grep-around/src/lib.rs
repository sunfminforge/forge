//! grep-around: what it decides, without touching the world. The
//! arguments it takes, which form they ask for (a search or a line
//! range), the regions around the matching lines, merged where they
//! overlap or touch, and the text it prints for a file. `world` walks the
//! paths, reads the files, runs ripgrep's matcher on them and prints.

pub mod world;

pub const NAME: &str = "grep-around";

/// Exit code of a search that matched at least one line, and of a range
/// printed.
pub const EXIT_MATCH: u8 = 0;
/// Exit code of a search that matched no line.
pub const EXIT_NO_MATCH: u8 = 1;
/// Exit code of a bad pattern, path, range or argument.
pub const EXIT_BAD: u8 = 2;

/// Lines of context before and after each matching line, by default.
pub const DEFAULT_CONTEXT: u64 = 10;

/// The directory grep-around always skips, as cargo's build output.
pub const SKIPPED_DIR: &str = "target";

/// The mark before a matching line's number; other lines have a space.
pub const MATCH_MARK: char = '>';

/// The line between two regions of one file that do not touch.
pub const REGION_SEPARATOR: &str = "--";

pub const USAGE: &str = "usage: grep-around <pattern> [path…] [--glob <g>]... [--context N] [--fixed] | grep-around <file>:<from>-<to>";

/// What `--help` answers, before its constants are put in by `help`.
const HELP: &str = concat!(
    "usage: grep-around <pattern> [path…] [--glob <g>]... [--context N] [--fixed]\n",
    "       grep-around <file>:<from>-<to>\n",
    "       grep-around --version\n\n",
    "Finds a pattern and reads around it in one call, instead of grep for the files, grep -n for the line numbers and sed -n for the lines around them. It prints plain text, never JSON.\n\n",
    "Search: grep-around <pattern> [path…] searches each path (a directory recursively, or a file; . when none is given) for lines matching <pattern>, a regular expression in Rust regex syntax (ripgrep's), case-sensitive. A directory is walked honouring .gitignore, .ignore and git's exclude files (in a git repository or not), skipping hidden files and directories, every directory named {SKIPPED_DIR}, and binary files (a file holding a NUL byte). Files are searched in path order. For each file with a match it prints:\n",
    "  == <file>                 the file's path as walked from the path given, ./ dropped\n",
    "  {MATCH_MARK}<line>: <text>            a matching line: the mark {MATCH_MARK}, its 1-based number, a colon, a space and the line as it is\n",
    "   <line>: <text>            a line of context, a space where the mark would be\n",
    "  {REGION_SEPARATOR}                        between two regions of the file that do not touch\n",
    "Each matching line's region is the line with up to N lines before and after it; regions that overlap or touch are merged into one, so no line prints twice.\n\n",
    "Range: grep-around <file>:<from>-<to>, given alone, prints == <file> and then lines <from> to <to> of the file, 1-based and inclusive, each as <line>: <text>, with no mark; a <to> past the file's end stops at its last line. To search for a pattern shaped like <x>:<n>-<m>, give a path after it.\n\n",
    "options:\n",
    "  --glob <g>, -g <g>        search only files whose path matches the glob, as ripgrep's -g: *.rs, src/**/*.toml; !<g> skips the files it matches instead. Repeatable; the last that matches a file wins\n",
    "  --context N, -C N         lines of context before and after each matching line; default {DEFAULT_CONTEXT}, 0 for the matching lines alone\n",
    "  --fixed, -F               <pattern> is a fixed string, not a regular expression: fn main( matches as written\n",
    "  --                        end of options: what follows is the pattern and paths, even when it starts with -\n",
    "  -h, --help                print this help\n",
    "  -V, --version             print grep-around's version\n\n",
    "Exits {EXIT_MATCH} when a line matched, or the range printed; {EXIT_NO_MATCH} when no line matched, printing nothing; {EXIT_BAD} with the usage and one line per fault on stderr, each naming its fix, on a pattern that is not a valid regular expression, a path that does not exist, a range that is not one, or an argument it does not take; nothing is searched then. A file it cannot read mid-walk is named on stderr and skipped.\n",
);

/// What `--help` answers, its constants as the code says.
pub fn help() -> String {
    HELP.replace("{EXIT_MATCH}", &EXIT_MATCH.to_string())
        .replace("{EXIT_NO_MATCH}", &EXIT_NO_MATCH.to_string())
        .replace("{EXIT_BAD}", &EXIT_BAD.to_string())
        .replace("{DEFAULT_CONTEXT}", &DEFAULT_CONTEXT.to_string())
        .replace("{SKIPPED_DIR}", SKIPPED_DIR)
        .replace("{MATCH_MARK}", &MATCH_MARK.to_string())
        .replace("{REGION_SEPARATOR}", REGION_SEPARATOR)
}

/// A search the arguments ask for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Search {
    pub pattern: String,
    /// The paths to search, `.` when none was given.
    pub paths: Vec<String>,
    pub globs: Vec<String>,
    pub context: u64,
    pub fixed: bool,
}

/// A line range of one file: `from` at least 1, `to` at least `from`.
/// Made only by `parse_range`, which checks it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Range {
    pub file: String,
    from: u64,
    to: u64,
}

impl Range {
    pub fn from(&self) -> u64 {
        self.from
    }
    pub fn to(&self) -> u64 {
        self.to
    }
}

/// What the arguments ask.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked {
    Help,
    Version,
    Search(Search),
    Range(Range),
}

/// One thing wrong, with the edit that fixes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fault {
    pub message: String,
    pub fix: String,
}

impl Fault {
    fn new(message: impl Into<String>, fix: impl Into<String>) -> Self {
        Fault {
            message: message.into(),
            fix: fix.into(),
        }
    }
}

impl std::fmt::Display for Fault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}; fix: {}", self.message, self.fix)
    }
}

/// `<file>:<from>-<to>` split, when the argument has that shape: a
/// non-empty file, then digits, a dash and digits at its end.
fn range_shape(arg: &str) -> Option<(&str, &str, &str)> {
    let (file, nums) = arg.rsplit_once(':')?;
    let (from, to) = nums.split_once('-')?;
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    if file.is_empty() || !digits(from) || !digits(to) {
        return None;
    }
    Some((file, from, to))
}

/// The range an argument shaped `<file>:<from>-<to>` names, or why it is
/// not one. `None` when the argument does not have the shape.
pub fn parse_range(arg: &str) -> Option<Result<Range, Fault>> {
    let (file, from, to) = range_shape(arg)?;
    let fix = format!("give {file}:<from>-<to> with 1 <= from <= to, as {file}:1-20");
    let (Ok(from), Ok(to)) = (from.parse::<u64>(), to.parse::<u64>()) else {
        return Some(Err(Fault::new(
            format!("range {arg}: a line number is too large"),
            fix,
        )));
    };
    if from == 0 {
        return Some(Err(Fault::new(
            format!("range {arg}: lines are numbered from 1"),
            fix,
        )));
    }
    if from > to {
        return Some(Err(Fault::new(
            format!("range {arg}: {from} is after {to}"),
            fix,
        )));
    }
    Some(Ok(Range {
        file: file.to_string(),
        from,
        to,
    }))
}

/// The arguments, after the program's name.
pub fn parse(args: &[String]) -> Result<Asked, Vec<Fault>> {
    let mut faults = Vec::new();
    let mut positional: Vec<String> = Vec::new();
    let mut globs = Vec::new();
    let mut context = None;
    let mut fixed = false;
    let mut flags = false;
    let mut i = 0;
    let mut ended = false;
    while i < args.len() {
        let a = &args[i];
        i += 1;
        if ended || a == "-" || !a.starts_with('-') {
            positional.push(a.clone());
            continue;
        }
        let (flag, inline) = match a.split_once('=') {
            Some((f, v)) if f.starts_with("--") => (f, Some(v.to_string())),
            _ => (a.as_str(), None),
        };
        let mut value = |name: &str| -> Option<String> {
            if let Some(v) = inline.clone() {
                return Some(v);
            }
            if i < args.len() {
                i += 1;
                Some(args[i - 1].clone())
            } else {
                faults.push(Fault::new(
                    format!("{name} takes a value"),
                    format!(
                        "give it one, as {name} {}",
                        if name == "--glob" { "'*.rs'" } else { "3" }
                    ),
                ));
                None
            }
        };
        match flag {
            "--" => ended = true,
            "-h" | "--help" => return Ok(Asked::Help),
            "-V" | "--version" => return Ok(Asked::Version),
            "-g" | "--glob" => {
                flags = true;
                if let Some(g) = value("--glob") {
                    globs.push(g);
                }
            }
            "-C" | "--context" => {
                flags = true;
                if let Some(v) = value("--context") {
                    match v.parse::<u64>() {
                        Ok(n) => context = Some(n),
                        Err(_) => faults.push(Fault::new(
                            format!("--context {v} is not a number of lines"),
                            "give a whole number, as --context 3",
                        )),
                    }
                }
            }
            "-F" | "--fixed" if inline.is_none() => {
                flags = true;
                fixed = true;
            }
            _ => faults.push(Fault::new(
                format!("{a} is not an option of {NAME}"),
                format!(
                    "drop it, or put -- before a pattern that starts with -; see {NAME} --help"
                ),
            )),
        }
    }
    if positional.is_empty() {
        faults.push(Fault::new(
            "no pattern given",
            format!(
                "give a pattern, as {NAME} 'fn main' src, or a range, as {NAME} src/main.rs:1-20"
            ),
        ));
    }
    if !faults.is_empty() {
        return Err(faults);
    }
    if positional.len() == 1 && !flags && !ended {
        if let Some(r) = parse_range(&positional[0]) {
            return r.map(Asked::Range).map_err(|f| vec![f]);
        }
    }
    let pattern = positional.remove(0);
    let paths = if positional.is_empty() {
        vec![".".to_string()]
    } else {
        positional
    };
    Ok(Asked::Search(Search {
        pattern,
        paths,
        globs,
        context: context.unwrap_or(DEFAULT_CONTEXT),
        fixed,
    }))
}

/// A run of lines, 1-based and inclusive, `start <= end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub start: u64,
    pub end: u64,
}

/// The regions around the matching lines of a file of `n_lines` lines:
/// each matching line with up to `context` lines before and after it,
/// within the file, regions that overlap or touch merged. `matches` are
/// 1-based line numbers, in any order; those past the file are dropped.
pub fn regions(matches: &[u64], context: u64, n_lines: u64) -> Vec<Region> {
    let mut sorted: Vec<u64> = matches
        .iter()
        .copied()
        .filter(|&m| m >= 1 && m <= n_lines)
        .collect();
    sorted.sort_unstable();
    sorted.dedup();
    let mut out: Vec<Region> = Vec::new();
    for m in sorted {
        let start = m.saturating_sub(context).max(1);
        let end = m.saturating_add(context).min(n_lines);
        match out.last_mut() {
            Some(last) if start <= last.end.saturating_add(1) => last.end = last.end.max(end),
            _ => out.push(Region { start, end }),
        }
    }
    out
}

/// A file's lines, split on `\n`, a `\r` before it dropped, invalid UTF-8
/// shown as U+FFFD. A final line ending does not start another line.
pub fn lines(bytes: &[u8]) -> Vec<String> {
    let mut v: Vec<String> = bytes
        .split(|&b| b == b'\n')
        .map(|l| String::from_utf8_lossy(l.strip_suffix(b"\r").unwrap_or(l)).into_owned())
        .collect();
    if bytes.is_empty() || bytes.ends_with(b"\n") {
        v.pop();
    }
    v
}

/// Whether a file is binary, as grep-around skips it: it holds a NUL byte.
pub fn is_binary(bytes: &[u8]) -> bool {
    bytes.contains(&0)
}

/// A path as the header shows it: `./` at its start dropped.
pub fn shown(path: &str) -> &str {
    let mut p = path;
    while let Some(rest) = p.strip_prefix("./") {
        if rest.is_empty() {
            break;
        }
        p = rest;
    }
    p
}

/// What a search prints for one file: its header, then each region, the
/// matching lines marked, regions apart by the separator. Empty when no
/// line matched.
pub fn render_matches(path: &str, lines: &[String], matches: &[u64], context: u64) -> String {
    let rs = regions(matches, context, lines.len() as u64);
    if rs.is_empty() {
        return String::new();
    }
    let mut out = format!("== {}\n", shown(path));
    for (k, r) in rs.iter().enumerate() {
        if k > 0 {
            out.push_str(REGION_SEPARATOR);
            out.push('\n');
        }
        for n in r.start..=r.end {
            let mark = if matches.contains(&n) {
                MATCH_MARK
            } else {
                ' '
            };
            out.push_str(&format!("{mark}{n}: {}\n", lines[(n - 1) as usize]));
        }
    }
    out
}

/// What a range prints of a file, or why it cannot: `from` past the
/// file's last line.
pub fn render_range(range: &Range, lines: &[String]) -> Result<String, Fault> {
    let n = lines.len() as u64;
    if range.from > n {
        return Err(Fault::new(
            format!(
                "range {}:{}-{}: {} has {n} lines",
                range.file, range.from, range.to, range.file
            ),
            format!("start the range at a line from 1 to {n}"),
        ));
    }
    let mut out = format!("== {}\n", shown(&range.file));
    for k in range.from..=range.to.min(n) {
        out.push_str(&format!("{k}: {}\n", lines[(k - 1) as usize]));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn regions_merge_overlapping_and_touching() {
        // Worked by hand: 3±2 = 1..5, 7±2 = 5..9 overlap; 13±2 = 11..15
        // touches 10? no: 9+1 = 10 < 11, apart.
        assert_eq!(
            regions(&[7, 3, 13], 2, 20),
            vec![Region { start: 1, end: 9 }, Region { start: 11, end: 15 }]
        );
        // 3±1 = 2..4 and 6±1 = 5..7 touch: merged.
        assert_eq!(regions(&[3, 6], 1, 10), vec![Region { start: 2, end: 7 }]);
        // Clamped to the file.
        assert_eq!(regions(&[1, 10], 5, 10), vec![Region { start: 1, end: 10 }]);
        assert_eq!(regions(&[], 5, 10), vec![]);
    }

    #[test]
    fn range_shape_and_bounds() {
        assert!(parse_range("src/main.rs").is_none());
        assert!(parse_range("a:b-c").is_none());
        let r = parse_range("C:/x.rs:3-5").unwrap().unwrap();
        assert_eq!((r.file.as_str(), r.from(), r.to()), ("C:/x.rs", 3, 5));
        assert!(parse_range("x.rs:0-5").unwrap().is_err());
        assert!(parse_range("x.rs:6-5").unwrap().is_err());
    }

    #[test]
    fn a_range_alone_is_a_range_with_a_path_a_pattern() {
        assert!(matches!(parse(&args(&["x.rs:1-2"])), Ok(Asked::Range(_))));
        assert!(matches!(
            parse(&args(&["x.rs:1-2", "."])),
            Ok(Asked::Search(_))
        ));
        assert!(matches!(
            parse(&args(&["--", "x.rs:1-2"])),
            Ok(Asked::Search(_))
        ));
    }

    #[test]
    fn options_parse() {
        let Ok(Asked::Search(s)) = parse(&args(&[
            "fn",
            "a",
            "--glob",
            "*.rs",
            "-g",
            "!x",
            "--context=2",
            "-F",
            "b",
        ])) else {
            panic!()
        };
        assert_eq!(s.paths, args(&["a", "b"]));
        assert_eq!(s.globs, args(&["*.rs", "!x"]));
        assert_eq!(s.context, 2);
        assert!(s.fixed);
    }

    #[test]
    fn every_fault_at_once() {
        let e = parse(&args(&["--bogus", "--context", "x"])).unwrap_err();
        assert_eq!(e.len(), 3, "{e:?}");
    }

    #[test]
    fn lines_split() {
        assert_eq!(lines(b"a\r\nb\n"), args(&["a", "b"]));
        assert_eq!(lines(b"a\n\nb"), args(&["a", "", "b"]));
        assert!(lines(b"").is_empty());
        assert_eq!(shown("./src/x.rs"), "src/x.rs");
        assert_eq!(shown("src/x.rs"), "src/x.rs");
    }
}
