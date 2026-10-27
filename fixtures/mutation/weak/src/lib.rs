/// Adults are 18 or older.
pub fn is_adult(age: u32) -> bool {
    age >= 18
}

#[cfg(test)]
mod tests {
    use super::*;

    // Seeded weakness: only one easy case, the 18 boundary is never checked, so mutants
    // like `>=` -> `>` survive.
    #[test]
    fn thirty_is_adult() {
        assert!(is_adult(30));
    }
}
