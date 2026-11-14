//! What grep-around does to the world: reads its arguments, checks the
//! pattern, globs and paths, walks the paths with ripgrep's walker
//! (`ignore`), reads each file once, finds its matching lines with
//! ripgrep's matcher and searcher (`grep-regex`, `grep-searcher`), and
//! prints what `render_*` makes of them, file by file as they are found.

use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

use grep_matcher::Matcher;
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::{Searcher, SearcherBuilder, Sink, SinkMatch};
use ignore::overrides::{Override, OverrideBuilder};
use ignore::WalkBuilder;

use crate::{
    help, is_binary, lines, parse, render_matches, render_range, Asked, Fault, Range, Search,
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

fn matcher(s: &Search) -> Result<RegexMatcher, Fault> {
    RegexMatcherBuilder::new()
        .line_terminator(Some(b'\n'))
        .fixed_strings(s.fixed)
        .build(&s.pattern)
        .map_err(|e| Fault {
            message: format!(
                "pattern {:?} is not a valid regular expression: {}",
                s.pattern,
                e.to_string().replace('\n', " ")
            ),
            fix: "escape what it means literally, as fn main\\(, or pass --fixed to match it as written".into(),
        })
}

fn overrides(globs: &[String]) -> Result<Override, Vec<Fault>> {
    let mut b = OverrideBuilder::new(".");
    let mut faults = Vec::new();
    for g in globs {
        if let Err(e) = b.add(g) {
            faults.push(Fault {
                message: format!("--glob {g} is not a glob: {e}"),
                fix: "give a glob as ripgrep's -g takes it, as '*.rs' or 'src/**/*.toml'".into(),
            });
        }
    }
    if !faults.is_empty() {
        return Err(faults);
    }
    b.build().map_err(|e| {
        vec![Fault {
            message: format!("the globs do not build: {e}"),
            fix: "check each --glob".into(),
        }]
    })
}

fn search(s: &Search) -> ExitCode {
    let mut faults = Vec::new();
    let m = matcher(s).map_err(|f| faults.push(f)).ok();
    let ov = overrides(&s.globs).map_err(|f| faults.extend(f)).ok();
    for p in &s.paths {
        if std::fs::metadata(p).is_err() {
            faults.push(Fault {
                message: format!("path {p} does not exist"),
                fix: "give a file or directory that exists, relative to the current directory or absolute".into(),
            });
        }
    }
    let (Some(m), Some(ov)) = (m, ov) else {
        return refuse(&faults);
    };
    if !faults.is_empty() {
        return refuse(&faults);
    }

    let mut walk = WalkBuilder::new(&s.paths[0]);
    for p in &s.paths[1..] {
        walk.add(p);
    }
    walk.overrides(ov)
        .require_git(false)
        .sort_by_file_name(|a, b| a.cmp(b))
        .filter_entry(|e| {
            !(e.depth() > 0
                && e.file_type().is_some_and(|t| t.is_dir())
                && e.file_name() == SKIPPED_DIR)
        });
    let mut searcher = SearcherBuilder::new().line_number(true).build();
    let mut any = false;
    for entry in walk.build() {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                eprintln!("{NAME}: skipped: {e}");
                continue;
            }
        };
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        if let Some(hit) = search_file(&m, &mut searcher, entry.path(), s.context) {
            any = true;
            print(&hit);
        }
    }
    ExitCode::from(if any { EXIT_MATCH } else { EXIT_NO_MATCH })
}

/// What a search prints for one file, `None` when it is binary, cannot
/// be read, or has no match.
fn search_file(
    m: &RegexMatcher,
    searcher: &mut Searcher,
    path: &Path,
    context: u64,
) -> Option<String> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{NAME}: skipped {}: {e}", path.display());
            return None;
        }
    };
    if is_binary(&bytes) || m.find(&bytes).ok().flatten().is_none() {
        return None;
    }
    let mut found = Lines(Vec::new());
    if let Err(e) = searcher.search_slice(m, &bytes, &mut found) {
        eprintln!("{NAME}: skipped {}: {e}", path.display());
        return None;
    }
    let text = render_matches(
        &path.display().to_string(),
        &lines(&bytes),
        &found.0,
        context,
    );
    (!text.is_empty()).then_some(text)
}

fn range(r: &Range) -> ExitCode {
    let bytes = match std::fs::read(&r.file) {
        Ok(b) => b,
        Err(e) => {
            return refuse(&[Fault {
                message: format!("cannot read {}: {e}", r.file),
                fix: "give a file that exists, relative to the current directory or absolute"
                    .into(),
            }])
        }
    };
    match render_range(r, &lines(&bytes)) {
        Ok(text) => {
            print(&text);
            ExitCode::from(EXIT_MATCH)
        }
        Err(f) => refuse(&[f]),
    }
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
        Ok(Asked::Search(s)) => search(&s),
        Ok(Asked::Range(r)) => range(&r),
        Err(faults) => refuse(&faults),
    }
}
