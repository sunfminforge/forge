//! apply-patch: what it decides, without touching the world. The
//! arguments it takes, the patch read from stdin (its envelope is codex's
//! apply_patch grammar, its updates apply-edits' SEARCH/REPLACE blocks),
//! what each change does to the file as found (every change or none),
//! what a refused change says with its fix, and what is printed. `world`
//! reads stdin and the files, writes them, and prints.

pub mod world;

use std::collections::HashMap;
use std::path::{Component, Path};

pub use apply_edits::{Applied, Block};

pub const NAME: &str = "apply-patch";

/// Exit code when any change cannot be made as the patch asks.
pub const EXIT_REFUSED: u8 = 1;
/// Exit code of a malformed patch, a file it cannot write, or a usage
/// error.
pub const EXIT_FAILED: u8 = 2;

pub const USAGE: &str = "usage: apply-patch [--dry-run] < patch";

pub const BEGIN: &str = "*** Begin Patch";
pub const END: &str = "*** End Patch";
pub const ADD: &str = "*** Add File: ";
pub const UPDATE: &str = "*** Update File: ";
pub const DELETE: &str = "*** Delete File: ";
pub const MOVE: &str = "*** Move to: ";

/// What `--help` answers, before its exit codes are put in by `help`.
const HELP: &str = concat!(
    "usage: apply-patch [--dry-run] < patch\n",
    "       apply-patch --version\n\n",
    "Applies one change set to several files in one call: adds files whole, updates files with apply-edits' SEARCH/REPLACE blocks, deletes files, every change or none. Standard input holds one patch:\n\n",
    "  *** Begin Patch\n",
    "  *** Add File: <path>\n",
    "  +each line of the new file, after a '+'\n",
    "  +\n",
    "  *** Update File: <path>\n",
    "  <<<<<<< SEARCH\n",
    "  the exact old text, one or more lines\n",
    "  =======\n",
    "  the new text, zero or more lines\n",
    "  >>>>>>> REPLACE\n",
    "  *** Delete File: <path>\n",
    "  *** End Patch\n\n",
    "Example, adding new.txt and replacing a with b in old.txt:\n",
    "  printf '*** Begin Patch\\n*** Add File: new.txt\\n+hello\\n*** Update File: old.txt\\n<<<<<<< SEARCH\\na\\n=======\\nb\\n>>>>>>> REPLACE\\n*** End Patch\\n' | apply-patch\n\n",
    "How a patch applies:\n",
    "  envelope       the patch opens with '*** Begin Patch' and closes with '*** End Patch', each alone on its line; a patch without its end line is refused as cut short. Between them, one section per file, each opened by its header line; blank lines may sit before the first header and after the end line.\n",
    "  paths          as written after the header, relative to the current directory or absolute. Each file is named once: put all of one file's blocks under its one '*** Update File:'.\n",
    "  add            '*** Add File: <path>' then every line of the new file, each written '+' and its text ('+' alone is an empty line); the file is that text, each line ending in a newline. It must not exist yet; missing parent directories are made.\n",
    "  update         '*** Update File: <path>' then one or more SEARCH/REPLACE blocks, exactly as apply-edits takes them: each search text matched byte for byte, exactly once ('<<<<<<< SEARCH all' replaces every match), blocks in order, an empty REPLACE deletes. The file must exist. A search or replace text cannot hold a line that starts like a header ('*** Add File: ', '*** Update File: ', '*** Delete File: ') or is '*** End Patch'.\n",
    "  delete         '*** Delete File: <path>', nothing under it but blank lines. The file must exist.\n",
    "  every or none  every change is checked against the files as they are before any is written: when one cannot be made, nothing is written and every file stays byte for byte as it was. Then each new or updated file is written to a temp file beside it, and only when all are written are they renamed over their paths and the deleted files removed; a failure there puts back what was already done.\n\n",
    "What it prints, on success, to stdout: per file, in the patch's order, one header, files parted by a blank line:\n",
    "  '== add <path>: <n> line(s)', then every line of the new file as '<line>: <text>'\n",
    "  '== update <path>: <k> block(s)', then per block apply-edits' region: '== block <n>: <m> match(es), line(s) <where>' and the changed lines as '<line>: <text>'\n",
    "  '== delete <path>: <n> line(s) removed'\n\n",
    "Reading files first is peek's (peek <file>, or peek <file>:<from>-<to> for a range); one file's edits alone are apply-edits'.\n\n",
    "options:\n",
    "  --dry-run       check every change and print what it would do, but write nothing\n",
    "  -h, --help      print this help\n",
    "  -V, --version   print apply-patch's version\n\n",
    "Exits 0 when every change applied (and was written, unless --dry-run). Exits {EXIT_REFUSED} when any change cannot be made (an add over a file or directory that exists, an update or delete of a file that does not, a block whose search text matches zero or several times, a file it cannot read): stderr names every such change by its file, what it was to do and its line in the patch, with the fix; nothing is written. Exits {EXIT_FAILED} on a malformed patch (stderr names its line and the fix), a file or directory it cannot write (what was done is put back), standard input left a terminal, or a usage error; nothing is written.\n",
);

/// What `--help` answers, its exit codes as the constants say.
pub fn help() -> String {
    HELP.replace("{EXIT_REFUSED}", &EXIT_REFUSED.to_string())
        .replace("{EXIT_FAILED}", &EXIT_FAILED.to_string())
}

/// What the arguments ask.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked {
    Help,
    Version,
    Apply { dry_run: bool },
}

/// The arguments, after the program's name: `--dry-run` at most; the
/// files are the patch's to name.
pub fn parse(args: &[String]) -> Result<Asked, String> {
    let mut dry_run = false;
    for a in args {
        match a.as_str() {
            "-h" | "--help" => return Ok(Asked::Help),
            "-V" | "--version" if args.len() == 1 => return Ok(Asked::Version),
            "--dry-run" => dry_run = true,
            f if f.starts_with('-') => return Err(format!("no such option: {f}")),
            f => {
                return Err(format!(
                    "apply-patch takes no file argument ({f}); name each file in the patch on stdin, under '{ADD}', '{UPDATE}' or '{DELETE}'"
                ))
            }
        }
    }
    Ok(Asked::Apply { dry_run })
}

/// What a patch asks of one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Create it with this content.
    Add(Vec<u8>),
    /// Apply these blocks to it.
    Update(Vec<Block>),
    Delete,
}

impl Kind {
    pub fn verb(&self) -> &'static str {
        match self {
            Kind::Add(_) => "add",
            Kind::Update(_) => "update",
            Kind::Delete => "delete",
        }
    }
}

/// One file's section of the patch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    /// The line of stdin its header is on.
    pub line: usize,
    pub path: String,
    pub kind: Kind,
}

/// A line's content: without its newline, a carriage return before it,
/// and trailing spaces or tabs.
fn content(line: &[u8]) -> &[u8] {
    let l = line.strip_suffix(b"\n").unwrap_or(line);
    let l = l.strip_suffix(b"\r").unwrap_or(l);
    let end = l
        .iter()
        .rposition(|b| *b != b' ' && *b != b'\t')
        .map_or(0, |e| e + 1);
    &l[..end]
}

fn blank(line: &[u8]) -> bool {
    line.iter().all(u8::is_ascii_whitespace)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HeaderKind {
    Add,
    Update,
    Delete,
}

enum Line<'a> {
    Begin,
    End,
    Header(HeaderKind, &'a [u8]),
    Move,
    Text,
}

fn classify(c: &[u8]) -> Line<'_> {
    if c == BEGIN.as_bytes() {
        return Line::Begin;
    }
    if c == END.as_bytes() {
        return Line::End;
    }
    for (m, k) in [
        (ADD, HeaderKind::Add),
        (UPDATE, HeaderKind::Update),
        (DELETE, HeaderKind::Delete),
    ] {
        if let Some(p) = c.strip_prefix(m.as_bytes()) {
            return Line::Header(k, p);
        }
        // A header with nothing after its colon.
        if c == m.trim_end().as_bytes() {
            return Line::Header(k, b"");
        }
    }
    if c.starts_with(MOVE.trim_end().as_bytes()) {
        return Line::Move;
    }
    Line::Text
}

/// A path as compared for "named twice": its components, `.` dropped.
fn key(path: &str) -> String {
    Path::new(path)
        .components()
        .filter(|c| *c != Component::CurDir)
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

/// The section being read.
struct Open {
    line: usize,
    path: String,
    kind: HeaderKind,
    /// An add's content so far, or an update's raw lines, padded with
    /// one newline per line of stdin before them so apply-edits' block
    /// parser counts lines as stdin does.
    body: Vec<u8>,
}

fn close(o: Open) -> Result<Change, String> {
    let kind = match o.kind {
        HeaderKind::Add => Kind::Add(o.body),
        HeaderKind::Delete => Kind::Delete,
        HeaderKind::Update => {
            if blank(&o.body) {
                return Err(format!(
                    "stdin line {}: '{UPDATE}{}' has no block under it; put one or more blocks under it, each '<<<<<<< SEARCH', the old lines, '=======', the new lines, '>>>>>>> REPLACE'",
                    o.line, o.path
                ));
            }
            Kind::Update(
                apply_edits::parse_blocks(&o.body)
                    .map_err(|e| format!("{e} (in the update of {})", o.path))?,
            )
        }
    };
    Ok(Change {
        line: o.line,
        path: o.path,
        kind,
    })
}

/// The patch on stdin, its changes in order, or what is malformed: the
/// line of stdin and the fix.
pub fn parse_patch(input: &[u8]) -> Result<Vec<Change>, String> {
    enum At {
        Before,
        Between,
        In(Open),
        Ended,
    }
    let mut changes: Vec<Change> = Vec::new();
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut at = At::Before;
    for (i, line) in input.split_inclusive(|&b| b == b'\n').enumerate() {
        let no = i + 1;
        let c = content(line);
        let shown = || String::from_utf8_lossy(c).into_owned();
        at = match (at, classify(c)) {
            (At::Before, Line::Begin) => At::Between,
            (At::Before, _) if blank(c) => At::Before,
            (At::Before, _) => {
                return Err(format!(
                    "stdin line {no}: '{}' before '{BEGIN}'; open the patch with a '{BEGIN}' line",
                    shown()
                ))
            }
            (At::Ended, _) if blank(c) => At::Ended,
            (At::Ended, _) => {
                return Err(format!(
                    "stdin line {no}: '{}' after '{END}'; put every section before the '{END}' line",
                    shown()
                ))
            }
            (At::Between | At::In(_), Line::Begin) => {
                return Err(format!(
                    "stdin line {no}: a second '{BEGIN}'; send one patch, one '{BEGIN}' and one '{END}'"
                ))
            }
            (At::Between | At::In(_), Line::Move) => {
                return Err(format!(
                    "stdin line {no}: '{}': apply-patch does not move files; add the file at its new path with '{ADD}' and delete the old one with '{DELETE}'",
                    shown()
                ))
            }
            (prev, Line::End) => {
                if let At::In(o) = prev {
                    changes.push(close(o)?);
                }
                At::Ended
            }
            (prev, Line::Header(kind, path)) => {
                if let At::In(o) = prev {
                    changes.push(close(o)?);
                }
                let path = String::from_utf8_lossy(path).trim().to_string();
                if path.is_empty() {
                    return Err(format!(
                        "stdin line {no}: '{}' names no file; write the path after the colon and a space",
                        shown()
                    ));
                }
                if let Some(first) = seen.insert(key(&path), no) {
                    return Err(format!(
                        "stdin line {no}: {path} is named again (first on line {first}); name each file once, all of one file's blocks under its one '{UPDATE}'"
                    ));
                }
                let body = match kind {
                    HeaderKind::Update => vec![b'\n'; no],
                    _ => Vec::new(),
                };
                At::In(Open {
                    line: no,
                    path,
                    kind,
                    body,
                })
            }
            (At::Between, _) if blank(c) => At::Between,
            (At::Between, _) => {
                return Err(format!(
                    "stdin line {no}: '{}' outside a file's section; open each section with '{ADD}<path>', '{UPDATE}<path>' or '{DELETE}<path>'",
                    shown()
                ))
            }
            (At::In(mut o), Line::Text) => {
                match o.kind {
                    HeaderKind::Update => o.body.extend_from_slice(line),
                    HeaderKind::Delete if blank(c) => {}
                    HeaderKind::Delete => {
                        return Err(format!(
                            "stdin line {no}: '{}' under '{DELETE}{}'; a delete takes nothing under its header: to change the file instead, use '{UPDATE}{}'",
                            shown(),
                            o.path,
                            o.path
                        ))
                    }
                    HeaderKind::Add => match line.strip_prefix(b"+") {
                        Some(rest) => {
                            o.body.extend_from_slice(rest);
                            if !rest.ends_with(b"\n") {
                                o.body.push(b'\n');
                            }
                        }
                        None if c.is_empty() => {
                            return Err(format!(
                                "stdin line {no}: an empty line under '{ADD}{}'; write an empty line of the new file as '+' alone",
                                o.path
                            ))
                        }
                        None => {
                            return Err(format!(
                                "stdin line {no}: '{}' under '{ADD}{}' does not start with '+'; write each line of the new file as '+' and its text",
                                shown(),
                                o.path
                            ))
                        }
                    },
                }
                At::In(o)
            }
        };
    }
    match at {
        At::Ended if changes.is_empty() => Err(format!(
            "the patch names no file; put one or more sections between '{BEGIN}' and '{END}', each opened by '{ADD}', '{UPDATE}' or '{DELETE}' and a path"
        )),
        At::Ended => Ok(changes),
        At::Before => Err(format!(
            "no patch on stdin; send one, from a '{BEGIN}' line to a '{END}' line"
        )),
        At::Between | At::In(_) => Err(format!(
            "stdin ended before the '{END}' line: the patch may be cut short; end it with a '{END}' line"
        )),
    }
}

/// What the world holds at a change's path, read before any write.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Found {
    Missing,
    Dir,
    File(Vec<u8>),
    Unreadable(String),
}

/// What one change will do, decided against what was found.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Done {
    Added {
        text: Vec<u8>,
    },
    Updated {
        original: Vec<u8>,
        text: Vec<u8>,
        blocks: Vec<Applied>,
    },
    Deleted {
        original: Vec<u8>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Planned {
    pub path: String,
    pub done: Done,
}

/// The directory a path is in, as named to the reader.
fn dir_of(path: &str) -> String {
    Path::new(path)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map_or(".".into(), |p| p.display().to_string())
}

/// What every change does to the files as `found` (one per change, in
/// order), or what each change that cannot be made says, newline
/// included: its verb, file and line, and the fix.
pub fn plan(changes: &[Change], found: &[Found]) -> Result<Vec<Planned>, Vec<String>> {
    let mut planned = Vec::new();
    let mut refused = Vec::new();
    for (ch, f) in changes.iter().zip(found) {
        let p = &ch.path;
        let head = format!("{} {p} (patch line {})", ch.kind.verb(), ch.line);
        let done = match (&ch.kind, f) {
            (_, Found::Unreadable(e)) => Err(format!(
                "{head}: cannot read {p}: {e}; make it readable, or name another path"
            )),
            (Kind::Add(_), Found::File(_)) => Err(format!(
                "{head}: {p} exists already; to change it use '{UPDATE}{p}' with SEARCH/REPLACE blocks, or name a new path"
            )),
            (Kind::Add(_), Found::Dir) | (Kind::Update(_) | Kind::Delete, Found::Dir) => Err(
                format!("{head}: {p} is a directory; name a file"),
            ),
            (Kind::Add(text), Found::Missing) => Ok(Done::Added { text: text.clone() }),
            (Kind::Update(_), Found::Missing) => Err(format!(
                "{head}: no such file: {p}; check the path with peek {}, or create it with '{ADD}{p}'",
                dir_of(p)
            )),
            (Kind::Delete, Found::Missing) => Err(format!(
                "{head}: no such file: {p}; check the path with peek {}, or drop its '{DELETE}' section",
                dir_of(p)
            )),
            (Kind::Delete, Found::File(b)) => Ok(Done::Deleted { original: b.clone() }),
            (Kind::Update(blocks), Found::File(b)) => match apply_edits::apply(b, blocks) {
                apply_edits::Outcome::Applied { text, blocks } => Ok(Done::Updated {
                    original: b.clone(),
                    text,
                    blocks,
                }),
                apply_edits::Outcome::Refused(faults) => {
                    let mut s = format!(
                        "{head}: {} of {} block{} did not match exactly once:",
                        faults.len(),
                        blocks.len(),
                        if blocks.len() == 1 { "" } else { "s" }
                    );
                    for fault in &faults {
                        s.push_str("\n  ");
                        s.push_str(apply_edits::fault_line(p, fault).trim_end());
                    }
                    Err(s)
                }
            },
        };
        match done {
            Ok(done) => planned.push(Planned {
                path: p.clone(),
                done,
            }),
            Err(mut s) => {
                s.push('\n');
                refused.push(s);
            }
        }
    }
    if refused.is_empty() {
        Ok(planned)
    } else {
        Err(refused)
    }
}

/// How many lines `text` has, a last one without its newline counted.
pub fn line_count(text: &[u8]) -> usize {
    let n = text.iter().filter(|&&b| b == b'\n').count();
    n + usize::from(!text.is_empty() && text.last() != Some(&b'\n'))
}

fn lines(n: usize) -> String {
    format!("{n} line{}", if n == 1 { "" } else { "s" })
}

/// What one planned change prints: its header, then its regions.
pub fn render(p: &Planned) -> Vec<u8> {
    match &p.done {
        Done::Added { text } => {
            let mut out = format!("== add {}: {}\n", p.path, lines(line_count(text))).into_bytes();
            for (i, l) in text.split_inclusive(|&b| b == b'\n').enumerate() {
                out.extend_from_slice(format!("{}: ", i + 1).as_bytes());
                out.extend_from_slice(l.strip_suffix(b"\n").unwrap_or(l));
                out.push(b'\n');
            }
            out
        }
        Done::Updated { text, blocks, .. } => {
            let k = blocks.len();
            let mut out = format!(
                "== update {}: {k} block{}\n",
                p.path,
                if k == 1 { "" } else { "s" }
            )
            .into_bytes();
            for a in blocks {
                out.extend_from_slice(&apply_edits::render(text, a));
            }
            out
        }
        Done::Deleted { original } => format!(
            "== delete {}: {} removed\n",
            p.path,
            lines(line_count(original))
        )
        .into_bytes(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn arguments_are_dry_run_only() {
        assert_eq!(parse(&[]), Ok(Asked::Apply { dry_run: false }));
        assert_eq!(
            parse(&s(&["--dry-run"])),
            Ok(Asked::Apply { dry_run: true })
        );
        assert_eq!(parse(&s(&["-V"])), Ok(Asked::Version));
        assert_eq!(parse(&s(&["--help"])), Ok(Asked::Help));
        assert!(parse(&s(&["a.txt"]))
            .unwrap_err()
            .contains("name each file in the patch"));
        assert!(parse(&s(&["-x"])).unwrap_err().contains("no such option"));
    }

    #[test]
    fn a_patch_parses_into_its_changes_in_order() {
        let c = parse_patch(
            b"\n*** Begin Patch\n*** Add File: d/new.txt\n+one\n+\n+two\n*** Update File: old.txt\n<<<<<<< SEARCH\na\n=======\nb\n>>>>>>> REPLACE\n\n*** Delete File: gone.txt\n\n*** End Patch\n",
        )
        .unwrap();
        assert_eq!(c.len(), 3);
        assert_eq!(c[0].path, "d/new.txt");
        assert_eq!(c[0].kind, Kind::Add(b"one\n\ntwo\n".to_vec()));
        assert_eq!(c[1].line, 7);
        let Kind::Update(b) = &c[1].kind else {
            panic!()
        };
        assert_eq!(
            (b[0].line, &b[0].search[..], &b[0].replace[..]),
            (8, &b"a\n"[..], &b"b\n"[..])
        );
        assert_eq!(
            (c[2].path.as_str(), &c[2].kind),
            ("gone.txt", &Kind::Delete)
        );
    }

    #[test]
    fn an_add_with_no_lines_is_an_empty_file() {
        let c = parse_patch(b"*** Begin Patch\n*** Add File: e\n*** End Patch").unwrap();
        assert_eq!(c[0].kind, Kind::Add(Vec::new()));
    }

    #[test]
    fn a_malformed_patch_names_its_line_and_fix() {
        let e = |i: &str| parse_patch(i.as_bytes()).unwrap_err();
        assert!(e("").contains("no patch on stdin"));
        assert!(e("hi\n").contains("stdin line 1: 'hi' before '*** Begin Patch'"));
        assert!(e("*** Begin Patch\n*** Add File: a\n+x\n").contains("cut short"));
        assert!(e("*** Begin Patch\n*** End Patch\n").contains("names no file"));
        assert!(e("*** Begin Patch\nx\n*** End Patch\n").contains("outside a file's section"));
        assert!(e("*** Begin Patch\n*** Add File: a\nx\n*** End Patch\n")
            .contains("stdin line 3: 'x' under '*** Add File: a' does not start with '+'"));
        assert!(e("*** Begin Patch\n*** Add File: a\n\n*** End Patch\n").contains("'+' alone"));
        assert!(e("*** Begin Patch\n*** Delete File: a\nx\n*** End Patch\n")
            .contains("a delete takes nothing"));
        assert!(e("*** Begin Patch\n*** Update File: a\n*** End Patch\n")
            .contains("stdin line 2: '*** Update File: a' has no block"));
        assert!(
            e("*** Begin Patch\n*** Update File: a\n<<<<<<< SEARCH\nx\n*** End Patch\n")
                .contains("(opened on line 3)")
        );
        assert!(
            e("*** Begin Patch\n*** Add File: a\n*** Delete File: ./a\n*** End Patch\n")
                .contains("stdin line 3: ./a is named again (first on line 2)")
        );
        assert!(
            e("*** Begin Patch\n*** Update File: a\n*** Move to: b\n*** End Patch\n")
                .contains("does not move files")
        );
        assert!(e("*** Begin Patch\n*** Add File:\n*** End Patch\n").contains("names no file"));
        assert!(e("*** Begin Patch\n*** Add File: a\n*** End Patch\nx\n")
            .contains("after '*** End Patch'"));
    }

    fn change(path: &str, kind: Kind) -> Change {
        Change {
            line: 2,
            path: path.into(),
            kind,
        }
    }

    fn blocks(search: &str, replace: &str) -> Kind {
        Kind::Update(vec![Block {
            n: 1,
            line: 3,
            all: false,
            search: search.into(),
            replace: replace.into(),
        }])
    }

    #[test]
    fn every_change_plans_against_what_was_found() {
        let p = plan(
            &[
                change("new", Kind::Add(b"x\n".to_vec())),
                change("old", blocks("a\n", "b\n")),
                change("gone", Kind::Delete),
            ],
            &[
                Found::Missing,
                Found::File(b"a\n".to_vec()),
                Found::File(b"1\n2\n".to_vec()),
            ],
        )
        .unwrap();
        assert_eq!(render(&p[0]), b"== add new: 1 line\n1: x\n");
        assert_eq!(
            render(&p[1]),
            b"== update old: 1 block\n== block 1: 1 match, line 1\n1: b\n"
        );
        assert_eq!(render(&p[2]), b"== delete gone: 2 lines removed\n");
    }

    #[test]
    fn every_change_that_cannot_be_made_is_refused_with_its_fix() {
        let r = plan(
            &[
                change("new", Kind::Add(b"x\n".to_vec())),
                change("d/old", blocks("zzz\n", "b\n")),
                change("d/gone", Kind::Delete),
                change("ok", Kind::Delete),
                change("twice", blocks("a\n", "b\n")),
            ],
            &[
                Found::File(b"".to_vec()),
                Found::File(b"a\n".to_vec()),
                Found::Missing,
                Found::File(b"".to_vec()),
                Found::File(b"a\na\n".to_vec()),
            ],
        )
        .unwrap_err();
        assert_eq!(r.len(), 4);
        assert!(r[0].starts_with(
            "add new (patch line 2): new exists already; to change it use '*** Update File: new'"
        ));
        assert!(r[1].starts_with("update d/old (patch line 2): 1 of 1 block did not match exactly once:\n  block 1 ('zzz'): not found, 0 matches in d/old;"));
        assert!(r[2].contains(
            "delete d/gone (patch line 2): no such file: d/gone; check the path with peek d,"
        ));
        assert!(r[3].contains("2 matches in twice, at lines 1, 2"));
    }

    #[test]
    fn help_names_its_exit_codes() {
        assert!(help().contains("Exits 1 when any change"));
        assert!(help().contains("Exits 2 on a malformed"));
    }
}
