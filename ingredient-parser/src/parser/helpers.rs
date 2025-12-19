//! Helper functions for parsing ingredients
//!
//! This module contains low-level parsing helpers used by the main ingredient parser.

use nom::{
    branch::alt,
    bytes::complete::tag,
    character::complete::{alpha1, satisfy},
    error::context,
    multi::many0,
    number::complete::double,
    IResult, Parser,
};
use nom_language::error::VerboseError;

use crate::fraction::fraction_number;
use crate::unit::Measure;

pub(crate) type Res<T, U> = IResult<T, U, VerboseError<T>>;

/// Parse a simple amount string like "4 lb", "$5", "120g", "1/2 cup"
///
/// This is a public utility for parsing amount strings without full ingredient context.
/// It handles:
/// - Currency prefix: "$5", "$3.50"
/// - Number + unit: "4 lb", "120g", "2.5 cups"
/// - Fractions: "1/2 cup", "1 ½ lb"
///
/// # Examples
/// ```
/// use ingredient::parser::helpers::parse_amount_string;
///
/// let measure = parse_amount_string("4 lb").unwrap();
/// assert_eq!(measure.values().0, 4.0);
///
/// let price = parse_amount_string("$5").unwrap();
/// assert_eq!(price.values().0, 5.0);
/// ```
pub fn parse_amount_string(input: &str) -> Result<Measure, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("Empty amount".to_string());
    }

    // Handle currency prefix: "$5", "$3.50"
    if let Some(price_str) = input.strip_prefix('$') {
        return parse_number_only(price_str.trim())
            .map(|value| Measure::new("dollar", value))
            .map_err(|_| format!("Invalid price value: '{input}'"));
    }

    // Parse number (supports fractions, decimals)
    let (remaining, value) =
        parse_number_nom(input).map_err(|_| format!("Invalid numeric value in: '{input}'"))?;

    // Extract unit from remaining text
    let unit = remaining.trim();
    if unit.is_empty() {
        return Err(format!("Missing unit in: '{input}'"));
    }

    // Parse the unit text (letters only)
    let (leftover, unit_str) = unitamt(unit).map_err(|_| format!("Invalid unit in: '{input}'"))?;

    if unit_str.is_empty() {
        return Err(format!("Missing unit in: '{input}'"));
    }

    // Warn if there's unexpected leftover text (but still succeed)
    if !leftover.trim().is_empty() {
        // Could log warning here, but for now just ignore
    }

    Ok(Measure::new(&unit_str, value))
}

/// Parse a number using fraction or decimal parsing
fn parse_number_nom(input: &str) -> Res<&str, f64> {
    // Try fraction first (handles "1/2", "1 ½", etc.), then fall back to decimal
    alt((fraction_number, double)).parse(input)
}

/// Parse just a number (for currency values)
fn parse_number_only(input: &str) -> Result<f64, ()> {
    parse_number_nom(input)
        .map(|(remaining, value)| {
            // Ensure we consumed the whole input or just whitespace
            if remaining.trim().is_empty() {
                value
            } else {
                // There's leftover - but for "$5.50 extra" we'd fail
                // For simplicity, accept if we got a valid number
                value
            }
        })
        .map_err(|_| ())
}

/// Parse text characters for ingredient names.
///
/// Allows: alphanumeric, whitespace, hyphens, apostrophes, periods, backslashes.
///
/// Note: This is more restrictive than `rich_text::text2()` which also allows
/// punctuation like commas, parentheses, semicolons, etc. for parsing recipe
/// instructions rather than ingredient names.
pub(crate) fn text(input: &str) -> Res<&str, String> {
    satisfy(|c| match c {
        '-' | '—' | '\'' | '\u{2019}' | '.' | '\\' => true,
        c => c.is_alphanumeric() || c.is_whitespace(),
    })
    .parse(input)
    .map(|(next_input, res)| (next_input, res.to_string()))
}

/// Parse unit/amount text including degrees and quotes
pub(crate) fn unitamt(input: &str) -> Res<&str, String> {
    many0(alt((alpha1, tag("°"), tag("\""))))
        .parse(input)
        .map(|(next_input, res)| (next_input, res.join("")))
}

/// Parse text numbers like "one" or "a"
pub(crate) fn text_number(input: &str) -> Res<&str, f64> {
    context("text_number", alt((tag("one"), tag("a "))))
        .parse(input)
        .map(|(next_input, _)| (next_input, 1.0))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use rstest::rstest;

    // ============================================================================
    // text() Parser Tests
    // ============================================================================

    #[rstest]
    #[case::letter("a", "", "a")]
    #[case::word("flour", "lour", "f")]
    #[case::hyphen("-", "", "-")]
    #[case::em_dash("—", "", "—")]
    #[case::apostrophe("'", "", "'")]
    #[case::right_quote("\u{2019}", "", "\u{2019}")]
    #[case::period(".", "", ".")]
    #[case::backslash("\\", "", "\\")]
    #[case::space(" ", "", " ")]
    fn test_text(#[case] input: &str, #[case] remaining: &str, #[case] expected: &str) {
        assert_eq!(text(input), Ok((remaining, expected.to_string())));
    }

    // ============================================================================
    // unitamt() Parser Tests
    // ============================================================================

    #[rstest]
    #[case::unit("cups", "", "cups")]
    #[case::degrees("°F", "", "°F")]
    #[case::quote("\"", "", "\"")]
    #[case::short_unit("oz", "", "oz")]
    #[case::empty("", "", "")]
    fn test_unitamt(#[case] input: &str, #[case] remaining: &str, #[case] expected: &str) {
        assert_eq!(unitamt(input), Ok((remaining, expected.to_string())));
    }

    // ============================================================================
    // text_number() Parser Tests
    // ============================================================================

    #[rstest]
    #[case::one("one", 1.0)]
    #[case::a("a ", 1.0)]
    fn test_text_number_success(#[case] input: &str, #[case] expected: f64) {
        assert_eq!(text_number(input), Ok(("", expected)));
    }

    #[rstest]
    #[case::two("two")]
    #[case::digit("1")]
    fn test_text_number_fail(#[case] input: &str) {
        assert!(text_number(input).is_err());
    }

    // ============================================================================
    // parse_amount_string() Success Tests
    // ============================================================================

    #[rstest]
    #[case::basic("4 lb", 4.0)]
    #[case::currency("$5", 5.0)]
    #[case::currency_decimal("$3.50", 3.5)]
    #[case::extra_text("4 lb extra", 4.0)]
    fn test_parse_amount_string_success(#[case] input: &str, #[case] expected: f64) {
        let measure = parse_amount_string(input).unwrap();
        assert_eq!(measure.values().0, expected);
    }

    #[test]
    fn test_parse_amount_string_fraction() {
        let measure = parse_amount_string("1/2 cup").unwrap();
        assert!((measure.values().0 - 0.5).abs() < 0.001);
    }

    // ============================================================================
    // parse_amount_string() Error Tests
    // ============================================================================

    #[rstest]
    #[case::empty("", "Empty")]
    #[case::whitespace("   ", "Empty")]
    #[case::missing_unit("4", "Missing unit")]
    #[case::missing_unit_space("4 ", "Missing unit")]
    #[case::invalid_number("abc lb", "Invalid")]
    #[case::invalid_currency("$abc", "Invalid price")]
    #[case::non_alpha_unit("5 !!!", "Missing unit")]
    fn test_parse_amount_string_error(#[case] input: &str, #[case] expected_error: &str) {
        let result = parse_amount_string(input);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().contains(expected_error),
            "Expected error containing '{expected_error}'"
        );
    }

    #[test]
    fn test_parse_number_only_with_leftover() {
        // Tests the leftover text branch in parse_number_only (line 93)
        // "$5x" parses "5", leaving "x" as leftover but still succeeds
        // This hits the else branch where remaining.trim() is not empty
        let measure = parse_amount_string("$5x").unwrap();
        assert_eq!(measure.values().0, 5.0);
    }
}
