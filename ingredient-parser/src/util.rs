pub fn num_without_zeroes(val: f64) -> String {
    let mut val = format!("{val:.2}");
    val = val.trim_end_matches('0').trim_end_matches('.').to_string();
    val
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_num_without_zeroes() {
        assert_eq!(num_without_zeroes(1.0), "1");
        assert_eq!(num_without_zeroes(1.1), "1.1");
        assert_eq!(num_without_zeroes(1.01), "1.01");
        assert_eq!(num_without_zeroes(1.234), "1.23");
    }
}
