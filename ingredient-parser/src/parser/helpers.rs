//! Helper functions for parsing ingredients
//!
//! This module contains low-level parsing helpers used by the main ingredient parser.

use nom::{
    IResult, Parser,
    branch::alt,
    bytes::complete::{tag, tag_no_case, take_while_m_n, take_while1},
    character::complete::{alpha1, char},
    combinator::{map_res, opt, recognize},
    error::{ParseError, context},
    multi::{many0, many1},
};
use nom_language::error::VerboseError;

use crate::fraction::{finite_double, fraction_number};
use crate::unit::Measure;

pub(crate) type Res<T, U> = IResult<T, U, VerboseError<T>>;

/// Lowercase `s` only when the result preserves byte length, so offsets found in
/// the lowercase copy align with `s` for slicing. Returns `None` when case-folding
/// changes byte length (e.g. `'İ'` → `"i\u{307}"`).
pub(crate) fn byte_aligned_lowercase(s: &str) -> Option<String> {
    let lower = s.to_lowercase();
    (lower.len() == s.len()).then_some(lower)
}

/// Parse a simple amount string like "4 lb", "$5", "120g", "1/2 cup"
///
/// Crate-internal utility for parsing standalone amount strings without full
/// ingredient context (used by `unit_mapping`). It handles:
/// - Currency prefix: "$5", "$3.50"
/// - Number + unit: "4 lb", "120g", "2.5 cups"
/// - Fractions: "1/2 cup", "1 ½ lb"
///
/// # Examples
///
/// ```ignore
/// let measure = parse_amount_string("4 lb").unwrap();
/// assert_eq!(measure.value(), 4.0);
///
/// let price = parse_amount_string("$5").unwrap();
/// assert_eq!(price.value(), 5.0);
/// ```
pub(crate) fn parse_amount_string(input: &str) -> Result<Measure, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("Empty amount".to_string());
    }

    // Handle currency prefix: "$5", "$3.50"
    if let Some(price_str) = input.strip_prefix('$') {
        return parse_number(price_str.trim())
            .map(|(_, value)| Measure::new("dollar", value))
            .map_err(|_| format!("Invalid price value: '{input}'"));
    }

    // Parse number (supports fractions, decimals)
    let (remaining, value) =
        parse_number(input).map_err(|_| format!("Invalid numeric value in: '{input}'"))?;

    // Extract unit from remaining text
    let unit = remaining.trim();
    if unit.is_empty() {
        return Err(format!("Missing unit in: '{input}'"));
    }

    // Parse the unit text (letters only). Any trailing leftover is ignored —
    // a bare amount string still succeeds with the recognized unit.
    let (_leftover, unit_str) =
        parse_unit_text(unit).map_err(|_| format!("Invalid unit in: '{input}'"))?;

    if unit_str.is_empty() {
        return Err(format!("Missing unit in: '{input}'"));
    }

    Ok(Measure::new(unit_str, value))
}

/// Parse a number using fraction or decimal parsing
fn parse_number(input: &str) -> Res<&str, f64> {
    // Fraction first ("1/2", "1 ½"), then thousands-separated integers ("1,000"),
    // then a plain decimal. thousands_number must come before the decimal parser,
    // which would otherwise stop at the comma and parse "1,000" as just 1.
    // finite_double (not raw `double`) so "inf"/"nan" never parse as amounts.
    alt((fraction_number, thousands_number, finite_double)).parse(input)
}

/// Parse a number written with thousands separators, e.g. "1,000" or "1,000,000"
/// (optionally with a decimal part). The comma must be followed by exactly three
/// digits, so this never matches list commas like "flour, sifted" or a European
/// decimal comma like "1,5".
pub(crate) fn thousands_number(input: &str) -> Res<&str, f64> {
    let is_digit = |c: char| c.is_ascii_digit();
    map_res(
        recognize((
            take_while_m_n(1, 3, is_digit),
            many1((char(','), take_while_m_n(3, 3, is_digit))),
            opt((char('.'), take_while1(is_digit))),
        )),
        |s: &str| s.replace(',', "").parse::<f64>(),
    )
    .parse(input)
}

/// Parse text characters for ingredient names.
///
/// Consumes a contiguous run of: alphanumeric, whitespace, hyphens, apostrophes,
/// periods, slashes, backslashes, em-dashes, and right single quotes.
///
/// Note: This is more restrictive than `rich_text::parse_rich_char()` which also allows
/// punctuation like commas, parentheses, semicolons, etc. for parsing recipe
/// instructions rather than ingredient names.
pub(crate) fn parse_ingredient_text(input: &str) -> Res<&str, &str> {
    take_while1(|c: char| match c {
        '-' | '\u{2014}' | '\'' | '\u{2019}' | '.' | '/' | '\\' => true,
        c => c.is_alphanumeric() || c.is_whitespace(),
    })
    .parse(input)
}

/// Parse unit/amount text including degrees and quotes.
///
/// Returns the consumed slice directly via `recognize`, avoiding the per-token
/// `Vec<&str>` + `join` allocation this used to do on every unit parse.
pub(crate) fn parse_unit_text(input: &str) -> Res<&str, &str> {
    recognize(many0(alt((alpha1, tag("°"), tag("\""))))).parse(input)
}

/// Match a spelled-out number word, requiring a trailing word boundary
/// (whitespace or end of input). Without this, `tag("ten")` would match inside
/// "tenderloin" and `tag("one")` inside a hyphenated "five-spice", producing
/// nonsense amounts. Case-insensitive so a capitalized leading count
/// ("One 10-ounce disk") parses like its lowercase form.
fn number_word<'a>(word: &'static str, value: f64, input: &'a str) -> Res<&'a str, f64> {
    let (rest, _) = tag_no_case(word).parse(input)?;
    match rest.chars().next() {
        None => Ok((rest, value)),
        Some(c) if c.is_whitespace() => Ok((rest, value)),
        _ => Err(nom::Err::Error(VerboseError::from_error_kind(
            input,
            nom::error::ErrorKind::Tag,
        ))),
    }
}

/// Try each spelled-out number word from [`vocab::NUMBER_WORDS`], longest first.
fn try_spelled_number_words(input: &str) -> Res<&str, f64> {
    for &(word, value) in crate::parser::vocab::NUMBER_WORDS {
        if let Ok(result) = number_word(word, value, input) {
            return Ok(result);
        }
    }
    Err(nom::Err::Error(VerboseError::from_error_kind(
        input,
        nom::error::ErrorKind::Tag,
    )))
}

/// Parse spelled-out text numbers: integer words ("one".."twelve", "dozen") and
/// the articles "a"/"an" (which mean a quantity of one). Numeric words require a
/// word boundary so they never match inside a larger word.
pub(crate) fn text_number(input: &str) -> Res<&str, f64> {
    context(
        "text_number",
        alt((
            try_spelled_number_words,
            |i| tag_no_case("an ").parse(i).map(|(r, _)| (r, 1.0)),
            |i| tag_no_case("a ").parse(i).map(|(r, _)| (r, 1.0)),
        )),
    )
    .parse(input)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use rstest::rstest;

    // ============================================================================
    // parse_ingredient_text() Parser Tests
    // ============================================================================

    #[rstest]
    #[case::letter("a", "", "a")]
    #[case::word("flour", "", "flour")]
    #[case::hyphen("-", "", "-")]
    #[case::em_dash("—", "", "—")]
    #[case::apostrophe("'", "", "'")]
    #[case::right_quote("\u{2019}", "", "\u{2019}")]
    #[case::period(".", "", ".")]
    #[case::slash("and/or", "", "and/or")]
    #[case::backslash("\\", "", "\\")]
    #[case::space(" ", "", " ")]
    #[case::multiword("all-purpose flour", "", "all-purpose flour")]
    fn test_parse_ingredient_text(
        #[case] input: &str,
        #[case] remaining: &str,
        #[case] expected: &str,
    ) {
        assert_eq!(parse_ingredient_text(input), Ok((remaining, expected)));
    }

    // ============================================================================
    // parse_unit_text() Parser Tests
    // ============================================================================

    #[rstest]
    #[case::unit("cups", "", "cups")]
    #[case::degrees("°F", "", "°F")]
    #[case::quote("\"", "", "\"")]
    #[case::short_unit("oz", "", "oz")]
    #[case::empty("", "", "")]
    fn test_parse_unit_text(#[case] input: &str, #[case] remaining: &str, #[case] expected: &str) {
        assert_eq!(parse_unit_text(input), Ok((remaining, expected)));
    }

    // ============================================================================
    // text_number() Parser Tests
    // ============================================================================

    #[rstest]
    #[case::one("one", "", 1.0)]
    #[case::two("two", "", 2.0)]
    #[case::three("three", "", 3.0)]
    #[case::twelve("twelve", "", 12.0)]
    #[case::dozen("dozen", "", 12.0)]
    #[case::a("a ", "", 1.0)]
    #[case::an("an ", "", 1.0)]
    // Articles are case-insensitive like the number words ("An egg", "A cup").
    #[case::a_capitalized("A cup", "cup", 1.0)]
    #[case::an_capitalized("An egg", "egg", 1.0)]
    #[case::half("half", "", 0.5)]
    #[case::half_a("half a cup", " a cup", 0.5)]
    #[case::two_with_remainder("two eggs", " eggs", 2.0)]
    #[case::ten_with_remainder("ten cloves", " cloves", 10.0)]
    fn test_text_number_success(
        #[case] input: &str,
        #[case] remaining: &str,
        #[case] expected: f64,
    ) {
        assert_eq!(text_number(input), Ok((remaining, expected)));
    }

    #[rstest]
    #[case::digit("1")]
    #[case::word("flour")]
    // Numeric words must hit a word boundary: "ten" must not match inside "tenderloin".
    #[case::embedded_ten("tenderloin")]
    #[case::embedded_one("oner")]
    #[case::hyphenated("five-spice")]
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
        assert_eq!(measure.value(), expected);
    }

    #[test]
    fn test_parse_amount_string_fraction() {
        let measure = parse_amount_string("1/2 cup").unwrap();
        assert!((measure.value() - 0.5).abs() < 0.001);
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
    // Non-finite spellings must not parse as numbers (finite_double guard):
    // "$inf" would otherwise clamp to i64::MAX, "nan lb" to a 0-lb measure.
    #[case::inf_value("inf lb", "Invalid")]
    #[case::nan_value("nan lb", "Invalid")]
    #[case::inf_currency("$inf", "Invalid price")]
    #[case::nan_currency("$nan", "Invalid price")]
    fn test_parse_amount_string_error(#[case] input: &str, #[case] expected_error: &str) {
        let result = parse_amount_string(input);
        assert!(result.is_err());
        assert!(
            result.unwrap_err().contains(expected_error),
            "Expected error containing '{expected_error}'"
        );
    }

    #[test]
    fn test_currency_with_leftover() {
        // "$5x" parses "5", leaving "x" as leftover but still succeeds
        let measure = parse_amount_string("$5x").unwrap();
        assert_eq!(measure.value(), 5.0);
    }
}
