//! The pure part, `decide`, on every input `proptest` makes: what Flux does
//! not say of it.

use pr_get::decide::{self, Number, Repo, Status};
use proptest::prelude::*;
use serde_json::{Map, Value, json};

proptest! {
    #[test]
    fn a_number_is_digits_from_one(text in "\\PC{0,8}") {
        let digits = !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit());
        let expected = digits && text.parse::<u64>().is_ok_and(|n| n >= 1);
        prop_assert_eq!(Number::parse(&text).is_ok(), expected);
        if let Ok(n) = Number::parse(&text) {
            prop_assert_eq!(n.get(), text.parse::<u64>().unwrap());
        }
    }

    #[test]
    fn a_repo_reads_back_as_written(owner in "[A-Za-z0-9_-][A-Za-z0-9._-]{0,8}", name in "[A-Za-z0-9_-][A-Za-z0-9._-]{0,8}") {
        let text = format!("{owner}/{name}");
        prop_assert_eq!(Repo::parse(&text).unwrap().full(), text);
    }

    #[test]
    fn a_repo_with_anything_else_is_refused(text in "\\PC{0,12}") {
        let fine = |p: &str| !p.is_empty() && p != "." && p != ".." && p.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
        let expected = matches!(text.split_once('/'), Some((o, n)) if fine(o) && fine(n));
        prop_assert_eq!(Repo::parse(&text).is_ok(), expected);
    }

    #[test]
    fn curl_s_answer_splits_into_body_and_status(body in "\\PC{0,40}", status in 100u16..=599) {
        let out = format!("{body}\n{status}");
        let (got, rest) = decide::split_response(&out).unwrap();
        prop_assert_eq!(got.code(), status);
        prop_assert_eq!(rest, body.as_str());
    }

    #[test]
    fn a_status_is_from_100_to_599(code in 0u32..100_000) {
        prop_assert_eq!(Status::parse(&code.to_string()).is_some(), (100..=599).contains(&code));
    }

    #[test]
    fn the_fields_selected_are_those_named_in_order(keys in proptest::collection::vec("[a-z]{1,4}", 1..6)) {
        let mut pull = Map::new();
        for (i, key) in keys.iter().enumerate() {
            pull.insert(key.clone(), json!(i));
        }
        let mut wanted: Vec<String> = keys.clone();
        wanted.reverse();
        let fields = decide::fields(&wanted).unwrap();
        let Value::Object(out) = decide::select(pull.clone(), &fields).unwrap() else { unreachable!() };
        let mut unique = wanted.clone();
        let mut seen = std::collections::HashSet::new();
        unique.retain(|k| seen.insert(k.clone()));
        prop_assert_eq!(out.keys().cloned().collect::<Vec<_>>(), unique);
        for (k, v) in &out {
            prop_assert_eq!(Some(v), pull.get(k));
        }
    }

    #[test]
    fn the_api_root_drops_trailing_slashes(slashes in 0usize..4) {
        let url = format!("https://forge.test{}", "/".repeat(slashes));
        prop_assert_eq!(decide::api_root(None, Some(&url)).unwrap(), "https://forge.test/api/v1");
        prop_assert_eq!(decide::api_root(Some(&url), None).unwrap(), "https://forge.test");
    }
}

#[test]
fn a_token_s_debug_hides_it() {
    let token = decide::token("T", Some("abc")).unwrap();
    assert_eq!(format!("{token:?}"), "Token(..)");
}
