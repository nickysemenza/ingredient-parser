/// Format a float without trailing zeros (e.g., "2.50" → "2.5")
pub fn num_without_zeroes(val: f64) -> String {
    // Trailing zeros/'.' are only ever stripped from the end, so truncate the
    // owned buffer in place rather than allocating a second String for the slice.
    let mut s = format!("{val:.2}");
    let keep = s.trim_end_matches('0').trim_end_matches('.').len();
    s.truncate(keep);
    s
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

/// Format a measurement quantity as an *editable* ASCII fraction: `1.0/3.0` → "1/3",
/// `1.5` → "1 1/2", `34.0` → "34". Recovers any fraction with denominator ≤ 64 (so
/// `0.0625` → "1/16", beyond [`format_quantity`]'s fixed glyph table), and falls back
/// to a trimmed decimal when the value isn't a clean fraction (`0.37` → "0.37").
///
/// Unlike [`format_quantity`] (Unicode vulgar glyphs, for `Display`), this targets
/// editable text fields where ASCII round-trips through a keyboard. Uses a
/// continued-fraction approximation with a `1e-6` recheck to reject false snaps
/// (e.g. `0.37` is near `3/8`, but not within tolerance).
pub fn format_quantity_ascii(val: f64) -> String {
    use num_rational::Rational64;
    const MAX_DENOM: i64 = 64;

    if val == 0.0 {
        return "0".to_string();
    }
    if let Some(r) = Rational64::approximate_float(val) {
        // num-rational normalizes to a positive denominator, so the sign rides on
        // the numerator.
        let (numer, denom) = (*r.numer(), *r.denom());
        if denom != 0
            && denom.abs() <= MAX_DENOM
            && (numer as f64 / denom as f64 - val).abs() < 1e-6
        {
            if denom.abs() == 1 {
                return (numer / denom).to_string();
            }
            let (n, d) = (numer.abs(), denom.abs());
            let (whole, rem) = (n / d, n % d);
            let sign = if val < 0.0 { "-" } else { "" };
            return if whole != 0 {
                format!("{sign}{whole} {rem}/{d}")
            } else {
                format!("{sign}{rem}/{d}")
            };
        }
    }
    // Not a clean fraction: trimmed decimal, a touch more precision than
    // `num_without_zeroes`'s `:.2` since this is shown in an editable field.
    format!("{val:.4}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_quantity_ascii_table() {
        let cases: &[(f64, &str)] = &[
            (1.0 / 3.0, "1/3"),
            (2.0 / 3.0, "2/3"),
            (1.0 / 2.0, "1/2"),
            (3.0 / 4.0, "3/4"),
            (1.5, "1 1/2"),
            (2.5, "2 1/2"),
            (3.0 / 16.0, "3/16"),
            (1.0 / 16.0, "1/16"),
            (34.0, "34"),
            (2.0, "2"),
            (0.0, "0"),
            // Not a clean fraction (0.37 is near 3/8 but outside tolerance) → decimal.
            (0.37, "0.37"),
            // Sign rides on the numerator; covers proper and mixed negatives.
            (-0.5, "-1/2"),
            (-1.5, "-1 1/2"),
        ];
        for (val, expected) in cases {
            assert_eq!(&format_quantity_ascii(*val), expected, "for {val}");
        }
    }
}
