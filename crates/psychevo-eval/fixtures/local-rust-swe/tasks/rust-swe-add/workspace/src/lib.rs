pub fn add(left: i32, right: i32) -> i32 {
    left - right
}

#[cfg(test)]
pub(crate) mod tests {
    use super::add;

    #[test]
    fn adds_two_numbers() {
        assert_eq!(add(2, 3), 5);
    }
}
