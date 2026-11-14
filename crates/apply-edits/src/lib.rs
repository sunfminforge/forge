//! apply-edits: what it decides, without touching the world. The
//! arguments it takes, the SEARCH/REPLACE blocks read from stdin, how
//! they apply to a file's bytes (every block or none), what a refused
//! block says with its fix, and the changed regions as printed. `world`
//! reads the file and stdin, writes the file once, and prints.

pub mod world;

pub const NAME: &str = "apply-edits";

/// Exit code when any block matched zero or several times.
pub const EXIT_REFUSED: u8 = 1;
/// Exit code of a malformed block, an unreadable or unwritable file, or
/// a usage error.
pub const EXIT_FAILED: u8 = 2;

pub const USAGE: &str = "usage: apply-edits <file> [--dry-run] < blocks";

pub const OPEN: &str = "<<<<<<< SEARCH";
pub const OPEN_ALL: &str = "<<<<<<< SEARCH all";
pub const SPLIT: &str = "=======";
pub const CLOSE: &str = ">>>>>>> REPLACE";

/// What `--help` answers, before its exit codes are put in by `help`.
const HELP: &str = concat!(
    "usage: apply-edits <file> [--dry-run] < blocks\n",
    "       apply-edits --version\n\n",
    "Edits one file in one call, instead of a python3 heredoc of chained str.replace. Standard input holds one or more blocks, each:\n\n",
    "  <<<<<<< SEARCH\n",
    "  the exact old text, one or more lines\n",
    "  =======\n",
    "  the new text, zero or more lines\n",
    "  >>>>>>> REPLACE\n\n",
    "Example:\n",
    "  printf '<<<<<<< SEARCH\\nb = 2\\n=======\\nb = 3\\n>>>>>>> REPLACE\\n' | apply-edits config.toml\n\n",
    "How blocks apply:\n",
    "  matching       the search text is the lines between the SEARCH and ======= lines, each with its newline, matched byte for byte: whitespace, indentation and newlines included, no regex, no fuzz. A file whose last line has no newline matches as if it had one, and still has none after.\n",
    "  once           a block's search text must match exactly once in the file. To replace every match instead, open the block with '<<<<<<< SEARCH all'.\n",
    "  in order       blocks apply one after another, each to the file as the blocks before it left it.\n",
    "  deletion       an empty REPLACE (======= followed at once by >>>>>>> REPLACE) deletes the search text.\n",
    "  every or none  when any block matches zero or several times, nothing is written: the file stays byte for byte as it was.\n",
    "  marker lines   are exactly as shown, alone on their line; blank lines may sit between blocks, any other text outside a block is refused. A search text cannot hold a line that is exactly '=======', nor a replace text one that is exactly '>>>>>>> REPLACE'.\n\n",
    "What it prints, on success, to stdout: per block a header '== block <n>: <m> match(es), line(s) <where>', then the changed region of the file as now written, each line as '<line>: <text>', separate matches of one block parted by '--'. A deletion prints the lines either side of where the text was, under 'deleted, joined at line <l>'. The file is written once, after every block applied; when nothing changed it is not written.\n\n",
    "Reading the regions first is read-files' (<path>:<start>-<end>) and peek's, not apply-edits'.\n\n",
    "options:\n",
    "  --dry-run       apply in memory and print the regions as they would be, but write nothing\n",
    "  --              end of options: what follows is the file, even when it starts with -\n",
    "  -h, --help      print this help\n",
    "  -V, --version   print apply-edits' version\n\n",
    "Exits 0 when every block applied (and the file was written, unless --dry-run). Exits {EXIT_REFUSED} when any block matched zero or several times: stderr names every such block by its number and first line, with its match count, where its matches or its first line are, and the fix; nothing is written. Exits {EXIT_FAILED} on a malformed block (stderr names its line on stdin and the fix), a file it cannot read or write, standard input left a terminal, or a usage error; nothing is written.\n",
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
    Apply { file: String, dry_run: bool },
}

/// The arguments, after the program's name: one file and `--dry-run`, in
/// either order; after `--`, the next argument is the file.
pub fn parse(args: &[String]) -> Result<Asked, String> {
    let mut file: Option<String> = None;
    let mut dry_run = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-h" | "--help" => return Ok(Asked::Help),
            "-V" | "--version" if args.len() == 1 => return Ok(Asked::Version),
            "--dry-run" => dry_run = true,
            "--" => {
                let rest: Vec<&String> = it.by_ref().collect();
                for f in rest {
                    if file.replace(f.clone()).is_some() {
                        return Err("name one file only; run apply-edits once per file".into());
                    }
                }
            }
            f if f.starts_with('-') && f.len() > 1 => {
                return Err(format!(
                    "no such option: {f}; to edit a file named {f}, put -- before it"
                ))
            }
            f => {
                if file.replace(f.to_string()).is_some() {
                    return Err("name one file only; run apply-edits once per file".into());
                }
            }
        }
    }
    match file {
        Some(file) => Ok(Asked::Apply { file, dry_run }),
        None => Err("name the file to edit, and send its blocks on stdin".into()),
    }
}

/// One SEARCH/REPLACE block as read from stdin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    /// Its number, counted from 1 in the order given.
    pub n: usize,
    /// The line of stdin its SEARCH line is on.
    pub line: usize,
    /// Whether it replaces every match, not exactly one.
    pub all: bool,
    pub search: Vec<u8>,
    pub replace: Vec<u8>,
}

/// A line's content: without its newline and a carriage return before it.
fn content(line: &[u8]) -> &[u8] {
    let l = line.strip_suffix(b"\n").unwrap_or(line);
    l.strip_suffix(b"\r").unwrap_or(l)
}

fn is_marker(c: &[u8]) -> bool {
    [OPEN, OPEN_ALL, SPLIT, CLOSE]
        .iter()
        .any(|m| c == m.as_bytes())
}

/// The blocks on stdin, or what is malformed: the line of stdin and the
/// fix.
pub fn parse_blocks(input: &[u8]) -> Result<Vec<Block>, String> {
    enum At {
        Outside,
        Search(Block),
        Replace(Block),
    }
    let mut blocks = Vec::new();
    let mut at = At::Outside;
    for (i, line) in input.split_inclusive(|&b| b == b'\n').enumerate() {
        let no = i + 1;
        let c = content(line);
        at = match at {
            At::Outside => {
                if c == OPEN.as_bytes() || c == OPEN_ALL.as_bytes() {
                    At::Search(Block {
                        n: blocks.len() + 1,
                        line: no,
                        all: c == OPEN_ALL.as_bytes(),
                        search: Vec::new(),
                        replace: Vec::new(),
                    })
                } else if c.iter().all(u8::is_ascii_whitespace) {
                    At::Outside
                } else if is_marker(c) {
                    return Err(format!(
                        "stdin line {no}: '{}' outside a block; open each block with a '{OPEN}' line",
                        String::from_utf8_lossy(c)
                    ));
                } else {
                    return Err(format!(
                        "stdin line {no}: text outside a block; send only blocks, each from a '{OPEN}' line to a '{CLOSE}' line, with nothing but blank lines between them"
                    ));
                }
            }
            At::Search(mut b) => {
                if c == SPLIT.as_bytes() {
                    if b.search.is_empty() {
                        return Err(format!(
                            "stdin line {no}: block {} has an empty search text; put the exact lines to replace between its '{OPEN}' and '{SPLIT}' lines",
                            b.n
                        ));
                    }
                    At::Replace(b)
                } else if is_marker(c) {
                    return Err(format!(
                        "stdin line {no}: '{}' in block {}'s search text, before its '{SPLIT}' line; end the search text with a '{SPLIT}' line",
                        String::from_utf8_lossy(c),
                        b.n
                    ));
                } else {
                    b.search.extend_from_slice(line);
                    At::Search(b)
                }
            }
            At::Replace(mut b) => {
                if c == CLOSE.as_bytes() {
                    blocks.push(b);
                    At::Outside
                } else if c == OPEN.as_bytes() || c == OPEN_ALL.as_bytes() || c == SPLIT.as_bytes()
                {
                    return Err(format!(
                        "stdin line {no}: '{}' in block {}'s replace text; end block {} with a '{CLOSE}' line first",
                        String::from_utf8_lossy(c),
                        b.n,
                        b.n
                    ));
                } else {
                    b.replace.extend_from_slice(line);
                    At::Replace(b)
                }
            }
        };
    }
    match at {
        At::Outside if blocks.is_empty() => Err(format!(
            "no block on stdin; send one or more blocks, each '{OPEN}', the old lines, '{SPLIT}', the new lines, '{CLOSE}'"
        )),
        At::Outside => Ok(blocks),
        At::Search(b) => Err(format!(
            "stdin ended inside block {} (opened on line {}) before its '{SPLIT}' line; end its search text with '{SPLIT}', then the new text and '{CLOSE}'",
            b.n, b.line
        )),
        At::Replace(b) => Err(format!(
            "stdin ended inside block {} (opened on line {}) before its '{CLOSE}' line; end it with a '{CLOSE}' line",
            b.n, b.line
        )),
    }
}

/// Where each non-overlapping match of `needle` starts in `hay`, left to
/// right: memchr's memmem, byte for byte. An empty needle matches nowhere.
pub fn find_all(hay: &[u8], needle: &[u8]) -> Vec<usize> {
    if needle.is_empty() {
        return Vec::new();
    }
    memchr::memmem::find_iter(hay, needle).collect()
}

/// The line, counted from 1, that byte `pos` of `text` is on.
pub fn line_of(text: &[u8], pos: usize) -> usize {
    1 + text[..pos.min(text.len())]
        .iter()
        .filter(|&&b| b == b'\n')
        .count()
}

/// A block that applied: its number, how many matches it replaced, and
/// the byte ranges of its new text in the file as finally written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Applied {
    pub n: usize,
    pub ranges: Vec<(usize, usize)>,
}

/// A block refused: its number, its first line, how many times it
/// matched, and the lines of its matches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fault {
    pub n: usize,
    pub first: String,
    pub count: usize,
    pub lines: Vec<usize>,
    /// When it matched nowhere: the lines its first non-blank line is on,
    /// leading and trailing whitespace ignored.
    pub near: Vec<usize>,
    /// When it matched nowhere: whether the file's lines end in \r\n.
    pub crlf: bool,
}

/// What the blocks do to a file: its new bytes and every block's
/// regions, or every refused block.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Applied { text: Vec<u8>, blocks: Vec<Applied> },
    Refused(Vec<Fault>),
}

/// Moves range `r` for an edit at `p` of `old` bytes replaced by `new`.
fn shift(r: (usize, usize), p: usize, old: usize, new: usize) -> (usize, usize) {
    let (a, b) = r;
    let end = p + old;
    if b <= p {
        return (a, b);
    }
    if a >= end {
        return (a - old + new, b - old + new);
    }
    let na = a.min(p);
    let nb = if b > end { b - old + new } else { p + new };
    (na, nb.max(na))
}

fn first_line(search: &[u8]) -> String {
    let l = search
        .split(|&b| b == b'\n')
        .find(|l| !l.iter().all(u8::is_ascii_whitespace))
        .unwrap_or(b"");
    String::from_utf8_lossy(content(l)).into_owned()
}

fn trim(l: &[u8]) -> &[u8] {
    let s = l
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(l.len());
    let e = l
        .iter()
        .rposition(|b| !b.is_ascii_whitespace())
        .map_or(s, |e| e + 1);
    &l[s..e.max(s)]
}

/// The lines of `text` whose trimmed content is `want`'s trimmed.
fn lines_like(text: &[u8], want: &str) -> Vec<usize> {
    let w = trim(want.as_bytes());
    if w.is_empty() {
        return Vec::new();
    }
    text.split(|&b| b == b'\n')
        .enumerate()
        .filter(|(_, l)| trim(l) == w)
        .map(|(i, _)| i + 1)
        .take(5)
        .collect()
}

/// Applies every block in order to `original`, or refuses: every block
/// that matched zero times, or several times without `all`.
pub fn apply(original: &[u8], blocks: &[Block]) -> Outcome {
    // A last line without a newline matches as if it had one.
    let unterminated = !original.is_empty() && original.last() != Some(&b'\n');
    let mut text = original.to_vec();
    if unterminated {
        text.push(b'\n');
    }
    let mut done: Vec<Applied> = Vec::new();
    let mut faults = Vec::new();
    for b in blocks {
        let at = find_all(&text, &b.search);
        if at.is_empty() || (at.len() > 1 && !b.all) {
            let first = first_line(&b.search);
            faults.push(Fault {
                n: b.n,
                count: at.len(),
                lines: at.iter().map(|&p| line_of(&text, p)).collect(),
                near: if at.is_empty() {
                    lines_like(&text, &first)
                } else {
                    Vec::new()
                },
                crlf: at.is_empty() && text.windows(2).any(|w| w == b"\r\n"),
                first,
            });
            continue;
        }
        let (old, new) = (b.search.len(), b.replace.len());
        let mut mine = Vec::new();
        // From the last match back, so earlier match positions hold.
        for &p in at.iter().rev() {
            text.splice(p..p + old, b.replace.iter().copied());
            for a in done.iter_mut() {
                for r in a.ranges.iter_mut() {
                    *r = shift(*r, p, old, new);
                }
            }
            for r in mine.iter_mut() {
                *r = shift(*r, p, old, new);
            }
            mine.push((p, p + new));
        }
        mine.reverse();
        done.push(Applied {
            n: b.n,
            ranges: mine,
        });
    }
    if !faults.is_empty() {
        return Outcome::Refused(faults);
    }
    if unterminated && text.last() == Some(&b'\n') {
        text.pop();
    }
    let len = text.len();
    for a in done.iter_mut() {
        for r in a.ranges.iter_mut() {
            *r = (r.0.min(len), r.1.min(len));
        }
    }
    Outcome::Applied { text, blocks: done }
}

/// What a refused block says, newline included: which block, its count,
/// where, and the fix.
pub fn fault_line(file: &str, f: &Fault) -> String {
    let lines = |v: &[usize]| {
        v.iter()
            .map(usize::to_string)
            .collect::<Vec<_>>()
            .join(", ")
    };
    if f.count == 0 {
        let mut fix = if f.near.is_empty() {
            format!("its first line is nowhere in {file}; read the region with read-files {file} or peek {file} and copy the old text exactly")
        } else {
            let l = f.near[0];
            format!(
                "a line like its first is at line {}; read read-files {file}:{}-{} and copy the old text byte for byte, indentation and blank lines included",
                lines(&f.near),
                l,
                l + 10
            )
        };
        if f.crlf {
            fix.push_str(&format!(
                "; {file}'s lines end in \\r\\n, so the search text's must too"
            ));
        }
        format!(
            "block {} ('{}'): not found, 0 matches in {file}; {fix}\n",
            f.n, f.first
        )
    } else {
        format!(
            "block {} ('{}'): {} matches in {file}, at lines {}; add lines around the search text until it matches once, or open the block with '{OPEN_ALL}' to replace every match\n",
            f.n,
            f.first,
            f.count,
            lines(&f.lines)
        )
    }
}

/// The byte ranges of each line of `text`, without its newline.
fn line_spans(text: &[u8]) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();
    let mut s = 0;
    for (i, &b) in text.iter().enumerate() {
        if b == b'\n' {
            spans.push((s, i));
            s = i + 1;
        }
    }
    if s < text.len() {
        spans.push((s, text.len()));
    }
    spans
}

/// The first and last line a range covers; a deletion (empty range)
/// covers the lines either side of where it was.
fn covered(text: &[u8], r: (usize, usize), nlines: usize) -> (usize, usize) {
    if r.1 > r.0 {
        return (line_of(text, r.0), line_of(text, r.1 - 1));
    }
    let l = line_of(text, r.0).min(nlines.max(1));
    if r.0 > 0 && text.get(r.0 - 1) != Some(&b'\n') {
        // Joined within a line: that line alone.
        return (l, l);
    }
    (l.saturating_sub(1).max(1), l)
}

/// What one applied block prints: its header, then its regions'
/// numbered lines, matches parted by '--'.
pub fn render(text: &[u8], a: &Applied) -> Vec<u8> {
    let spans = line_spans(text);
    let nlines = spans.len();
    let deleted = a.ranges.iter().all(|r| r.0 == r.1);
    // Line groups, merged where they touch or overlap.
    let mut groups: Vec<(usize, usize)> = Vec::new();
    for &r in &a.ranges {
        let (s, e) = covered(text, r, nlines);
        match groups.last_mut() {
            Some(g) if s <= g.1 + 1 => g.1 = g.1.max(e),
            _ => groups.push((s, e)),
        }
    }
    let m = a.ranges.len();
    let where_: Vec<String> = groups
        .iter()
        .map(|&(s, e)| {
            if s == e {
                s.to_string()
            } else {
                format!("{s}-{e}")
            }
        })
        .collect();
    let mut out = if deleted {
        let at: Vec<String> = a
            .ranges
            .iter()
            .map(|r| line_of(text, r.0).to_string())
            .collect();
        format!(
            "== block {}: {m} match{}, deleted, joined at line{} {}\n",
            a.n,
            if m == 1 { "" } else { "es" },
            if m == 1 { "" } else { "s" },
            at.join(", ")
        )
    } else {
        format!(
            "== block {}: {m} match{}, line{} {}\n",
            a.n,
            if m == 1 { "" } else { "es" },
            if groups.len() == 1 && groups[0].0 == groups[0].1 {
                ""
            } else {
                "s"
            },
            where_.join(", ")
        )
    }
    .into_bytes();
    if nlines == 0 {
        out.extend_from_slice(b"(file now empty)\n");
        return out;
    }
    for (i, &(s, e)) in groups.iter().enumerate() {
        if i > 0 {
            out.extend_from_slice(b"--\n");
        }
        for l in s..=e.min(nlines) {
            let (a, b) = spans[l - 1];
            out.extend_from_slice(format!("{l}: ").as_bytes());
            out.extend_from_slice(&text[a..b]);
            out.push(b'\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    fn block(n: usize, all: bool, search: &str, replace: &str) -> Block {
        Block {
            n,
            line: 1,
            all,
            search: search.into(),
            replace: replace.into(),
        }
    }

    #[test]
    fn arguments_are_one_file_and_dry_run() {
        let ap = |f: &str, d| {
            Ok(Asked::Apply {
                file: f.into(),
                dry_run: d,
            })
        };
        assert_eq!(parse(&s(&["a"])), ap("a", false));
        assert_eq!(parse(&s(&["--dry-run", "a"])), ap("a", true));
        assert_eq!(parse(&s(&["a", "--dry-run"])), ap("a", true));
        assert_eq!(parse(&s(&["--", "-a"])), ap("-a", false));
        assert_eq!(parse(&s(&["-V"])), Ok(Asked::Version));
        assert_eq!(parse(&s(&["a", "--help"])), Ok(Asked::Help));
        assert!(parse(&[]).is_err());
        assert!(parse(&s(&["a", "b"])).unwrap_err().contains("one file"));
        assert!(parse(&s(&["-x"])).unwrap_err().contains("put -- before it"));
    }

    #[test]
    fn blocks_parse_with_their_text_and_mode() {
        let b = parse_blocks(
            b"<<<<<<< SEARCH\na\nb\n=======\nc\n>>>>>>> REPLACE\n\n<<<<<<< SEARCH all\nx\n=======\n>>>>>>> REPLACE",
        )
        .unwrap();
        assert_eq!(b.len(), 2);
        assert_eq!(b[0].search, b"a\nb\n");
        assert_eq!(b[0].replace, b"c\n");
        assert!(!b[0].all);
        assert!(b[1].all);
        assert_eq!(b[1].line, 8);
        assert_eq!(b[1].replace, b"");
    }

    #[test]
    fn malformed_blocks_name_their_line_and_fix() {
        let e = |i: &str| parse_blocks(i.as_bytes()).unwrap_err();
        assert!(e("").contains("no block"));
        assert!(e("hello\n").contains("stdin line 1: text outside"));
        assert!(e("=======\n").contains("outside a block"));
        assert!(e("<<<<<<< SEARCH\na\n").contains("before its '======='"));
        assert!(e("<<<<<<< SEARCH\na\n=======\nb\n").contains("before its '>>>>>>> REPLACE'"));
        assert!(e("<<<<<<< SEARCH\n=======\nb\n>>>>>>> REPLACE\n").contains("empty search"));
        assert!(e("<<<<<<< SEARCH\na\n>>>>>>> REPLACE\n").contains("stdin line 3"));
        assert!(e("<<<<<<< SEARCH\na\n=======\n=======\n").contains("replace text"));
    }

    #[test]
    fn blocks_apply_in_order_with_final_ranges() {
        let text = b"a\nb\nc\nd\n";
        let o = apply(
            text,
            &[
                block(1, false, "c\n", "C1\nC2\n"),
                block(2, false, "a\n", "A\nA\nA\n"),
            ],
        );
        let Outcome::Applied { text, blocks } = o else {
            panic!()
        };
        assert_eq!(text, b"A\nA\nA\nb\nC1\nC2\nd\n");
        assert_eq!(
            render(&text, &blocks[0]),
            b"== block 1: 1 match, lines 5-6\n5: C1\n6: C2\n"
        );
        assert_eq!(
            render(&text, &blocks[1]),
            b"== block 2: 1 match, lines 1-3\n1: A\n2: A\n3: A\n"
        );
    }

    #[test]
    fn zero_or_several_matches_refuse_every_block() {
        let o = apply(
            b"x\ny\nx\n",
            &[
                block(1, false, "x\n", "z\n"),
                block(2, false, " y \n", "q\n"),
                block(3, false, "y\n", "w\n"),
            ],
        );
        let Outcome::Refused(f) = o else { panic!() };
        assert_eq!(f.len(), 2);
        assert_eq!((f[0].n, f[0].count, f[0].lines.clone()), (1, 2, vec![1, 3]));
        assert_eq!((f[1].n, f[1].count, f[1].near.clone()), (2, 0, vec![2]));
        assert!(fault_line("f", &f[0]).contains("2 matches in f, at lines 1, 3"));
        assert!(fault_line("f", &f[1]).contains("not found, 0 matches"));
        assert!(fault_line("f", &f[1]).contains("read-files f:2-12"));
    }

    #[test]
    fn all_replaces_every_match_and_deletion_joins() {
        let Outcome::Applied { text, blocks } =
            apply(b"x\ny\nx\n", &[block(1, true, "x\n", "z\n")])
        else {
            panic!()
        };
        assert_eq!(text, b"z\ny\nz\n");
        assert_eq!(
            render(&text, &blocks[0]),
            b"== block 1: 2 matches, lines 1, 3\n1: z\n--\n3: z\n"
        );
        let Outcome::Applied { text, blocks } = apply(b"a\nb\nc\n", &[block(1, false, "b\n", "")])
        else {
            panic!()
        };
        assert_eq!(text, b"a\nc\n");
        assert_eq!(
            render(&text, &blocks[0]),
            b"== block 1: 1 match, deleted, joined at line 2\n1: a\n2: c\n"
        );
    }

    #[test]
    fn an_unterminated_last_line_matches_and_stays_so() {
        let Outcome::Applied { text, blocks } =
            apply(b"a = 1\nb = 2", &[block(1, false, "b = 2\n", "b = 3\n")])
        else {
            panic!()
        };
        assert_eq!(text, b"a = 1\nb = 3");
        assert_eq!(
            render(&text, &blocks[0]),
            b"== block 1: 1 match, line 2\n2: b = 3\n"
        );
    }

    #[test]
    fn a_later_block_inside_an_earlier_region_keeps_it_whole() {
        let Outcome::Applied { text, blocks } = apply(
            b"a\nb\nc\nd\n",
            &[
                block(1, false, "b\n", "B1\nB2\n"),
                block(2, false, "B2\nc\n", "X\n"),
            ],
        ) else {
            panic!()
        };
        assert_eq!(text, b"a\nB1\nX\nd\n");
        assert_eq!(blocks[0].ranges, vec![(2, 7)]);
        assert_eq!(blocks[1].ranges, vec![(5, 7)]);
    }

    #[test]
    fn matches_are_non_overlapping_and_an_empty_search_finds_none() {
        assert_eq!(find_all(b"aaaaa", b"aa"), vec![0, 2]);
        assert_eq!(find_all(b"x\ny\nx\n", b"x\n"), vec![0, 4]);
        assert_eq!(find_all(b"abc", b""), Vec::<usize>::new());
        assert_eq!(find_all(b"ab", b"abc"), Vec::<usize>::new());
    }

    #[test]
    fn help_names_its_exit_codes() {
        assert!(help().contains("Exits 1 when any block"));
        assert!(help().contains("Exits 2 on a malformed"));
    }
}
