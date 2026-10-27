/// Adults are 18 or older.
pub fn is_adult(age: u32) -> bool {
    age >= 18
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boundary_is_checked_on_both_sides() {
        assert!(!is_adult(17));
        assert!(is_adult(18));
        assert!(is_adult(30));
    }
}
