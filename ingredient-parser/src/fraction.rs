use nom::{
    branch::alt,
    bytes::complete::tag,
    character::complete::{space0, space1},
    combinator::opt,
    error::{context, ParseError},
    number::complete::double,
    Parser,
};

use crate::Res;

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
fn n_fraction(input: &str) -> Res<&str, f64> {
    context("n_fraction", (double, tag("/"), double))
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
    use crate::trace::{trace_enter, trace_exit_failure, trace_exit_success};
    trace_enter("fraction_number", input);

    // Define parser for unicode vulgar fractions with optional whole number
    let vulgar_fraction_parser = (
        opt((double, space0)), // Optional whole number with optional whitespace
        v_fraction,            // Unicode vulgar fraction like ½, ¼, etc.
    );

    // Define parser for slash-notation fractions with optional whole number
    let slash_fraction_parser = (
        opt((double, space1)), // Optional whole number with required whitespace
        n_fraction,            // Standard fraction notation like 1/4, 3/8, etc.
    );

    // Try both parsers
    let result = context(
        "fraction_number",
        alt((vulgar_fraction_parser, slash_fraction_parser)),
    )
    .parse(input)
    .map(|(next_input, res)| {
        let (whole_number, fractional_part) = res;

        // Extract whole number or default to 0.0
        let whole_value = whole_number.map_or(0.0, |(num, _)| num);

        // Sum whole and fractional parts
        (next_input, whole_value + fractional_part)
    });

    match &result {
        Ok((remaining, value)) => {
            let consumed = input.len() - remaining.len();
            trace_exit_success(consumed, &format!("{value}"));
        }
        Err(_) => trace_exit_failure("no fraction"),
    }
    result
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
