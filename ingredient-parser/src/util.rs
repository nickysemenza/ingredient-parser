/// Format a float without trailing zeros (e.g., "2.50" → "2.5")
pub fn num_without_zeroes(val: f64) -> String {
    let mut val = format!("{val:.2}");
    val = val.trim_end_matches('0').trim_end_matches('.').to_string();
    val
}

/// Truncate a float to 3 decimal places (e.g., 1.23456 → 1.234)
/// Used for unit conversion weights to avoid floating point precision issues
pub fn truncate_3_decimals(f: f64) -> f64 {
    f64::trunc(f * 1000.0) / 1000.0
}

/// Round a float to nearest integer
/// Used for final conversion results
pub fn round_to_int(x: f64) -> f64 {
    x.round()
}
