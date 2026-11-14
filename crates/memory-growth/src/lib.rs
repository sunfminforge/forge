//! memory-growth: a detector of the trainer crew. Finds roles whose memory
//! (core.md, skills/) grew faster than the work they did: lore size per
//! role, read through Forgejo, against the tasks each settled, read from L3.
//!
//! This crate's root decides, from what [`world`] read; [`lore`] and
//! [`world`] read Forgejo and L3, and print.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

pub mod lore;
pub mod py;
pub mod world;

pub const NAME: &str = "memory-growth";
/// Commits of lore's history read back; older than this is outside any window.
pub const MAX_COMMITS: usize = 2000;
pub const DAY: f64 = 86400.0;

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

/// A role's memory at a commit: path, then size.
pub type Sizes = BTreeMap<String, i64>;

/// What changed in a role's memory between two states, file by file,
/// core.md first.
pub fn change(before: &Sizes, after: &Sizes) -> Vec<String> {
    let mut paths: Vec<&String> = before
        .keys()
        .chain(after.keys())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    paths.sort_by_key(|p| (p.as_str() != "core.md", p.as_str()));
    let mut out = Vec::new();
    for p in paths {
        match (before.get(p), after.get(p)) {
            (a, b) if a == b => {}
            (None, Some(b)) => out.push(format!("{p} added ({b} bytes)")),
            (Some(a), None) => out.push(format!("{p} removed ({a} bytes)")),
            (Some(a), Some(b)) => out.push(format!("{p} {a}→{b} ({:+})", b - a)),
            (None, None) => {}
        }
    }
    out
}

/// Days since 1970-01-01 of a civil date.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// The civil date of a day since 1970-01-01.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (
        if m <= 2 {
            yoe + era * 400 + 1
        } else {
            yoe + era * 400
        },
        m,
        d,
    )
}

/// The Unix time of an ISO 8601 date and time, `Z` or an offset; without
/// one it is UTC.
pub fn when(iso: &str) -> Option<f64> {
    let num = |s: &str| -> Option<i64> {
        (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
            .then(|| s.parse().ok())
            .flatten()
    };
    let (date, time) = iso.split_once(['T', ' '])?;
    let mut d = date.split('-');
    let (y, m, dd) = (num(d.next()?)?, num(d.next()?)?, num(d.next()?)?);
    if d.next().is_some() {
        return None;
    }
    let (clock, offset) = match time.find(['Z', 'z', '+', '-']) {
        Some(i) => (&time[..i], &time[i..]),
        None => (time, ""),
    };
    let (hms, frac) = clock.split_once('.').unwrap_or((clock, ""));
    let mut t = hms.split(':');
    let (h, mi) = (num(t.next()?)?, num(t.next()?)?);
    let s = t.next().map(num).unwrap_or(Some(0))?;
    let frac: f64 = if frac.is_empty() {
        0.0
    } else {
        format!("0.{frac}").parse().ok()?
    };
    let off = match offset {
        "" | "Z" | "z" => 0,
        o => {
            let sign = if o.starts_with('-') { -1 } else { 1 };
            let o = o[1..].replace(':', "");
            let (oh, om) = (num(o.get(..2)?)?, num(o.get(2..4).unwrap_or("00"))?);
            sign * (oh * 3600 + om * 60)
        }
    };
    let secs = days_from_civil(y, m, dd) * 86400 + h * 3600 + mi * 60 + s - off;
    Some(secs as f64 + frac)
}

/// The UTC date of a Unix time, `YYYY-MM-DD`.
pub fn day(t: f64) -> String {
    let (y, m, d) = civil_from_days((t / DAY).floor() as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// A number as Python's `format(x, "g")` writes it.
pub fn fmt_g(x: f64) -> String {
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "inf".into() } else { "-inf".into() };
    }
    if x == 0.0 {
        return if x.is_sign_negative() {
            "-0".into()
        } else {
            "0".into()
        };
    }
    let sci = format!("{x:.5e}");
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let exp: i32 = exp.parse().unwrap_or(0);
    let trim = |s: &str| -> String {
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.').to_string()
        } else {
            s.to_string()
        }
    };
    if (-4..6).contains(&exp) {
        trim(&format!("{x:.*}", (5 - exp) as usize))
    } else {
        let sign = if exp < 0 { '-' } else { '+' };
        format!("{}e{sign}{:02}", trim(mantissa), exp.abs())
    }
}

/// A commit of lore's main: sha, time, subject.
#[derive(Clone, Debug, PartialEq)]
pub struct Commit {
    pub sha: String,
    pub time: f64,
    pub subject: String,
}

/// A commit as Forgejo's list of commits gives it, and its parents.
pub fn commit_of(c: &Value) -> Option<(Commit, Vec<String>)> {
    let sha = c.get("sha")?.as_str()?.to_string();
    let parents = c
        .get("parents")
        .and_then(Value::as_array)
        .map(|ps| {
            ps.iter()
                .filter_map(|p| p.get("sha").and_then(Value::as_str))
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let inner = c.get("commit").filter(|v| !v.is_null());
    let date = inner
        .and_then(|i| i.get("committer"))
        .and_then(|c| c.get("date"))
        .and_then(Value::as_str)
        .filter(|d| !d.is_empty())
        .or_else(|| c.get("created").and_then(Value::as_str))?;
    let message = inner
        .and_then(|i| i.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("");
    let subject = py::splitlines(message)
        .first()
        .copied()
        .unwrap_or("")
        .to_string();
    Some((
        Commit {
            sha,
            time: when(date)?,
            subject,
        },
        parents,
    ))
}

/// Lore's main as it landed, newest first, along first parents from `head`.
pub fn chain(head: Option<&str>, commits: &BTreeMap<String, (Commit, Vec<String>)>) -> Vec<Commit> {
    let mut out = Vec::new();
    let mut sha = head.map(str::to_string);
    while let Some((c, parents)) = sha.as_ref().and_then(|s| commits.get(s)) {
        if out.len() > commits.len() {
            break; // a cycle: no history Forgejo would give
        }
        out.push(c.clone());
        sha = parents.first().cloned();
    }
    out
}

/// `{role: [task]}`: the tasks each role settled work on since the window
/// opened, from L3's settlements (role, task, in the order of their first
/// settlement) and its flow records.
///
/// The foreman settles no task of its own: its work is its goals' tasks, so
/// it is credited with every task settled since that belongs to a goal.
pub fn settled(settlements: &[(String, String)], flow: &[String]) -> BTreeMap<String, Vec<String>> {
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut all_tasks: Vec<&String> = Vec::new();
    for (role, task) in settlements {
        out.entry(role.clone()).or_default().push(task.clone());
        if !all_tasks.contains(&task) {
            all_tasks.push(task);
        }
    }
    let mut of_goals = BTreeSet::new();
    for record in flow {
        let Ok(Value::Object(r)) = serde_json::from_str::<Value>(record) else {
            continue;
        };
        let (Some(goal), Some(task)) = (r.get("goal"), r.get("task")) else {
            continue;
        };
        if goal.is_null() || task.is_null() {
            continue;
        }
        of_goals.insert(match task {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        });
    }
    let foreman = out.entry("foreman".into()).or_default();
    for task in all_tasks {
        let num = task.rsplit('#').next().unwrap_or("");
        if of_goals.contains(num) && !foreman.contains(task) {
            foreman.push(task.clone());
        }
    }
    out
}

/// The bars a role's growth is held to.
#[derive(Clone, Copy, Debug)]
pub struct Bars {
    pub days: f64,
    pub bytes_per_task: i64,
    pub min_bytes: i64,
}

/// A finding: the role, what was seen, and the evidence for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub role: String,
    pub saw: String,
    pub evidence: String,
}

/// Each role whose memory grew faster than the work it settled.
///
/// `states` is lore from the window's opening to now: lore as the window
/// opened first when `opened` (when there was lore before it), then each
/// commit in the window, oldest first.
pub fn findings(
    states: &[(Commit, BTreeMap<String, Sizes>)],
    opened: bool,
    tasks: &BTreeMap<String, Vec<String>>,
    since: f64,
    bars: Bars,
) -> Vec<Finding> {
    let Some((_, head)) = states.last() else {
        return Vec::new();
    };
    let total = |s: &Sizes| -> i128 { s.values().map(|v| *v as i128).sum() };
    let mut out = Vec::new();
    for (role, now) in head {
        // From the window's opening, or from when the role first came into
        // lore within it: a new role's first draft is not growth.
        let Some((at, (first, base))) = states
            .iter()
            .enumerate()
            .find_map(|(i, (c, s))| s.get(role).map(|b| (i, (c, b))))
        else {
            continue;
        };
        let grew = total(now) - total(base);
        let none = Vec::new();
        let done = tasks.get(role).unwrap_or(&none);
        let allowed = bars.bytes_per_task as i128 * done.len() as i128;
        if grew < bars.min_bytes as i128 || grew <= allowed {
            continue;
        }
        let mut steps = Vec::new();
        let mut prev = base;
        for (c, s) in states {
            let Some(mine) = s.get(role) else { continue };
            if c.time <= first.time {
                continue;
            }
            let what = change(prev, mine);
            if !what.is_empty() {
                steps.push(format!(
                    "lore {} {} \"{}\": {}",
                    py::take(&c.sha, 10),
                    day(c.time),
                    c.subject,
                    what.join("; ")
                ));
            }
            prev = mine;
        }
        let opened_at = if opened && at == 0 {
            format!(
                "since the window opened {} ({})",
                day(since),
                py::take(&first.sha, 10)
            )
        } else {
            format!(
                "since it came into lore {} ({})",
                day(first.time),
                py::take(&first.sha, 10)
            )
        };
        let per = if done.is_empty() {
            "with no task settled".to_string()
        } else {
            format!("{} bytes per task", grew.div_euclid(done.len() as i128))
        };
        out.push(Finding {
            role: role.clone(),
            saw: format!(
                "its memory (core.md and skills/) grew {grew} bytes, {}→{}, over the last {} days while it settled {} task{} ({per}; at most {} per task): memory growing faster than the work",
                total(base),
                total(now),
                fmt_g(bars.days),
                done.len(),
                if done.len() != 1 { "s" } else { "" },
                bars.bytes_per_task
            ),
            evidence: format!(
                "growth {opened_at}:\n{}\nL3 settlements of {role} since {}: {}",
                steps.join("\n"),
                day(since),
                if done.is_empty() { "none".to_string() } else { done.join(", ") }
            ),
        });
    }
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

    #[test]
    fn g_formats_as_format_g_does() {
        for (x, s) in [
            (14.0, "14"),
            (25.0, "25"),
            (0.5, "0.5"),
            (1.0 / 3.0, "0.333333"),
            (1e6, "1e+06"),
            (123456.0, "123456"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (2.5e-7, "2.5e-07"),
        ] {
            assert_eq!(fmt_g(x), s, "{x}");
        }
    }

    #[test]
    fn dates_read_and_write_as_utc() {
        assert_eq!(when("1970-01-02T00:00:00Z"), Some(86400.0));
        assert_eq!(
            when("2026-11-01T10:00:00+09:00"),
            when("2026-11-01T01:00:00+00:00")
        );
        assert_eq!(
            day(when("2026-03-01T00:00:00-00:30").unwrap()),
            "2026-03-01"
        );
        assert_eq!(day(when("2024-02-29T23:59:59Z").unwrap()), "2024-02-29");
    }

    #[test]
    fn the_foreman_is_credited_with_its_goals_tasks() {
        let s = vec![
            ("a".to_string(), "guild/tasks#2".to_string()),
            ("b".to_string(), "guild/tasks#3".to_string()),
        ];
        let tasks = settled(
            &s,
            &[
                r#"{"task":2,"goal":1}"#.to_string(),
                r#"{"task":3}"#.to_string(),
            ],
        );
        assert_eq!(tasks["foreman"], ["guild/tasks#2"]);
    }
}
