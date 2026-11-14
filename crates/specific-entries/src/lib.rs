//! specific-entries: a detector of the trainer crew. Finds entries of a
//! role's memory (core.md, skills/) that name a specific repo, path, issue
//! or URL, where a role should hold only how it does its kind of work.
//!
//! This crate's root decides, from the files [`world`] read; [`lore`] and
//! [`world`] read lore through Forgejo, and print.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;

pub mod lore;
pub mod py;
pub mod world;

pub const NAME: &str = "specific-entries";

/// Guild's own repos are where every role's work is handed and kept, not a
/// case: naming them is general.
pub const GUILD_REPOS: [&str; 3] = ["guild/tasks", "guild/lore", "guild/forge"];
/// A path under this, one segment deep, is the role's own skill.
const ROLE_OWN: &str = "skills/";

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).unwrap_or_else(|e| panic!("{pattern}: {e}"))
}

static ITEM: LazyLock<Regex> = LazyLock::new(|| re(r"^\s*(?:[-*+]|\d+[.)])\s+"));
static URL: LazyLock<Regex> = LazyLock::new(|| re(r"\bhttps?://[^\s`)>\]]+"));
static CODE: LazyLock<Regex> = LazyLock::new(|| re(r"`([^`\n]+)`"));
static EXTENSION: LazyLock<Regex> = LazyLock::new(|| re(r"\.[A-Za-z][A-Za-z0-9]{0,7}$"));

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

/// A placeholder or pattern names no specific thing: `<repo>/src`,
/// `skills/{name}.md`, `*.rs`.
pub fn generic(token: &str) -> bool {
    token.chars().any(|c| "<>{}*$".contains(c))
}

/// The whitespace-delimited word the span `start..end` sits in.
pub fn around(text: &str, start: usize, end: usize) -> &str {
    let s = text[..start].rfind(py::is_space).map_or(0, |i| {
        i + text[i..].chars().next().map_or(1, char::len_utf8)
    });
    let e = text[end..]
        .find(py::is_space)
        .map_or(text.len(), |i| end + i);
    &text[s..e]
}

/// `\w` of Python's regular expressions.
fn word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// `[\w.-]`.
fn word_dot(c: char) -> bool {
    word(c) || c == '.' || c == '-'
}

/// The character before byte `p` of `s`.
fn before(s: &str, p: usize) -> Option<char> {
    s[..p].chars().next_back()
}

/// The end of a maximal run of `class` from byte `p`.
fn run(s: &str, p: usize, class: impl Fn(char) -> bool) -> usize {
    s[p..].find(|c: char| !class(c)).map_or(s.len(), |i| p + i)
}

/// The end of `#\d+\b` at byte `p`.
fn number_at(s: &str, p: usize) -> Option<usize> {
    let p = p + s[p..].strip_prefix('#').map(|_| 1)?;
    let end = run(s, p, |c| c.is_ascii_digit());
    let bounded = s[end..].chars().next().is_none_or(|c| !word(c));
    (end > p && bounded).then_some(end)
}

/// The end of a match at byte `p` of
/// `(?<![\w/&])(?:([A-Za-z0-9][\w.-]*/[\w.-]+))?#(\d+)\b`: an issue,
/// `owner/repo#n` or `#n`.
fn issue_at(s: &str, p: usize) -> Option<usize> {
    if before(s, p).is_some_and(|c| word(c) || c == '/' || c == '&') {
        return None;
    }
    let repo = || -> Option<usize> {
        let first = s[p..].chars().next().filter(char::is_ascii_alphanumeric)?;
        let owner = run(s, p + first.len_utf8(), word_dot);
        let slash = owner + s[owner..].strip_prefix('/').map(|_| 1)?;
        let name = run(s, slash, word_dot);
        (name > slash).then(|| number_at(s, name)).flatten()
    };
    repo().or_else(|| number_at(s, p))
}

/// The end of `(?:[\w.-]+/)*[\w.-]+/?` at byte `q`, as greedy as Python
/// takes it: segments apart by one slash, and a slash at the end.
fn body_at(s: &str, q: usize) -> Option<usize> {
    let mut end = q;
    loop {
        let seg = run(s, end, word_dot);
        if seg == end {
            return (end > q).then_some(end);
        }
        end = seg;
        if !s[end..].starts_with('/') {
            return Some(end);
        }
        end += 1;
    }
}

/// The end of a match at byte `p` of
/// `(?<![\w.<{/-])(?:(?:~|\.{1,2})/|/)?(?:[\w.-]+/)*[\w.-]+/?`: what may be
/// a path.
fn path_at(s: &str, p: usize) -> Option<usize> {
    if before(s, p).is_some_and(|c| word(c) || ".<{/-".contains(c)) {
        return None;
    }
    let rest = &s[p..];
    let prefix = ["~/", "../", "./", "/"]
        .iter()
        .find(|x| rest.starts_with(**x))
        .map(|x| x.len());
    prefix
        .and_then(|n| body_at(s, p + n))
        .or_else(|| body_at(s, p))
}

/// The spans of each match, leftmost first, none overlapping, as Python's
/// `finditer` finds them.
fn find_iter(s: &str, at: impl Fn(&str, usize) -> Option<usize>) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut p = 0;
    while p <= s.len() {
        if let Some(end) = at(s, p) {
            out.push((p, end));
            p = end;
            continue;
        }
        match s[p..].chars().next() {
            Some(c) => p += c.len_utf8(),
            None => break,
        }
    }
    out
}

/// The kind of a specific thing an entry names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Url,
    Issue,
    Path,
    Repo,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Url => "URL",
            Kind::Issue => "issue",
            Kind::Path => "path",
            Kind::Repo => "repo",
        }
    }
}

/// What an entry names that is specific: a URL, an issue, a path, a repo.
pub fn hits(entry: &str) -> Vec<(Kind, String)> {
    let mut found: Vec<(Kind, String)> = Vec::new();
    let mut taken: Vec<(usize, usize)> = Vec::new();
    let add = |found: &mut Vec<(Kind, String)>,
               taken: &mut Vec<(usize, usize)>,
               kind,
               token: &str,
               span| {
        taken.push(span);
        if !found.iter().any(|(k, t)| *k == kind && t == token) {
            found.push((kind, token.to_string()));
        }
    };
    let free = |taken: &[(usize, usize)], (s, e): (usize, usize)| {
        !taken.iter().any(|&(a, b)| a < e && s < b)
    };
    let strip = |t: &str| t.trim_end_matches(['.', ',', ';', ':']).to_string();
    for m in URL.find_iter(entry) {
        if !generic(m.as_str()) {
            add(
                &mut found,
                &mut taken,
                Kind::Url,
                &strip(m.as_str()),
                (m.start(), m.end()),
            );
        }
    }
    for (s, e) in find_iter(entry, issue_at) {
        if free(&taken, (s, e)) && !generic(around(entry, s, e)) {
            add(&mut found, &mut taken, Kind::Issue, &entry[s..e], (s, e));
        }
    }
    let code: Vec<(usize, usize)> = CODE
        .captures_iter(entry)
        .filter_map(|c| c.get(1))
        .map(|m| (m.start(), m.end()))
        .collect();
    for (s, e) in find_iter(entry, path_at) {
        if !free(&taken, (s, e)) {
            continue;
        }
        let token = strip(&entry[s..e]);
        if !token.contains('/') || generic(around(entry, s, e)) {
            continue;
        }
        let segments: Vec<&str> = token
            .split('/')
            .filter(|x| !["", "~", ".", ".."].contains(x))
            .collect();
        let rooted = ["/", "~/", "./", "../"]
            .iter()
            .any(|r| token.starts_with(r));
        let lettered = segments
            .iter()
            .all(|x| x.bytes().any(|b| b.is_ascii_lowercase()));
        if segments.len() < if rooted { 1 } else { 2 } || !lettered {
            continue; // I/O, 1/2: no path
        }
        if token.starts_with(ROLE_OWN) && token.matches('/').count() == 1 {
            continue; // the role's own skill
        }
        let in_code = code.iter().any(|&(a, b)| a <= s && e <= b);
        let last = segments.last().copied().unwrap_or("");
        if rooted || EXTENSION.is_match(last) || segments.len() >= 3 {
            add(&mut found, &mut taken, Kind::Path, &token, (s, e));
        } else if in_code && !GUILD_REPOS.contains(&token.as_str()) {
            add(&mut found, &mut taken, Kind::Repo, &token, (s, e));
        }
    }
    found
}

/// A finding: the role, what was seen, and the evidence for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub role: String,
    pub saw: String,
    pub evidence: String,
}

/// A role's finding, if any of its entries names something specific: its
/// files (path within the role, then text) in the order read, and `lore`
/// naming where they were read.
pub fn finding(role: &str, files: &[(String, String)], lore: &str) -> Option<Finding> {
    let mut lines = Vec::new();
    let mut named = 0;
    for (path, text) in files {
        for (line, entry) in entries(text) {
            let found = hits(&entry);
            named += usize::from(!found.is_empty());
            for (kind, token) in found {
                lines.push(format!(
                    "{role}/{path}:{line} names the {} `{token}`",
                    kind.name()
                ));
            }
        }
    }
    if lines.is_empty() {
        return None;
    }
    Some(Finding {
        role: role.to_string(),
        saw: format!(
            "{named} {} a specific repo, path or issue: a role holds how it does its kind of work, true beyond the case it came from; the case belongs to its home",
            if named == 1 { "entry names" } else { "entries name" }
        ),
        evidence: format!("lore {lore}:\n{}", lines.join("\n")),
    })
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

    fn named(entry: &str) -> Vec<(&'static str, String)> {
        hits(entry)
            .into_iter()
            .map(|(k, t)| (k.name(), t))
            .collect()
    }

    #[test]
    fn names_what_the_script_named() {
        assert_eq!(
            named("see sunfmin/control#722 and #41."),
            [
                ("issue", "sunfmin/control#722".into()),
                ("issue", "#41".into())
            ]
        );
        assert_eq!(
            named("run `./verify.sh`, then ../x/y"),
            [("path", "./verify.sh".into()), ("path", "../x/y".into())]
        );
        assert_eq!(
            named("from `acme/tap` or `guild/tasks`"),
            [("repo", "acme/tap".into())]
        );
        assert_eq!(named("I/O and 1/2 and a&b#3 and x#4"), []);
        assert_eq!(named("read skills/hand-in.md and <repo>/scripts/check"), []);
        assert_eq!(
            named("https://example.com/a, then"),
            [("URL", "https://example.com/a".into())]
        );
    }

    #[test]
    fn around_is_the_word_a_span_sits_in() {
        assert_eq!(around("a <repo>/src b", 8, 12), "<repo>/src");
        assert_eq!(around("x", 0, 1), "x");
    }
}
