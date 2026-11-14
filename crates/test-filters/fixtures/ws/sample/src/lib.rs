//! A crate whose tests are known by hand: `adds_*` two that pass (one a
//! unit test, one in tests/), `halves_*` one that passes and one that
//! fails, `skipped_one` ignored, and nothing named `nothing_here`.

pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

pub fn half(a: i32) -> i32 {
    a / 2
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_small() {
        assert_eq!(add(1, 2), 3);
    }

    #[test]
    fn halves_even() {
        assert_eq!(half(4), 2);
    }

    #[test]
    fn halves_odd_wrongly() {
        // Fails on purpose: integer division rounds 3 / 2 down to 1.
        assert_eq!(half(3), 2);
    }

    #[test]
    #[ignore]
    fn skipped_one() {}
}
