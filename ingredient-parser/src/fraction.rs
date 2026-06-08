use nom::{
    branch::alt,
    bytes::complete::{tag, tag_no_case},
    character::complete::{space0, space1},
    combinator::{opt, recognize},
    error::{context, ParseError},
    number::complete::double,
    Parser,
};

use crate::Res;

/// Every unicode vulgar-fraction glyph the parser recognizes, as a single string.
///
/// This is the one source of truth for the glyph set: `v_frac_to_num` (and thus
/// [`is_vulgar`]) must map exactly these, and the pre/post-parse regexes that need
/// a fraction character class build it from this const rather than re-listing the
/// glyphs (which previously drifted — the regexes had omitted `⅐ ⅑ ⅒`). Kept in
/// lockstep with `v_frac_to_num` by `tests::vulgar_fractions_match_is_vulgar`.
pub const VULGAR_FRACTIONS: &str = "¼½¾⅐⅑⅒⅓⅔⅕⅖⅗⅘⅙⅚⅛⅜⅝⅞";

fn v_frac_to_num(input: char) -> Option<f64> {
    // two ranges for unicode fractions
    // https://www.compart.com/en/unicode/search?q=vulgar+fraction#characters
    let (n, d): (i32, i32) = match input {
        '¾' => (3, 4),
        '⅛' => (1, 8),
        '¼' => (1, 4),
        '⅓' => (1, 3),
        '½' => (1, 2),
        '⅔' => (2, 3),
        // Adding more common unicode fractions
        '⅕' => (1, 5),
        '⅖' => (2, 5),
        '⅗' => (3, 5),
        '⅘' => (4, 5),
        '⅙' => (1, 6),
        '⅚' => (5, 6),
        '⅐' => (1, 7),
        '⅑' => (1, 9),
        '⅒' => (1, 10),
        '⅜' => (3, 8),
        '⅝' => (5, 8),
        '⅞' => (7, 8),
        _ => return None,
    };
    Some(n as f64 / d as f64)
}

/// Whether `c` is a unicode vulgar-fraction glyph (½, ⅓, ¼, …).
pub fn is_vulgar(c: char) -> bool {
    v_frac_to_num(c).is_some()
}

/// parses unicode vulgar fractions
fn v_fraction(input: &str) -> Res<&str, f64> {
    // Get the first character and try to convert it
    let mut chars = input.chars();
    match chars.next().and_then(v_frac_to_num) {
        Some(val) => {
            // Advance past the unicode fraction character
            let char_len = input.chars().next().map_or(0, |c| c.len_utf8());
            Ok((&input[char_len..], val))
        }
        None => Err(nom::Err::Error(
            nom_language::error::VerboseError::from_error_kind(
                input,
                nom::error::ErrorKind::Satisfy,
            ),
        )),
    }
}
/// Parse a finite f64, rejecting the non-finite spellings `nom::double` accepts.
///
/// Rust's float parser (and thus `nom::double`) treats "inf", "infinity", and
/// "nan" as valid floats, so without this guard "inf/2" or "nan ½" would parse
/// as a numeric value. Reject any non-finite result so only real numbers parse.
/// Shared with `measurement::number`, which uses it for the plain-decimal path.
pub(crate) fn finite_double(input: &str) -> Res<&str, f64> {
    let (remaining, value) = double(input)?;
    if value.is_finite() {
        Ok((remaining, value))
    } else {
        Err(nom::Err::Error(
            nom_language::error::VerboseError::from_error_kind(input, nom::error::ErrorKind::Float),
        ))
    }
}

fn n_fraction(input: &str) -> Res<&str, f64> {
    context("n_fraction", (finite_double, tag("/"), finite_double))
        .parse(input)
        .and_then(|(next_input, res)| {
            if res.2 == 0.0 {
                Err(nom::Err::Error(
                    nom_language::error::VerboseError::from_error_kind(
                        input,
                        nom::error::ErrorKind::Verify,
                    ),
                ))
            } else {
                Ok((next_input, res.0 / res.2))
            }
        })
}

/// Parses mixed number formats like `1 ⅛` or `1 1/8` into `1.125`
///
/// This parser handles both unicode vulgar fractions and standard slash-notation fractions,
/// either alone or with a whole number component.
pub fn fraction_number(input: &str) -> Res<&str, f64> {
    use crate::traced_parser;

    // Define parser for unicode vulgar fractions with optional whole number
    let vulgar_fraction_parser = (
        opt((finite_double, space0)), // Optional whole number with optional whitespace
        v_fraction,                   // Unicode vulgar fraction like ½, ¼, etc.
    );

    // Separator between a whole number and a slash fraction: either plain
    // whitespace ("1 1/2") or a spelled-out "and" ("1 and 1/2"). The "and" form
    // is tried first so the leading space isn't consumed by `space1` alone.
    let whole_fraction_sep = alt((recognize((space1, tag_no_case("and"), space1)), space1));

    // Define parser for slash-notation fractions with optional whole number
    let slash_fraction_parser = (
        opt((finite_double, whole_fraction_sep)), // Optional whole number + separator
        n_fraction,                               // Standard fraction notation like 1/4, 3/8, etc.
    );

    traced_parser!(
        "fraction_number",
        input,
        context(
            "fraction_number",
            alt((vulgar_fraction_parser, slash_fraction_parser)),
        )
        .parse(input)
        .map(|(next_input, res)| {
            let (whole_number, fractional_part) = res;
            let whole_value = whole_number.map_or(0.0, |(num, _)| num);
            (next_input, whole_value + fractional_part)
        }),
        |v: &f64| format!("{v}"),
        "no fraction"
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use nom::error::ErrorKind;
    use nom::Err as NomErr;
    use nom_language::error::{VerboseError, VerboseErrorKind};
    use rstest::rstest;

    use super::{fraction_number, v_frac_to_num};

    // ============================================================================
    // Unicode Vulgar Fraction Character Tests
    // ============================================================================

    #[rstest]
    #[case::half('½', 0.5)]
    #[case::quarter('¼', 0.25)]
    #[case::three_quarter('¾', 0.75)]
    #[case::eighth('⅛', 0.125)]
    #[case::three_eighths('⅜', 0.375)]
    #[case::five_eighths('⅝', 0.625)]
    #[case::seven_eighths('⅞', 0.875)]
    #[case::third('⅓', 1.0 / 3.0)]
    #[case::two_thirds('⅔', 2.0 / 3.0)]
    #[case::fifth('⅕', 0.2)]
    #[case::two_fifths('⅖', 0.4)]
    #[case::three_fifths('⅗', 0.6)]
    #[case::four_fifths('⅘', 0.8)]
    #[case::sixth('⅙', 1.0 / 6.0)]
    #[case::five_sixths('⅚', 5.0 / 6.0)]
    #[case::seventh('⅐', 1.0 / 7.0)]
    #[case::ninth('⅑', 1.0 / 9.0)]
    #[case::tenth('⅒', 0.1)]
    fn test_v_frac_to_num(#[case] char: char, #[case] expected: f64) {
        assert_eq!(v_frac_to_num(char), Some(expected));
    }

    #[test]
    fn test_v_frac_to_num_invalid() {
        assert_eq!(v_frac_to_num('x'), None);
        assert_eq!(v_frac_to_num('1'), None);
    }

    /// `VULGAR_FRACTIONS` and `is_vulgar` must agree exactly, so the regexes built
    /// from the const recognize precisely the glyphs the parser does. Both
    /// directions: every const glyph is vulgar, and no vulgar glyph (scanning the
    /// range that holds them all) is missing from the const.
    #[test]
    fn vulgar_fractions_match_is_vulgar() {
        use super::{is_vulgar, VULGAR_FRACTIONS};
        assert!(VULGAR_FRACTIONS.chars().all(is_vulgar));
        for c in '\u{0}'..='\u{2200}' {
            assert_eq!(
                is_vulgar(c),
                VULGAR_FRACTIONS.contains(c),
                "is_vulgar and VULGAR_FRACTIONS disagree on {c:?} (U+{:04X})",
                c as u32
            );
        }
    }

    // ============================================================================
    // Fraction Parser Tests - Unicode Fractions
    // ============================================================================

    #[rstest]
    #[case::half("½", 0.5)]
    #[case::quarter("¼", 0.25)]
    #[case::three_quarter("¾", 0.75)]
    #[case::eighth("⅛", 0.125)]
    #[case::three_eighths("⅜", 0.375)]
    #[case::five_eighths("⅝", 0.625)]
    #[case::seven_eighths("⅞", 0.875)]
    #[case::third("⅓", 1.0 / 3.0)]
    #[case::two_thirds("⅔", 2.0 / 3.0)]
    #[case::fifth("⅕", 0.2)]
    #[case::two_fifths("⅖", 0.4)]
    #[case::three_fifths("⅗", 0.6)]
    #[case::four_fifths("⅘", 0.8)]
    #[case::sixth("⅙", 1.0 / 6.0)]
    #[case::five_sixths("⅚", 5.0 / 6.0)]
    #[case::seventh("⅐", 1.0 / 7.0)]
    #[case::ninth("⅑", 1.0 / 9.0)]
    #[case::tenth("⅒", 0.1)]
    fn test_fraction_number_unicode(#[case] input: &str, #[case] expected: f64) {
        assert_eq!(fraction_number(input), Ok(("", expected)));
    }

    // ============================================================================
    // Fraction Parser Tests - Slash Notation
    // ============================================================================

    #[rstest]
    #[case::quarter("1/4", 0.25)]
    #[case::half("1/2", 0.5)]
    #[case::eighth("1/8", 0.125)]
    #[case::third("1/3", 1.0 / 3.0)]
    #[case::three_quarters("3/4", 0.75)]
    fn test_fraction_number_slash(#[case] input: &str, #[case] expected: f64) {
        assert_eq!(fraction_number(input), Ok(("", expected)));
    }

    // ============================================================================
    // Fraction Parser Tests - Mixed Numbers
    // ============================================================================

    #[rstest]
    #[case::one_and_eighth_unicode("1 ⅛", 1.125)]
    #[case::one_and_eighth_slash("1 1/8", 1.125)]
    #[case::one_and_third_no_space("1⅓", 1.0 + 1.0 / 3.0)]
    #[case::one_and_three_quarter("1¾", 1.75)]
    #[case::two_and_third("2 ⅓", 2.0 + 1.0 / 3.0)]
    #[case::three_and_fifth("3⅕", 3.2)]
    #[case::one_and_sixth("1 ⅙", 1.0 + 1.0 / 6.0)]
    #[case::two_and_seventh("2⅐", 2.0 + 1.0 / 7.0)]
    #[case::one_and_half_word("1 and 1/2", 1.5)]
    #[case::two_and_third_word("2 and 1/3", 2.0 + 1.0 / 3.0)]
    fn test_fraction_number_mixed(#[case] input: &str, #[case] expected: f64) {
        assert_eq!(fraction_number(input), Ok(("", expected)));
    }

    // ============================================================================
    // Fraction Parser Tests - Error Cases
    // ============================================================================

    #[rstest]
    #[case::one_over_zero("1/0")]
    #[case::zero_over_zero("0/0")]
    fn test_fraction_zero_denominator(#[case] input: &str) {
        assert!(fraction_number(input).is_err(), "should reject {input}");
    }

    #[test]
    fn test_fraction_zero_numerator() {
        assert_eq!(fraction_number("0/1"), Ok(("", 0.0)));
    }

    /// `nom::double` accepts "inf"/"infinity"/"nan"; the fraction parsers must
    /// reject them (via `finite_double`) so "inf/2" or "nan ½" never parse as a
    /// numeric value. Regression for the finite-guard bypass.
    #[rstest]
    #[case::inf_numerator("inf/2")]
    #[case::infinity_numerator("infinity/2")]
    #[case::nan_numerator("nan/1")]
    #[case::inf_denominator("1/inf")]
    #[case::inf_whole_vulgar("inf ½")]
    #[case::inf_whole_slash("inf 1/2")]
    fn test_fraction_rejects_non_finite(#[case] input: &str) {
        assert!(fraction_number(input).is_err(), "should reject {input}");
    }

    #[test]
    fn test_fraction_number_error() {
        // Just a number without fraction should fail
        assert_eq!(
            fraction_number("1"),
            Err(NomErr::Error(VerboseError {
                errors: vec![
                    ("", VerboseErrorKind::Nom(ErrorKind::Tag)),
                    ("1", VerboseErrorKind::Context("n_fraction")),
                    ("1", VerboseErrorKind::Nom(ErrorKind::Alt)),
                    ("1", VerboseErrorKind::Context("fraction_number")),
                ]
            }))
        );
    }
}
