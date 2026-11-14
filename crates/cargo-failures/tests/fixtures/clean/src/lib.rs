pub fn add(a: u32, b: u32) -> u32 {
    a + b
}

#[cfg(test)]
mod tests {
    #[test]
    fn adds() {
        assert_eq!(super::add(2, 2), 4);
    }

    #[test]
    fn adds_zero() {
        assert_eq!(super::add(3, 0), 3);
    }
}
