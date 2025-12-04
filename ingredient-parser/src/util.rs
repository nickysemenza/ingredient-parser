pub fn num_without_zeroes(val: f64) -> String {
    let mut val = format!("{val:.2}");
    val = val.trim_end_matches('0').trim_end_matches('.').to_string();
    val
}
