//! What the plan decides, for any outcomes of its steps: the review first,
//! an approve only after the review passed, and nothing past a failure.

use proptest::prelude::*;
use review_approve::plan::{Outcome, Request, Step};

fn outcome() -> impl Strategy<Value = Outcome> {
    prop_oneof![
        3 => Just(Outcome::Succeeded),
        1 => (proptest::option::of(1i32..255)).prop_map(|exit| Outcome::Failed { exit }),
        1 => Just(Outcome::NotStarted { why: "not found".into() }),
    ]
}

proptest! {
    #[test]
    fn no_approve_without_a_passed_review_and_none_past_a_failure(
        pulls in 1usize..6,
        outcomes in proptest::collection::vec(outcome(), 0..10),
    ) {
        let texts: Vec<String> = (1..=pulls).map(|n| format!("guild/forge#{n}")).collect();
        let request = Request::read("guild/tasks#59", "ok", &texts).unwrap();
        let mut run = request.run();
        let mut taken = Vec::new();
        let mut stopped = None;
        for outcome in &outcomes {
            let Some(step) = run.next() else { break };
            taken.push((step, outcome.clone()));
            match run.record(outcome) {
                Ok(next) => run = next,
                Err(s) => { stopped = Some(s); break; }
            }
        }
        // The first step is the review, and only the first.
        for (i, (step, _)) in taken.iter().enumerate() {
            prop_assert_eq!(matches!(step, Step::Review(_)), i == 0);
        }
        // Every step before the last one taken succeeded.
        for (_, outcome) in taken.iter().rev().skip(1) {
            prop_assert_eq!(outcome, &Outcome::Succeeded);
        }
        // An approve was handed out only after the review's success.
        if taken.len() > 1 {
            prop_assert_eq!(&taken[0].1, &Outcome::Succeeded);
        }
        // Approves come in the order given.
        for (i, (step, _)) in taken.iter().enumerate().skip(1) {
            prop_assert_eq!(*step, Step::Approve(&request.pulls()[i - 1]));
        }
        if let Some(s) = stopped {
            let failed = taken.len() - 1;
            prop_assert_eq!(s.step, taken[failed].0.label());
            prop_assert_eq!(s.approved.len(), failed.saturating_sub(1));
            prop_assert_eq!(s.approved.len() + s.not_approved.len(), pulls);
            let code = if failed == 0 { "review_failed" } else { "approve_failed" };
            prop_assert_eq!(s.fault.code, code);
        }
    }
}

#[test]
fn a_pull_is_repo_hash_number_from_one() {
    use review_approve::plan::Pull;
    let p = Pull::parse("guild/forge#6").unwrap();
    assert_eq!((p.repo(), p.number()), ("guild/forge", 6));
    assert_eq!(Pull::parse("a#b#7").unwrap().repo(), "a#b");
    for bad in [
        "",
        "#6",
        "forge",
        "forge#",
        "forge#0",
        "forge#-1",
        "forge#6x",
        "my forge#6",
        "forge#99999999999999999999",
    ] {
        let fault = Pull::parse(bad).unwrap_err();
        assert_eq!(fault.code, "bad_pull", "{bad}");
    }
}

#[test]
fn a_run_with_every_step_done_has_reviewed_and_approved_all() {
    let texts = vec!["guild/forge#1".to_string(), "guild/lore#2".to_string()];
    let request = Request::read("59", "ok", &texts).unwrap();
    let mut run = request.run();
    while run.next().is_some() {
        run = run.record(&Outcome::Succeeded).unwrap();
    }
    assert!(run.reviewed());
    assert_eq!(run.approved(), request.pulls());
}
