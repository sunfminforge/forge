//! What tree-grep does to the world: reads its arguments, checks the
//! pattern and globs, then per directory walks it once with ripgrep's
//! walker (`ignore`), to every depth, printing each entry down to the
//! depth asked as it is walked, reading each file once to skip it when
//! binary and to find its matching lines with ripgrep's matcher and
//! searcher (`grep-regex`, `grep-searcher`), and prints what `render_file`
//! makes of them after the directory's tree.

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use grep_around::{is_binary, lines};
use grep_matcher::Matcher;
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::{Searcher, SearcherBuilder, Sink, SinkMatch};
use ignore::overrides::{Override, OverrideBuilder};
use ignore::WalkBuilder;

use crate::{
    help, not_listed, parse, render_file, tree_header, tree_line, Asked, Fault, NotListed, Run,
    EXIT_BAD, EXIT_MATCH, EXIT_NO_MATCH, NAME, SKIPPED_DIR, USAGE,
};

/// Refuses: the usage, then each fault, on stderr.
fn refuse(faults: &[Fault]) -> ExitCode {
    let mut e = std::io::stderr().lock();
    let _ = writeln!(e, "{USAGE}");
    for f in faults {
        let _ = writeln!(e, "{NAME}: error: {f}");
    }
    ExitCode::from(EXIT_BAD)
}

/// Prints to stdout; a closed stdout ends nothing it would change.
fn print(text: &str) {
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(text.as_bytes()).and_then(|()| out.flush());
}

/// The line numbers of a file's matching lines, as ripgrep's searcher
/// reports them.
struct Lines(Vec<u64>);

impl Sink for Lines {
    type Error = std::io::Error;
    fn matched(&mut self, _: &Searcher, m: &SinkMatch<'_>) -> Result<bool, Self::Error> {
        if let Some(n) = m.line_number() {
            self.0.push(n);
        }
        Ok(true)
    }
}

fn matcher(r: &Run) -> Result<RegexMatcher, Fault> {
    RegexMatcherBuilder::new()
        .line_terminator(Some(b'\n'))
        .fixed_strings(r.fixed)
        .build(&r.pattern)
        .map_err(|e| {
            Fault::new(
                format!(
                    "pattern {:?} is not a valid regular expression: {}",
                    r.pattern,
                    e.to_string().replace('\n', " ")
                ),
                "escape what it means literally, as fn main\\(, or pass --fixed to match it as written",
            )
        })
}

fn overrides(globs: &[String]) -> Result<Override, Vec<Fault>> {
    let mut b = OverrideBuilder::new(".");
    let mut faults = Vec::new();
    for g in globs {
        if let Err(e) = b.add(g) {
            faults.push(Fault::new(
                format!("--glob {g} is not a glob: {e}"),
                "give a glob as ripgrep's -g takes it, as '*.rs' or 'src/**/*.toml'",
            ));
        }
    }
    if !faults.is_empty() {
        return Err(faults);
    }
    b.build().map_err(|e| {
        vec![Fault::new(
            format!("the globs do not build: {e}"),
            "check each --glob",
        )]
    })
}

/// Whether the globs let a file be searched: the last glob matching it
/// decides; with no glob matching, it is searched unless some glob is a
/// whitelist.
fn searched(ov: &Override, path: &Path) -> bool {
    let m = ov.matched(path, false);
    if m.is_whitelist() {
        return true;
    }
    if m.is_ignore() {
        return false;
    }
    ov.num_whitelists() == 0
}

/// Lists and searches one directory, printing its tree as it is walked
/// and its matches after. Whether it was listed, and whether a line
/// matched.
fn one_dir(
    dir: &str,
    r: &Run,
    m: &RegexMatcher,
    ov: &Override,
    searcher: &mut Searcher,
) -> (bool, bool) {
    print(&tree_header(dir));
    match std::fs::metadata(dir) {
        Err(_) => {
            print(&not_listed(dir, NotListed::Missing));
            return (false, false);
        }
        Ok(md) if !md.is_dir() => {
            print(&not_listed(dir, NotListed::NotADirectory));
            return (false, false);
        }
        Ok(_) => {}
    }
    let walk = WalkBuilder::new(dir)
        .require_git(false)
        .sort_by_file_name(|a, b| a.cmp(b))
        .filter_entry(|e| {
            !(e.depth() > 0
                && e.file_type().is_some_and(|t| t.is_dir())
                && e.file_name() == SKIPPED_DIR)
        })
        .build();
    let mut found = String::new();
    for entry in walk {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                eprintln!("{NAME}: skipped: {e}");
                continue;
            }
        };
        let level = entry.depth() as u64;
        if level == 0 {
            continue;
        }
        let in_tree = level <= r.depth;
        let name = entry.file_name().to_string_lossy();
        let Some(kind) = entry.file_type() else {
            continue;
        };
        if !kind.is_file() {
            if in_tree {
                print(&tree_line(level, &name, kind.is_dir()));
            }
            continue;
        }
        let search = searched(ov, entry.path());
        if !in_tree && !search {
            continue;
        }
        let bytes = match std::fs::read(entry.path()) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("{NAME}: skipped {}: {e}", entry.path().display());
                continue;
            }
        };
        if is_binary(&bytes) {
            continue;
        }
        if in_tree {
            print(&tree_line(level, &name, false));
        }
        if search {
            found.push_str(&search_file(m, searcher, entry.path(), &bytes, r.context));
        }
    }
    print(&found);
    (true, !found.is_empty())
}

/// What one file prints, empty when no line matched.
fn search_file(
    m: &RegexMatcher,
    searcher: &mut Searcher,
    path: &Path,
    bytes: &[u8],
    context: u64,
) -> String {
    if m.find(bytes).ok().flatten().is_none() {
        return String::new();
    }
    let mut hits = Lines(Vec::new());
    if let Err(e) = searcher.search_slice(m, bytes, &mut hits) {
        eprintln!("{NAME}: skipped {}: {e}", path.display());
        return String::new();
    }
    render_file(&path.display().to_string(), &lines(bytes), &hits.0, context)
}

fn run(r: &Run) -> ExitCode {
    let mut faults = Vec::new();
    let m = matcher(r).map_err(|f| faults.push(f)).ok();
    let ov = overrides(&r.globs).map_err(|f| faults.extend(f)).ok();
    let (Some(m), Some(ov)) = (m, ov) else {
        return refuse(&faults);
    };
    let mut searcher = SearcherBuilder::new().line_number(true).build();
    let mut all_listed = true;
    let mut any = false;
    for dir in &r.dirs {
        let (listed, matched) = one_dir(dir, r, &m, &ov, &mut searcher);
        all_listed &= listed;
        any |= matched;
    }
    ExitCode::from(match (all_listed, any) {
        (false, _) => EXIT_BAD,
        (true, true) => EXIT_MATCH,
        (true, false) => EXIT_NO_MATCH,
    })
}

pub fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match parse(&args) {
        Ok(Asked::Help) => {
            print(&help());
            ExitCode::SUCCESS
        }
        Ok(Asked::Version) => {
            println!("{NAME} {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Ok(Asked::Run(r)) => run(&r),
        Err(faults) => refuse(&faults),
    }
}
