//! The `[[finding]]` tables a detector printed, failing on anything else.

/// A finding as printed: role, saw, and its evidence, one string each.
#[derive(Debug)]
pub struct Finding {
    pub role: String,
    pub saw: String,
    pub evidence: Vec<String>,
}

fn string(line: &str, prefix: &str) -> String {
    let value = line
        .strip_prefix(prefix)
        .unwrap_or_else(|| panic!("not `{prefix}…`: {line:?}"));
    serde_json::from_str(value).unwrap_or_else(|e| panic!("not a basic string: {line:?}: {e}"))
}

/// The findings of `out`: each a `[[finding]]` table of role, saw and an
/// evidence array, one item a line, the tables apart by blank lines.
pub fn parse(out: &str) -> Vec<Finding> {
    let mut found = Vec::new();
    let mut lines = out.lines().peekable();
    while let Some(line) = lines.next() {
        if line.trim().is_empty() {
            continue;
        }
        assert_eq!(line, "[[finding]]", "output beside findings: {out:?}");
        let role = string(lines.next().expect("a role"), "role = ");
        let saw = string(lines.next().expect("a saw"), "saw = ");
        assert_eq!(
            lines.next(),
            Some("evidence = ["),
            "output beside findings: {out:?}"
        );
        let mut evidence = Vec::new();
        loop {
            let line = lines.next().expect("the evidence closed");
            if line == "]" {
                break;
            }
            let item = line
                .strip_prefix("  ")
                .and_then(|l| l.strip_suffix(','))
                .expect("an evidence item");
            evidence.push(serde_json::from_str(item).expect("a basic string"));
        }
        assert!(!evidence.is_empty(), "a finding with no evidence: {out:?}");
        found.push(Finding {
            role,
            saw,
            evidence,
        });
    }
    found
}
