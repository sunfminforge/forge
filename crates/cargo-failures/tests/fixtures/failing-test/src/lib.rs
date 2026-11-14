pub fn add(a: u32, b: u32) -> u32 {
    let unused = 7;
    a + b
}

#[cfg(test)]
mod tests {
    #[test]
    fn adds() {
        println!("printed by a passing test");
        assert_eq!(super::add(2, 2), 4);
    }

    #[test]
    fn adds_wrong() {
        println!("printed before the panic");
        assert_eq!(super::add(2, 2), 5, "two and two");
    }
}
