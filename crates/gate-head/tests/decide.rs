//! What decide says for any input: the exit is 0 only when green on HEAD's
//! tree and clean, the gate's failing code is passed on, and the tree of a
//! reused run's line is found whatever its command holds.

use gate_head::decide::{exit_code, gate_code, named_tree, verdict, GateTree, Seen};
use proptest::prelude::*;

fn hex(len: usize) -> impl Strategy<Value = String> {
    proptest::collection::vec(prop::sample::select(b"0123456789abcdef".to_vec()), len)
        .prop_map(|b| String::from_utf8(b).unwrap())
}

proptest! {
    #[test]
    fn exit_0_only_when_green_on_heads_tree_and_clean(gate in 0u8..=255, same: bool, clean: bool) {
        let exit = exit_code(gate, same, clean);
        prop_assert_eq!(exit == 0, gate == 0 && same && clean);
        if gate != 0 {
            prop_assert_eq!(exit, gate);
        } else if !(same && clean) {
            prop_assert_eq!(exit, 1);
        }
    }

    #[test]
    fn a_gate_code_is_passed_on_or_made_1(code in prop_oneof![
        Just(None), Just(Some(0)), Just(Some(255)), Just(Some(256)), Just(Some(-1)),
        any::<i32>().prop_map(Some),
    ]) {
        let got = gate_code(code);
        match code {
            Some(c @ 0..=255) => prop_assert_eq!(i32::from(got), c),
            _ => prop_assert_eq!(got, 1),
        }
    }

    #[test]
    fn the_tree_of_a_reused_line_is_found(command in "[ -~]{0,40}", tree in hex(40), before in "[ -~]{0,30}") {
        let line = format!("gate: reused the green run of `{command}` on tree {tree} at 1794876508 on mac in 0s, on macos-aarch64: nothing ran");
        let output = vec![before, line, "gate: green, but not on record: offline".to_string()];
        prop_assert_eq!(named_tree(&output), Some(tree));
    }

    #[test]
    fn no_tree_is_named_by_lines_of_another_kind(lines in proptest::collection::vec("[ -~]{0,60}", 0..6)) {
        let lines: Vec<String> = lines.into_iter().filter(|l| !l.starts_with("gate: reused")).collect();
        prop_assert_eq!(named_tree(&lines), None);
    }
}

fn seen(commit: &str, tree: &str, status: &[&str]) -> Seen {
    Seen {
        commit: commit.into(),
        tree: tree.into(),
        status: status.iter().map(|s| s.to_string()).collect(),
    }
}

#[test]
fn a_fresh_run_is_of_heads_tree_only_when_clean_at_one_head_throughout() {
    let clean = seen("c1", "t1", &[]);
    let v = verdict(&["gate: green".into()], Some(0), &clean, &clean);
    assert_eq!(v.gate_tree, GateTree::Ran("t1".into()));
    assert_eq!(v.exit, 0);
    let moved = seen("c2", "t2", &[]);
    let v = verdict(&["gate: green".into()], Some(0), &clean, &moved);
    assert_eq!(v.gate_tree, GateTree::None);
    assert_eq!(v.exit, 1);
    let stray = seen("c1", "t1", &["?? target.log"]);
    let v = verdict(&["gate: green".into()], Some(0), &clean, &stray);
    assert_eq!(v.gate_tree, GateTree::None);
    assert_eq!(v.dirty, vec!["?? target.log".to_string()]);
    assert_eq!(v.exit, 1);
}

#[test]
fn a_tree_name_too_short_or_not_hex_is_not_named() {
    for tree in ["abc", "not-a-tree", "87ed25a4177927XX"] {
        let line = format!(
            "gate: reused the green run of `x` on tree {tree} at 1 on m in 0s, on k: nothing ran"
        );
        assert_eq!(named_tree(&[line]), None, "{tree}");
    }
}
