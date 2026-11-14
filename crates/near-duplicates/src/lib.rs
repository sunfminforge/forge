//! near-duplicates: a detector of the trainer crew. Finds entries of the
//! roles' memory (core.md, skills/) that say nearly the same, within a role
//! or across roles.
//!
//! This crate's root decides, from the files [`world`] read; [`lore`] and
//! [`world`] read lore through Forgejo, and print.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::LazyLock;

use regex::Regex;

pub mod lore;
pub mod py;
pub mod world;

pub const NAME: &str = "near-duplicates";

/// Shorter entries are too small to call duplicates.
pub const MIN_WORDS: usize = 8;
/// The share of their words two entries have in common.
pub const JACCARD: f64 = 0.6;
/// Or how much of one reads as the other, word for word.
pub const SEQUENCE: f64 = 0.75;
/// Or how much of the shorter one the longer says again,
pub const CONTAINED: f64 = 0.9;
/// when the shorter has at least this many distinct words.
pub const CONTAINED_WORDS: usize = 10;

const STOP: &str = "
a about above after again all also am an and any are as at be because been before being below
between both but by can could did do does doing done down during each either else even ever every
few for from further had has have having he her here hers him his how i if in into is it its
itself just let may me might more most must my no nor not now of off on once one only or other
our ours out over own per same shall she should so some such than that the their theirs them then
there these they this those through to too under until up upon us very was we were what when
where which while who whom whose why will with within without would yet you your yours
";

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap_or_else(|e| panic!("{pattern}: {e}"))
}

static ITEM: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*(?:[-*+]|\d+[.)])\s+"));
static WORD: LazyLock<Regex> = LazyLock::new(|| re(r"[a-z0-9]+(?:['’][a-z]+)?"));
static SUFFIX: LazyLock<Regex> = LazyLock::new(|| re(r"['’][a-z]+$"));
static STOPS: LazyLock<BTreeSet<&'static str>> =
    LazyLock::new(|| STOP.split_whitespace().collect());

/// `{role: {path within the role: blob}}`: each role's core.md and skills/,
/// the role's memory. A role is a top-level directory of lore holding a
/// role.toml.
pub fn memory<B: Clone>(files: &BTreeMap<String, B>) -> BTreeMap<String, BTreeMap<String, B>> {
    let mut out: BTreeMap<String, BTreeMap<String, B>> = files
        .keys()
        .filter(|p| p.matches('/').count() == 1 && p.ends_with("/role.toml"))
        .map(|p| {
            (
                p.split('/').next().unwrap_or("").to_string(),
                BTreeMap::new(),
            )
        })
        .collect();
    for (path, blob) in files {
        let (role, rest) = path.split_once('/').unwrap_or((path, ""));
        let Some(mine) = out.get_mut(role) else {
            continue;
        };
        if rest == "core.md" || (rest.starts_with("skills/") && rest.ends_with(".md")) {
            mine.insert(rest.to_string(), blob.clone());
        }
    }
    out
}

/// A role's files in the order they are read: core.md, then the rest.
pub fn in_order<B>(files: &BTreeMap<String, B>) -> Vec<&String> {
    let mut paths: Vec<&String> = files.keys().collect();
    paths.sort_by_key(|p| (p.as_str() != "core.md", p.as_str()));
    paths
}

/// (line number, text) of each entry of a markdown file: a list item with
/// its continuation lines, a paragraph, or a fenced block. Headings are no
/// entries.
pub fn entries(text: &str) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    let mut start = 0;
    let mut fenced = false;
    let close = |out: &mut Vec<(usize, String)>, cur: &mut Vec<&str>, start: usize| {
        if !cur.is_empty() {
            out.push((start, cur.join("\n")));
        }
        cur.clear();
    };
    for (i, line) in py::splitlines(text).into_iter().enumerate() {
        let n = i + 1;
        let stripped = py::strip(line);
        if stripped.starts_with("```") || stripped.starts_with("~~~") {
            if fenced {
                cur.push(line);
                close(&mut out, &mut cur, start);
                fenced = false;
            } else {
                close(&mut out, &mut cur, start);
                fenced = true;
                start = n;
                cur = vec![line];
            }
            continue;
        }
        if fenced {
            cur.push(line);
            continue;
        }
        if stripped.is_empty() || stripped.starts_with('#') {
            close(&mut out, &mut cur, start);
            continue;
        }
        if ITEM.is_match(line) || cur.is_empty() {
            close(&mut out, &mut cur, start);
            start = n;
            cur = vec![line];
        } else {
            cur.push(line);
        }
    }
    close(&mut out, &mut cur, start);
    out
}

/// The words of an entry that say what it is: no stop word, each stemmed.
pub fn words(entry: &str) -> Vec<String> {
    let lower = entry.to_lowercase();
    let mut out = Vec::new();
    for m in WORD.find_iter(&lower) {
        let w = SUFFIX.replace(m.as_str(), "").into_owned();
        let n = w.chars().count();
        if STOPS.contains(w.as_str()) || n < 2 {
            continue;
        }
        let w = if n > 4 && w.ends_with("ies") {
            format!("{}y", &w[..w.len() - 3])
        } else if n > 3 && w.ends_with('s') && !w.ends_with("ss") {
            w[..w.len() - 1].to_string()
        } else {
            w
        };
        out.push(w);
    }
    out
}

/// `difflib.SequenceMatcher(None, a, b, autojunk=False).ratio()`: twice the
/// words matched, over the words of both.
pub fn sequence_ratio(a: &[String], b: &[String]) -> f64 {
    let mut b2j: HashMap<&str, Vec<usize>> = HashMap::new();
    for (j, w) in b.iter().enumerate() {
        b2j.entry(w.as_str()).or_default().push(j);
    }
    // The longest match in a[alo..ahi] and b[blo..bhi], the earliest of
    // the longest; Python's find_longest_match with no junk.
    let longest = |alo: usize, ahi: usize, blo: usize, bhi: usize| -> (usize, usize, usize) {
        let (mut besti, mut bestj, mut bestsize) = (alo, blo, 0);
        let mut j2len: HashMap<usize, usize> = HashMap::new();
        for (i, w) in a.iter().enumerate().take(ahi).skip(alo) {
            let mut next: HashMap<usize, usize> = HashMap::new();
            for &j in b2j.get(w.as_str()).map(Vec::as_slice).unwrap_or(&[]) {
                if j < blo {
                    continue;
                }
                if j >= bhi {
                    break;
                }
                let k = if j > 0 {
                    j2len.get(&(j - 1)).copied().unwrap_or(0)
                } else {
                    0
                } + 1;
                next.insert(j, k);
                if k > bestsize {
                    (besti, bestj, bestsize) = (i + 1 - k, j + 1 - k, k);
                }
            }
            j2len = next;
        }
        while besti > alo && bestj > blo && a[besti - 1] == b[bestj - 1] {
            besti -= 1;
            bestj -= 1;
            bestsize += 1;
        }
        while besti + bestsize < ahi
            && bestj + bestsize < bhi
            && a[besti + bestsize] == b[bestj + bestsize]
        {
            bestsize += 1;
        }
        (besti, bestj, bestsize)
    };
    let mut matched = 0;
    let mut queue = vec![(0, a.len(), 0, b.len())];
    while let Some((alo, ahi, blo, bhi)) = queue.pop() {
        let (i, j, k) = longest(alo, ahi, blo, bhi);
        if k > 0 {
            matched += k;
            if alo < i && blo < j {
                queue.push((alo, i, blo, j));
            }
            if i + k < ahi && j + k < bhi {
                queue.push((i + k, ahi, j + k, bhi));
            }
        }
    }
    let length = a.len() + b.len();
    if length == 0 {
        1.0
    } else {
        2.0 * matched as f64 / length as f64
    }
}

/// How alike two entries are, from 0 to 1, or `None` below the bar.
pub fn similar(a: &[String], b: &[String]) -> Option<f64> {
    let sa: BTreeSet<&String> = a.iter().collect();
    let sb: BTreeSet<&String> = b.iter().collect();
    let shared = sa.intersection(&sb).count();
    let jaccard = shared as f64 / sa.union(&sb).count() as f64;
    if jaccard >= JACCARD {
        return Some(jaccard);
    }
    let small = sa.len().min(sb.len());
    let contained = shared as f64 / small as f64;
    if small >= CONTAINED_WORDS && contained >= CONTAINED {
        return Some(contained);
    }
    // Cheap bound first: the sequence ratio is at most 2*min/(len a + len b).
    let bound = (2 * a.len().min(b.len())) as f64 / (a.len() + b.len()) as f64;
    if bound < SEQUENCE || jaccard < SEQUENCE / 2.0 {
        return None;
    }
    let seq = sequence_ratio(a, b);
    (seq >= SEQUENCE).then_some(seq)
}

/// An entry on one line, cut to `n` characters.
pub fn clip(entry: &str, n: usize) -> String {
    let one = py::squeeze(entry);
    if one.chars().count() <= n {
        one
    } else {
        format!("{}…", py::take(&one, n - 1))
    }
}

/// An entry of a role's memory, and its words.
#[derive(Clone, Debug)]
pub struct Item {
    pub role: String,
    pub path: String,
    pub line: usize,
    pub text: String,
    pub words: Vec<String>,
}

/// The entries of a role's file worth comparing: no example block, and
/// words enough to say something.
pub fn items_of(role: &str, path: &str, text: &str) -> Vec<Item> {
    let mut out = Vec::new();
    for (line, entry) in entries(text) {
        let lead = entry.trim_start_matches(py::is_space);
        if lead.starts_with("```") || lead.starts_with("~~~") {
            continue; // an example block, not a way of working
        }
        let words = words(&entry);
        if words.len() >= MIN_WORDS {
            out.push(Item {
                role: role.into(),
                path: path.into(),
                line,
                text: entry,
                words,
            });
        }
    }
    out
}

/// A finding: the role, what was seen, and the evidence for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub role: String,
    pub saw: String,
    pub evidence: String,
}

/// One finding per group of alike entries; `lore` names where they were read.
pub fn findings(items: &[Item], lore: &str) -> Vec<Finding> {
    // Group the pairs that are alike, so an entry said three times is one finding.
    let mut parent: Vec<usize> = (0..items.len()).collect();
    fn find(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    let mut score: BTreeMap<(usize, usize), f64> = BTreeMap::new();
    for i in 0..items.len() {
        for j in i + 1..items.len() {
            if let Some(s) = similar(&items[i].words, &items[j].words) {
                score.insert((i, j), s);
                let (rj, ri) = (find(&mut parent, j), find(&mut parent, i));
                parent[rj] = ri;
            }
        }
    }
    let mut groups: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
    for &(i, j) in score.keys() {
        let root = find(&mut parent, i);
        groups.entry(root).or_default().extend([i, j]);
    }
    let mut out = Vec::new();
    for members in groups.values() {
        let roles: Vec<&str> = members
            .iter()
            .map(|&i| items[i].role.as_str())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let count = |r: &str| members.iter().filter(|&&i| items[i].role == r).count();
        // The most entries, the first name among those alike.
        let role = roles
            .iter()
            .enumerate()
            .max_by_key(|(k, r)| (count(r), std::cmp::Reverse(*k)))
            .map(|(_, r)| *r)
            .unwrap_or("");
        let where_ = if roles.len() == 1 {
            "within the role".to_string()
        } else {
            format!("across the roles {}", roles.join(", "))
        };
        let pairs: Vec<String> = score
            .iter()
            .filter(|((i, _), _)| members.contains(i))
            .map(|(&(i, j), s)| {
                let (a, b) = (&items[i], &items[j]);
                if a.role == b.role {
                    format!(
                        "{s:.2} between {}:{} and {}:{}",
                        a.path, a.line, b.path, b.line
                    )
                } else {
                    format!(
                        "{s:.2} between {}/{}:{} and {}/{}:{}",
                        a.role, a.path, a.line, b.role, b.path, b.line
                    )
                }
            })
            .collect();
        let listed: Vec<String> = members
            .iter()
            .map(|&i| {
                format!(
                    "{}/{}:{}: \"{}\"",
                    items[i].role,
                    items[i].path,
                    items[i].line,
                    clip(&items[i].text, 160)
                )
            })
            .collect();
        out.push(Finding {
            role: role.to_string(),
            saw: format!(
                "{} entries say nearly the same {where_}: {}",
                members.len(),
                if roles.len() == 1 {
                    "merge them into one"
                } else {
                    "lift what they share into one place, a shared skill or the level above, instead of each role keeping its own copy"
                }
            ),
            evidence: format!(
                "lore {lore}:\n{}\nalike (shared words, word order, or the shorter said again in the longer): {}",
                listed.join("\n"),
                pairs.join("; ")
            ),
        });
    }
    out.sort_by(|a, b| (&a.role, &a.evidence).cmp(&(&b.role, &b.evidence)));
    out
}

/// A TOML basic string.
pub fn toml_string(s: &str) -> String {
    let mut out = String::from('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\u{:04X}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Each finding as a `[[finding]]` table: role, saw, evidence.
pub fn render(found: &[Finding]) -> String {
    found
        .iter()
        .map(|f| {
            format!(
                "[[finding]]\nrole = {}\nsaw = {}\nevidence = {}\n",
                toml_string(&f.role),
                toml_string(&f.saw),
                toml_string(&f.evidence)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn the_sequence_ratio_is_difflibs() {
        // Values Python's difflib.SequenceMatcher(None, a, b, autojunk=False).ratio() gives.
        assert_eq!(sequence_ratio(&w("a b c d"), &w("a b c d")), 1.0);
        assert_eq!(sequence_ratio(&w("a b c d"), &w("d c b a")), 0.25);
        assert_eq!(
            sequence_ratio(&w("x a b y c d"), &w("a b z c d x")),
            2.0 * 4.0 / 12.0
        );
        assert_eq!(
            sequence_ratio(&w("a b a b a"), &w("b a b")),
            2.0 * 3.0 / 8.0
        );
    }

    #[test]
    fn words_drop_stop_words_and_stem() {
        assert_eq!(
            words("The policies' tests: it's Bob's class"),
            ["policy", "test", "bob", "class"]
        );
    }

    #[test]
    fn entries_are_items_paragraphs_and_blocks() {
        let text = "# h\n\npara one\nstill\n- item\n  more\n1. next\n```\ncode\n```\n";
        let got: Vec<usize> = entries(text).into_iter().map(|(n, _)| n).collect();
        assert_eq!(got, [3, 5, 7, 8]);
    }
}
