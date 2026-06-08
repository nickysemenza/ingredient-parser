/// Format a float without trailing zeros (e.g., "2.50" → "2.5")
pub fn num_without_zeroes(val: f64) -> String {
    let mut val = format!("{val:.2}");
    val = val.trim_end_matches('0').trim_end_matches('.').to_string();
    val
}

/// Map a fractional part (0..1) to a Unicode vulgar fraction glyph, if it is one
/// of the common cooking fractions. Mirrors the set the parser accepts.
fn vulgar_fraction_glyph(frac: f64) -> Option<&'static str> {
    const TABLE: &[(f64, &str)] = &[
        (1.0 / 2.0, "½"),
        (1.0 / 3.0, "⅓"),
        (2.0 / 3.0, "⅔"),
        (1.0 / 4.0, "¼"),
        (3.0 / 4.0, "¾"),
        (1.0 / 5.0, "⅕"),
        (2.0 / 5.0, "⅖"),
        (3.0 / 5.0, "⅗"),
        (4.0 / 5.0, "⅘"),
        (1.0 / 6.0, "⅙"),
        (5.0 / 6.0, "⅚"),
        (1.0 / 8.0, "⅛"),
        (3.0 / 8.0, "⅜"),
        (5.0 / 8.0, "⅝"),
        (7.0 / 8.0, "⅞"),
    ];
    TABLE
        .iter()
        .find(|(value, _)| (frac - value).abs() < 1e-6)
        .map(|(_, glyph)| *glyph)
}

/// Format a measurement quantity, rendering a Unicode fraction when the value is
/// a clean cooking fraction (e.g. `0.5` → "½", `1.25` → "1¼", `2.0/3.0` → "⅔").
/// Falls back to a plain trimmed decimal otherwise (e.g. `1.23`). This makes
/// `Display` echo fractions faithfully instead of "0.5"/"1.25".
pub fn format_quantity(val: f64) -> String {
    let negative = val < 0.0;
    let abs = val.abs();
    let whole = abs.trunc();
    let frac = abs - whole;

    if let Some(glyph) = vulgar_fraction_glyph(frac) {
        let sign = if negative { "-" } else { "" };
        let whole_part = if whole == 0.0 {
            String::new()
        } else {
            format!("{}", whole as i64)
        };
        return format!("{sign}{whole_part}{glyph}");
    }

    num_without_zeroes(val)
}

/// Truncate a float to 3 decimal places (e.g., 1.23456 → 1.234)
/// Used for unit conversion weights to avoid floating point precision issues
pub fn truncate_3_decimals(f: f64) -> f64 {
    f64::trunc(f * 1000.0) / 1000.0
}

/// Truncate a string to a maximum length, adding "..." if truncated.
///
/// Uses character count, not byte count, to handle unicode correctly.
pub fn truncate_str(s: &str, max_len: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len).collect();
        format!("{truncated}...")
    }
}
