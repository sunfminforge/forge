//! subst: what it decides, without touching the world. The arguments it
//! takes, each substitution (a regex, its replacement and the count it
//! must match), what the substitutions do to each file as found (every
//! file or none), what a refused substitution says with its fix, and what
//! is printed. `world` reads the files, writes them, and prints.

pub mod world;

use regex::bytes::Regex;

pub const NAME: &str = "subst";

/// Exit code when any substitution matched nothing in a file, or not the
/// count it requires.
pub const EXIT_REFUSED: u8 = 1;
/// Exit code of a bad regex or replacement, a file it cannot read or
/// write, or a usage error.
pub const EXIT_FAILED: u8 = 2;

pub const USAGE: &str = "usage: subst [--dry-run] <regex> <replacement> [-n <count>] [--] <file>...\n       subst [--dry-run] -e <regex> <replacement> [-n <count>] [-e ...]... [--] <file>...";

/// What `--help` answers, before its exit codes are put in by `help`.
const HELP: &str = concat!(
    "usage: subst [--dry-run] <regex> <replacement> [-n <count>] [--] <file>...\n",
    "       subst [--dry-run] -e <regex> <replacement> [-n <count>] [-e ...]... [--] <file>...\n",
    "       subst --version\n\n",
    "Applies one or more regex substitutions to several files in one call, instead of sed -i 's/…/…/g' per file or a python3 heredoc that splices out the text between two anchors. Every substitution is checked against every file before any file is written: all files are written, or none.\n\n",
    "Examples:\n",
    "  subst '\\badd\\(' 'add_item(' a.rs b.rs                      rename a call in two files\n",
    "  subst 'fn (\\w+)_old\\(' 'fn ${1}_new(' -n 1 lib.rs           capture group; exactly one match\n",
    "  subst '(?s)// BEGIN gen\\n.*?// END gen\\n' '' -n 1 main.rs    splice out the text between two anchors\n",
    "  subst -e 'foo' 'bar' -e '(?m)^version = \"0\\.1\\.0\"$' 'version = \"0.2.0\"' -n 1 Cargo.toml\n\n",
    "How substitutions apply:\n",
    "  regex          Rust's regex crate syntax, matched against each file's bytes. '.' does not match a newline unless the regex opens with (?s); ^ and $ are the file's start and end unless it opens with (?m). (?s)A.*?B is the shortest text from A to B, across lines. (?i) ignores case. No look-around, no backreferences in the regex.\n",
    "  replacement    the text that replaces each match, with $1, $2 (capture groups by number), ${name} (by name, from (?P<name>…)) and $0 (the whole match). Write ${1}x, not $1x, when a letter, digit or _ follows; $$ is a literal $. Nothing else is special: no \\n escapes, so send a newline as one (bash $'…'). An empty replacement deletes each match.\n",
    "  every match    a substitution replaces every non-overlapping match in each file, left to right.\n",
    "  count          by default a substitution must match at least once in every file named. -n <count>, after its substitution, requires exactly that many matches in every file.\n",
    "  in order       substitutions apply one after another, each to the text the ones before it left; its matches are counted there.\n",
    "  every or none  when any substitution matches nothing in a file, or not its count, no file is written: every file stays byte for byte as it was.\n\n",
    "What it prints, on success, to stdout: per file a header '== <file>' (' (unchanged)' added when the replacements left it as it was), then per substitution a header '-- subst <n> (<regex>): <m> match(es), line(s) <where>' and the changed region of the file as now written, each line as '<line>: <text>', separate matches parted by '--'. A deletion prints the lines either side of where the text was, under 'deleted, joined at line <l>'. Each changed file is written once, to a tempfile renamed over its path; a failure part-way puts back the files already renamed.\n\n",
    "Exact-text edits, with no regex to escape, are apply-edits' (one file) and apply-patch's (several, adds and deletes too), not subst's. Finding first what a regex matches is grep-around's.\n\n",
    "options:\n",
    "  -e <regex> <replacement>   one substitution; repeat for several, applied in the order given. Without -e, the first two arguments are the one substitution.\n",
    "  -n <count>      after a substitution: it must match exactly <count> times (1 or more) in every file\n",
    "  --dry-run       apply in memory and print the regions as they would be, but write nothing\n",
    "  --              end of options: what follows are files, even when they start with -\n",
    "  -h, --help      print this help\n",
    "  -V, --version   print subst's version\n\n",
    "Exits 0 when every substitution matched as required in every file (and the files were written, unless --dry-run). Exits {EXIT_REFUSED} when any substitution matched nothing in a file, or not its count: stderr names every such substitution by its number and regex, the file, its match count and where its matches are, with the fix; no file is written. Exits {EXIT_FAILED} on a regex that does not compile, a replacement naming a group the regex lacks, a file it cannot read or write, a file named twice, or a usage error, each with its fix on stderr; no file is written.\n",
);

/// What `--help` answers, its exit codes as the constants say.
pub fn help() -> String {
    HELP.replace("{EXIT_REFUSED}", &EXIT_REFUSED.to_string())
        .replace("{EXIT_FAILED}", &EXIT_FAILED.to_string())
}

/// How many times a substitution must match in each file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Count {
    AtLeastOne,
    /// Exactly this many, 1 or more.
    Exactly(usize),
}

/// One substitution as given: its number from 1, its regex and
/// replacement as written, and its count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Given {
    pub n: usize,
    pub regex: String,
    pub replacement: String,
    pub count: Count,
}

/// What the arguments ask.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked {
    Help,
    Version,
    Apply {
        substs: Vec<Given>,
        files: Vec<String>,
        dry_run: bool,
    },
}

fn count_of(v: Option<&String>) -> Result<Count, String> {
    match v.map(|s| s.parse::<usize>()) {
        Some(Ok(0)) => Err(
            "-n 0: a count is 1 or more; to require no match, check first with grep-around".into(),
        ),
        Some(Ok(k)) => Ok(Count::Exactly(k)),
        Some(Err(_)) => Err(format!(
            "-n {}: not a count; give a whole number, 1 or more, e.g. -n 1",
            v.map_or("", |s| s.as_str())
        )),
        None => Err("-n needs a count, e.g. -n 1".into()),
    }
}

/// The arguments, after the program's name.
pub fn parse(args: &[String]) -> Result<Asked, String> {
    let mut substs: Vec<Given> = Vec::new();
    let mut positional: Vec<String> = Vec::new();
    // A -n given before any -e: the positional substitution's.
    let mut loose_count: Option<Count> = None;
    let mut dry_run = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "-h" | "--help" => return Ok(Asked::Help),
            "-V" | "--version" if args.len() == 1 => return Ok(Asked::Version),
            "--dry-run" => dry_run = true,
            "-e" => {
                let (Some(re), Some(rep)) = (it.next(), it.next()) else {
                    return Err("-e needs a regex and its replacement, e.g. -e 'old_(\\w+)' 'new_$1'".into());
                };
                substs.push(Given {
                    n: substs.len() + 1,
                    regex: re.clone(),
                    replacement: rep.clone(),
                    count: Count::AtLeastOne,
                });
            }
            "-n" => {
                let c = count_of(it.next())?;
                match substs.last_mut() {
                    Some(s) if s.count == Count::AtLeastOne => s.count = c,
                    Some(s) => {
                        return Err(format!(
                            "substitution {} has two counts; give one -n after each -e",
                            s.n
                        ))
                    }
                    None if loose_count.is_none() => loose_count = Some(c),
                    None => return Err("two counts for one substitution; give -n once".into()),
                }
            }
            "--" => positional.extend(it.by_ref().cloned()),
            f if f.starts_with('-') && f.len() > 1 => {
                return Err(format!(
                    "no such option: {f}; a regex or file starting with - goes after --, or as -e '{f}' <replacement>"
                ))
            }
            f => positional.push(f.to_string()),
        }
    }
    if substs.is_empty() {
        if positional.len() < 2 {
            return Err("give a regex and its replacement, then the files".into());
        }
        let rest = positional.split_off(2);
        substs.push(Given {
            n: 1,
            regex: positional[0].clone(),
            replacement: positional[1].clone(),
            count: loose_count.unwrap_or(Count::AtLeastOne),
        });
        positional = rest;
    } else if loose_count.is_some() {
        return Err("-n before the first -e; put each -n after the -e it counts".into());
    }
    if positional.is_empty() {
        return Err("name the files to change after the substitutions".into());
    }
    let mut seen = std::collections::HashSet::new();
    for f in &positional {
        if !seen.insert(f.as_str()) {
            return Err(format!("{f} is named twice; name each file once"));
        }
    }
    Ok(Asked::Apply {
        substs,
        files: positional,
        dry_run,
    })
}

/// A substitution compiled: as given, and its regex.
#[derive(Clone, Debug)]
pub struct Subst {
    pub given: Given,
    pub re: Regex,
}

/// The group names a replacement refers to, as the regex crate reads
/// them: `$$` is a literal, `${name}` a name, `$` then the longest run of
/// letters, digits and `_` a name or number.
fn references(rep: &str) -> Vec<String> {
    let b = rep.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'$' {
            i += 1;
            continue;
        }
        match b.get(i + 1) {
            Some(b'$') => i += 2,
            Some(b'{') => match rep[i + 2..].find('}') {
                Some(e) => {
                    out.push(rep[i + 2..i + 2 + e].to_string());
                    i += e + 3;
                }
                None => i += 1,
            },
            _ => {
                let e = b[i + 1..]
                    .iter()
                    .position(|c| !(c.is_ascii_alphanumeric() || *c == b'_'))
                    .map_or(b.len(), |p| i + 1 + p);
                if e > i + 1 {
                    out.push(rep[i + 1..e].to_string());
                }
                i = e.max(i + 1);
            }
        }
    }
    out
}

/// Every substitution compiled, or every one that cannot be, each with
/// its fix.
pub fn compile(given: &[Given]) -> Result<Vec<Subst>, Vec<String>> {
    let mut out = Vec::new();
    let mut faults = Vec::new();
    for g in given {
        let re = match Regex::new(&g.regex) {
            Ok(re) => re,
            Err(e) => {
                let e = e.to_string().replace('\n', "\n    ");
                faults.push(format!(
                    "subst {} ('{}'): the regex does not compile; fix it as this says, in Rust regex syntax (no look-around or backreferences):\n    {e}\n",
                    g.n, g.regex
                ));
                continue;
            }
        };
        let groups = re.captures_len();
        let names: Vec<&str> = re.capture_names().flatten().collect();
        for r in references(&g.replacement) {
            let known = match r.parse::<usize>() {
                Ok(k) => k < groups,
                Err(_) => names.contains(&r.as_str()),
            };
            if !known {
                let fix = match r.find(|c: char| !c.is_ascii_digit()) {
                    Some(p) if p > 0 => format!(
                        "write ${{{}}}{} to follow group {} with '{}'",
                        &r[..p],
                        &r[p..],
                        &r[..p],
                        &r[p..]
                    ),
                    _ => format!(
                        "the regex has groups $0 to ${}{}; name one of those, or write $$ for a literal $",
                        groups - 1,
                        if names.is_empty() {
                            String::new()
                        } else {
                            format!(" and ${{{}}}", names.join("}, ${"))
                        }
                    ),
                };
                faults.push(format!(
                    "subst {} ('{}'): its replacement names ${{{r}}}, a group the regex does not have; {fix}\n",
                    g.n, g.regex
                ));
            }
        }
        out.push(Subst {
            given: g.clone(),
            re,
        });
    }
    if faults.is_empty() {
        Ok(out)
    } else {
        Err(faults)
    }
}

/// A substitution that applied to a file: its number and the byte ranges
/// of its new text in the file as finally written.
pub type Applied = apply_edits::Applied;

/// A substitution refused in one file: its number and regex, the file,
/// how many times it matched, the count required, and its matches' lines.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fault {
    pub n: usize,
    pub regex: String,
    pub file: String,
    pub count: usize,
    pub required: Count,
    pub lines: Vec<usize>,
}

/// What the substitutions do to one file: its new bytes and each
/// substitution's regions, or every refused substitution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Applied { text: Vec<u8>, substs: Vec<Applied> },
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

/// Applies every substitution in order to `original`, named `file`, or
/// refuses: every substitution whose match count is not as required.
pub fn apply(file: &str, original: &[u8], substs: &[Subst]) -> Outcome {
    let mut text = original.to_vec();
    let mut done: Vec<Applied> = Vec::new();
    let mut faults = Vec::new();
    for s in substs {
        // Each match: where it is and what replaces it.
        let mut edits: Vec<(usize, usize, Vec<u8>)> = Vec::new();
        for caps in s.re.captures_iter(&text) {
            let m = caps.get(0).expect("group 0 is the match");
            let mut rep = Vec::new();
            caps.expand(s.given.replacement.as_bytes(), &mut rep);
            edits.push((m.start(), m.end(), rep));
        }
        let ok = match s.given.count {
            Count::AtLeastOne => !edits.is_empty(),
            Count::Exactly(k) => edits.len() == k,
        };
        if !ok {
            let mut lines: Vec<usize> = edits
                .iter()
                .map(|e| apply_edits::line_of(&text, e.0))
                .collect();
            lines.dedup();
            faults.push(Fault {
                n: s.given.n,
                regex: s.given.regex.clone(),
                file: file.to_string(),
                count: edits.len(),
                required: s.given.count,
                lines,
            });
            continue;
        }
        let mut mine = Vec::new();
        // From the last match back, so earlier match positions hold.
        for (p, e, rep) in edits.into_iter().rev() {
            let (old, new) = (e - p, rep.len());
            text.splice(p..e, rep);
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
            n: s.given.n,
            ranges: mine,
        });
    }
    if !faults.is_empty() {
        return Outcome::Refused(faults);
    }
    Outcome::Applied { text, substs: done }
}

fn plural(k: usize, one: &str, many: &str) -> String {
    format!("{k} {}", if k == 1 { one } else { many })
}

/// What a refused substitution says, newline included: which one, the
/// file, its count against the one required, where, and the fix.
pub fn fault_line(f: &Fault) -> String {
    let lines = f
        .lines
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let before = if f.n > 1 {
        format!(", counted after substs 1 to {} applied", f.n - 1)
    } else {
        String::new()
    };
    let (file, re) = (&f.file, &f.regex);
    match f.required {
        Count::AtLeastOne => format!(
            "subst {} ('{re}'): 0 matches in {file}{before}, at least 1 required; drop {file} from the call, or see what the regex finds with grep-around '{re}' {file} and fix the regex\n",
            f.n
        ),
        Count::Exactly(k) => format!(
            "subst {} ('{re}'): {} in {file}{}{before}, {k} required by -n {k}; {}\n",
            f.n,
            plural(f.count, "match", "matches"),
            if lines.is_empty() {
                String::new()
            } else {
                format!(" (line{} {lines})", if f.lines.len() == 1 { "" } else { "s" })
            },
            if f.count == 0 {
                format!("see what the regex finds with grep-around '{re}' {file} and fix the regex, or drop {file} from the call")
            } else if f.count > k {
                format!("narrow the regex until it matches {k} times, or give -n {} if every match should change", f.count)
            } else {
                format!("widen the regex until it matches {k} times, or give -n {} if those are all", f.count)
            }
        ),
    }
}

/// What one file prints: its header, then each substitution's header and
/// numbered regions.
pub fn render(
    file: &str,
    original: &[u8],
    text: &[u8],
    substs: &[Subst],
    done: &[Applied],
) -> Vec<u8> {
    let mut out = format!(
        "== {file}{}\n",
        if text == original { " (unchanged)" } else { "" }
    )
    .into_bytes();
    for (s, a) in substs.iter().zip(done) {
        let r = apply_edits::render(text, a);
        // apply-edits heads its region '== block <n>: <what>'; subst
        // heads it '-- subst <n> (<regex>): <what>'.
        let nl = r.iter().position(|&b| b == b'\n').unwrap_or(r.len());
        let head = String::from_utf8_lossy(&r[..nl]);
        let what = head.split_once(": ").map_or("", |(_, w)| w);
        out.extend_from_slice(
            format!("-- subst {} ({}): {what}", s.given.n, s.given.regex).as_bytes(),
        );
        out.extend_from_slice(&r[nl..]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    fn given(n: usize, re: &str, rep: &str, count: Count) -> Given {
        Given {
            n,
            regex: re.into(),
            replacement: rep.into(),
            count,
        }
    }

    #[test]
    fn arguments_take_one_positional_substitution_or_several_e() {
        let Ok(Asked::Apply {
            substs,
            files,
            dry_run,
        }) = parse(&s(&["a", "b", "-n", "2", "x", "y", "--dry-run"]))
        else {
            panic!()
        };
        assert_eq!(substs, vec![given(1, "a", "b", Count::Exactly(2))]);
        assert_eq!(files, s(&["x", "y"]));
        assert!(dry_run);
        let Ok(Asked::Apply { substs, files, .. }) =
            parse(&s(&["-e", "a", "b", "-n", "1", "-e", "c", "d", "--", "-f"]))
        else {
            panic!()
        };
        assert_eq!(
            substs,
            vec![
                given(1, "a", "b", Count::Exactly(1)),
                given(2, "c", "d", Count::AtLeastOne)
            ]
        );
        assert_eq!(files, s(&["-f"]));
        assert_eq!(parse(&s(&["-V"])), Ok(Asked::Version));
        assert_eq!(parse(&s(&["a", "--help"])), Ok(Asked::Help));
        assert!(parse(&s(&["a", "b"]))
            .unwrap_err()
            .contains("name the files"));
        assert!(parse(&s(&["a"]))
            .unwrap_err()
            .contains("regex and its replacement"));
        assert!(parse(&s(&["a", "b", "f", "f"]))
            .unwrap_err()
            .contains("named twice"));
        assert!(parse(&s(&["a", "b", "-n", "0", "f"]))
            .unwrap_err()
            .contains("1 or more"));
        assert!(parse(&s(&["-n", "1", "-e", "a", "b", "f"]))
            .unwrap_err()
            .contains("after the -e"));
        assert!(parse(&s(&["-x", "a", "b", "f"]))
            .unwrap_err()
            .contains("after --"));
    }

    #[test]
    fn a_replacement_naming_a_missing_group_is_refused_with_its_fix() {
        let e = compile(&[given(1, "(a)", "$1x", Count::AtLeastOne)]).unwrap_err();
        assert!(e[0].contains("write ${1}x"), "{}", e[0]);
        let e = compile(&[given(1, "(a)", "$2", Count::AtLeastOne)]).unwrap_err();
        assert!(e[0].contains("groups $0 to $1"), "{}", e[0]);
        let e = compile(&[
            given(1, "(", "", Count::AtLeastOne),
            given(2, "a", "$$1 ${0}", Count::AtLeastOne),
        ])
        .unwrap_err();
        assert_eq!(e.len(), 1);
        assert!(e[0].contains("does not compile"));
        assert!(compile(&[given(1, "(?P<w>a)", "${w}$1", Count::AtLeastOne)]).is_ok());
    }

    #[test]
    fn later_substitutions_keep_earlier_regions_in_place() {
        let substs = compile(&[
            given(1, "c", "C1\nC2", Count::AtLeastOne),
            given(2, "a", "A\nA\nA", Count::AtLeastOne),
        ])
        .unwrap();
        let Outcome::Applied { text, substs: done } = apply("f", b"a\nb\nc\nd\n", &substs) else {
            panic!()
        };
        assert_eq!(text, b"A\nA\nA\nb\nC1\nC2\nd\n");
        assert_eq!(
            String::from_utf8(render("f", b"", &text, &substs, &done)).unwrap(),
            "== f\n-- subst 1 (c): 1 match, lines 5-6\n5: C1\n6: C2\n-- subst 2 (a): 1 match, lines 1-3\n1: A\n2: A\n3: A\n"
        );
    }

    #[test]
    fn fault_lines_name_the_count_and_fix() {
        let f = Fault {
            n: 2,
            regex: "x".into(),
            file: "f".into(),
            count: 3,
            required: Count::Exactly(2),
            lines: vec![1, 4],
        };
        assert_eq!(
            fault_line(&f),
            "subst 2 ('x'): 3 matches in f (lines 1, 4), counted after substs 1 to 1 applied, 2 required by -n 2; narrow the regex until it matches 2 times, or give -n 3 if every match should change\n"
        );
    }

    #[test]
    fn help_names_its_exit_codes() {
        assert!(help().contains("Exits 1 when any substitution"));
        assert!(help().contains("Exits 2 on a regex"));
    }
}
