//! tree-grep: what it decides, without touching the world. The arguments
//! it takes, the line a tree entry prints as, the text it prints for a
//! file holding a match, and the line under a directory it cannot list.
//! Regions around a match are grep-around's, printed as it prints them.
//! `world` walks each directory once, reads each file once, runs
//! ripgrep's matcher on it and prints.

pub mod world;

pub const NAME: &str = "tree-grep";

/// Exit code of a run where a line matched and every directory was listed.
pub const EXIT_MATCH: u8 = 0;
/// Exit code of a run where no line matched and every directory was listed.
pub const EXIT_NO_MATCH: u8 = 1;
/// Exit code of a bad pattern, glob or argument, or a directory missing.
pub const EXIT_BAD: u8 = 2;

/// Levels of the tree printed below each directory, by default.
pub const DEFAULT_DEPTH: u64 = 2;

/// Lines of context before and after each matching line, by default.
pub const DEFAULT_CONTEXT: u64 = 0;

/// The directory tree-grep always skips, as cargo's build output.
pub const SKIPPED_DIR: &str = "target";

/// Spaces a tree entry is indented by per level below the first.
pub const INDENT: usize = 2;

pub const USAGE: &str =
    "usage: tree-grep <pattern> [dir…] [--depth N] [--glob <g>]... [--context N] [--fixed]";

/// What `--help` answers, before its constants are put in by `help`.
const HELP: &str = concat!(
    "usage: tree-grep <pattern> [dir…] [--depth N] [--glob <g>]... [--context N] [--fixed]\n",
    "       tree-grep --version\n\n",
    "Lists a directory's tree and greps it in one call, instead of ls, ls again one level down, and grep. It prints plain text, never JSON. Read-only.\n\n",
    "For each <dir>, in the order given (. when none is given), it prints:\n",
    "  == tree <dir>             the directory as given\n",
    "  <name>                    each entry down to --depth levels, one per line, sorted by name within its directory, each directory's entries right under it, indented {INDENT} spaces per level below the first; a directory's name ends in /\n",
    "  == <file>                 then each file under <dir>, at any depth, holding a line that matches <pattern>: its path as walked from <dir>, ./ dropped, files in path order\n",
    "  <line>: <text>            each matching line, its 1-based number, a colon, a space and the line as it is\n",
    "With --context N above 0, each match prints as grep-around prints its region instead: the line with up to N lines before and after it, regions that overlap or touch merged, the matching line marked > before its number and the others a space, -- between regions that do not touch.\n\n",
    "<pattern> is a regular expression in Rust regex syntax (ripgrep's), case-sensitive. The walk honours .gitignore, .ignore and git's exclude files (in a git repository or not) and skips hidden files and directories (.git among them), every directory named {SKIPPED_DIR}, and binary files (a file holding a NUL byte), in the tree and in the search alike.\n\n",
    "A <dir> that does not exist, or is not a directory, prints one line under its header, error: <what>; fix: <the fix>, and the others still print.\n\n",
    "options:\n",
    "  --depth N, -d N           levels of the tree to print; default {DEFAULT_DEPTH}, 0 for the headers alone. The search goes to every depth all the same\n",
    "  --glob <g>, -g <g>        search only files whose path matches the glob, as ripgrep's -g: *.rs, src/**/*.toml; !<g> skips the files it matches instead. Repeatable; the last that matches a file wins. The tree lists every file all the same\n",
    "  --context N, -C N         lines of context before and after each matching line; default {DEFAULT_CONTEXT}, the matching lines alone\n",
    "  --fixed, -F               <pattern> is a fixed string, not a regular expression: fn main( matches as written\n",
    "  --                        end of options: what follows is the pattern and directories, even when it starts with -\n",
    "  -h, --help                print this help\n",
    "  -V, --version             print tree-grep's version\n\n",
    "Exits {EXIT_MATCH} when a line matched, {EXIT_NO_MATCH} when none did (the trees still print), {EXIT_BAD} when a <dir> was missing or not a directory (the others still print); and {EXIT_BAD}, printing nothing on stdout, with the usage and one line per fault on stderr, each naming its fix, on a pattern that is not a valid regular expression, a glob that is not one, or an argument it does not take. A file it cannot read mid-walk is named on stderr and skipped.\n\n",
    "A file's regions alone are grep-around's (grep-around <pattern> <path> or <file>:<from>-<to>); a read of files and directories mixed is peek's.\n",
);

/// What `--help` answers, its constants as the code says.
pub fn help() -> String {
    HELP.replace("{EXIT_MATCH}", &EXIT_MATCH.to_string())
        .replace("{EXIT_NO_MATCH}", &EXIT_NO_MATCH.to_string())
        .replace("{EXIT_BAD}", &EXIT_BAD.to_string())
        .replace("{DEFAULT_DEPTH}", &DEFAULT_DEPTH.to_string())
        .replace("{DEFAULT_CONTEXT}", &DEFAULT_CONTEXT.to_string())
        .replace("{SKIPPED_DIR}", SKIPPED_DIR)
        .replace("{INDENT}", &INDENT.to_string())
}

/// A run the arguments ask for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Run {
    pub pattern: String,
    /// The directories, `.` when none was given.
    pub dirs: Vec<String>,
    pub depth: u64,
    pub globs: Vec<String>,
    pub context: u64,
    pub fixed: bool,
}

/// What the arguments ask.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked {
    Help,
    Version,
    Run(Run),
}

/// One thing wrong, with what fixes it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fault {
    pub message: String,
    pub fix: String,
}

impl Fault {
    pub fn new(message: impl Into<String>, fix: impl Into<String>) -> Self {
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

/// A number of lines or levels given to `flag`, or the fault it is not one.
fn count(flag: &str, v: &str) -> Result<u64, Fault> {
    v.parse::<u64>().map_err(|_| {
        Fault::new(
            format!("{flag} {v} is not a whole number"),
            format!("give one, as {flag} 3"),
        )
    })
}

/// The arguments, after the program's name.
pub fn parse(args: &[String]) -> Result<Asked, Vec<Fault>> {
    let mut faults = Vec::new();
    let mut positional: Vec<String> = Vec::new();
    let mut globs = Vec::new();
    let mut depth = DEFAULT_DEPTH;
    let mut context = DEFAULT_CONTEXT;
    let mut fixed = false;
    let mut ended = false;
    let mut i = 0;
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
                let example = if name == "--glob" { "'*.rs'" } else { "3" };
                faults.push(Fault::new(
                    format!("{name} takes a value"),
                    format!("give it one, as {name} {example}"),
                ));
                None
            }
        };
        match flag {
            "--" => ended = true,
            "-h" | "--help" => return Ok(Asked::Help),
            "-V" | "--version" => return Ok(Asked::Version),
            "-g" | "--glob" => {
                if let Some(g) = value("--glob") {
                    globs.push(g);
                }
            }
            "-d" | "--depth" => {
                if let Some(v) = value("--depth") {
                    match count("--depth", &v) {
                        Ok(n) => depth = n,
                        Err(f) => faults.push(f),
                    }
                }
            }
            "-C" | "--context" => {
                if let Some(v) = value("--context") {
                    match count("--context", &v) {
                        Ok(n) => context = n,
                        Err(f) => faults.push(f),
                    }
                }
            }
            "-F" | "--fixed" if inline.is_none() => fixed = true,
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
            format!("give a pattern and the directories, as {NAME} 'fn main' crates/peek"),
        ));
    }
    if !faults.is_empty() {
        return Err(faults);
    }
    let pattern = positional.remove(0);
    let dirs = if positional.is_empty() {
        vec![".".to_string()]
    } else {
        positional
    };
    Ok(Asked::Run(Run {
        pattern,
        dirs,
        depth,
        globs,
        context,
        fixed,
    }))
}

/// The header above a directory's tree.
pub fn tree_header(dir: &str) -> String {
    format!("== tree {dir}\n")
}

/// The line a tree entry prints as: `level` 1 for the directory's own
/// entries, indented `INDENT` spaces per level below that, a directory's
/// name ending in `/`.
pub fn tree_line(level: u64, name: &str, is_dir: bool) -> String {
    let pad = " ".repeat(INDENT * level.saturating_sub(1) as usize);
    let slash = if is_dir { "/" } else { "" };
    format!("{pad}{name}{slash}\n")
}

/// Why a directory given is not listed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NotListed {
    Missing,
    NotADirectory,
}

/// The line under a directory's header when it is not listed.
pub fn not_listed(dir: &str, why: NotListed) -> String {
    let f = match why {
        NotListed::Missing => Fault::new(
            format!("{dir} does not exist"),
            "give a directory that exists, relative to the current directory or absolute",
        ),
        NotListed::NotADirectory => Fault::new(
            format!("{dir} is not a directory"),
            format!("give its directory, or read the file with peek {dir}"),
        ),
    };
    format!("error: {f}\n")
}

/// What one file holding a match prints: its header, then each matching
/// line numbered, or, with `context` above 0, grep-around's regions.
/// Empty when no line matched.
pub fn render_file(path: &str, lines: &[String], matches: &[u64], context: u64) -> String {
    if context > 0 {
        return grep_around::render_matches(path, lines, matches, context);
    }
    let mut hits: Vec<u64> = matches
        .iter()
        .copied()
        .filter(|&m| m >= 1 && m <= lines.len() as u64)
        .collect();
    hits.sort_unstable();
    hits.dedup();
    if hits.is_empty() {
        return String::new();
    }
    let mut out = format!("== {}\n", grep_around::shown(path));
    for n in hits {
        out.push_str(&format!("{n}: {}\n", lines[(n - 1) as usize]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn options_parse() {
        let Ok(Asked::Run(r)) = parse(&args(&[
            "fn",
            "a",
            "-g",
            "*.rs",
            "--depth=3",
            "-C",
            "1",
            "-F",
            "b",
        ])) else {
            panic!()
        };
        assert_eq!(r.dirs, args(&["a", "b"]));
        assert_eq!(r.globs, args(&["*.rs"]));
        assert_eq!((r.depth, r.context, r.fixed), (3, 1, true));
        let Ok(Asked::Run(r)) = parse(&args(&["--", "-x"])) else {
            panic!()
        };
        assert_eq!(
            (r.pattern.as_str(), r.dirs, r.depth),
            ("-x", args(&["."]), 2)
        );
    }

    #[test]
    fn tree_lines_indent_and_slash() {
        assert_eq!(tree_line(1, "src", true), "src/\n");
        assert_eq!(tree_line(2, "lib.rs", false), "  lib.rs\n");
        assert_eq!(tree_line(3, "x", true), "    x/\n");
    }

    #[test]
    fn render_without_context_numbers_matching_lines_only() {
        let l = args(&["a", "b", "a"]);
        assert_eq!(render_file("./f", &l, &[3, 1, 9], 0), "== f\n1: a\n3: a\n");
        assert_eq!(render_file("f", &l, &[], 0), "");
    }
}
