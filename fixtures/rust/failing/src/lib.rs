pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

// Seeded bug: subtracts instead of multiplying.
pub fn multiply(a: i32, b: i32) -> i32 {
    a - b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_two_numbers() {
        assert_eq!(add(2, 3), 5);
    }

    #[test]
    fn multiplies_two_numbers() {
        assert_eq!(multiply(2, 3), 6);
    }
}
