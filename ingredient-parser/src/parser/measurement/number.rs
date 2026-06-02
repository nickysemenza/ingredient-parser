//! Number parsing for measurements

use nom::{
    branch::alt, bytes::complete::tag, character::complete::space1, combinator::opt,
    error::context, error::ParseError, number::complete::double, Parser,
};

use crate::fraction::fraction_number;
use crate::parser::{text_number, thousands_number, Res};
use crate::traced_parser;

use super::MeasurementParser;

/// Parse a finite f64, rejecting the non-finite spellings `nom::double` accepts.
///
/// Rust's float parser (and thus `nom::double`) treats "inf", "infinity", and
/// "nan" as valid floats. In prose that means words like "Reinforce" or
/// "infused" get their "inf" matched as a `Measure { value: inf }`. Reject any
/// non-finite result so only real numbers parse.
fn finite_double(input: &str) -> Res<&str, f64> {
    let (remaining, value) = double(input)?;
    if value.is_finite() {
        Ok((remaining, value))
    } else {
        Err(nom::Err::Error(
            nom_language::error::VerboseError::from_error_kind(input, nom::error::ErrorKind::Float),
        ))
    }
}

/// Parse a double but don't consume trailing periods that aren't part of decimals.
///
/// The standard `nom::double` parser treats "375." as a valid number and consumes
/// the trailing period. This causes issues in rich text mode where "375. Combine"
/// would have the period consumed, leaving " Combine" which (after space0)
/// becomes "Combine" and triggers step number detection.
///
/// This parser ensures trailing periods are only consumed if followed by a digit.
fn double_no_trailing_period(input: &str) -> Res<&str, f64> {
    let (remaining, value) = finite_double(input)?;

    // Calculate what was consumed
    let consumed_len = input.len() - remaining.len();
    let consumed = &input[..consumed_len];

    // Check if we consumed a trailing period (like "375.")
    // A true decimal like "375.5" wouldn't end with a period after parsing
    if consumed.ends_with('.') {
        // Give back the period - return the input from one character earlier
        let new_remaining = &input[consumed_len - 1..];
        return Ok((new_remaining, value));
    }

    Ok((remaining, value))
}

impl<'a> MeasurementParser<'a> {
    /// Parse numeric values including fractions, decimals, and text numbers like "one"
    pub(super) fn parse_number<'b>(&self, input: &'b str) -> Res<&'b str, f64> {
        // Choose parsers based on whether we're in rich text mode
        traced_parser!(
            "parse_number",
            input,
            if self.is_rich_text {
                // Rich text mode: try fraction or decimal number
                // Use double_no_trailing_period to avoid consuming sentence-ending periods
                context(
                    "number",
                    alt((
                        fraction_number,           // Parse fractions like "½" or "1/2"
                        thousands_number,          // Parse "1,000" before double stops at the comma
                        double_no_trailing_period, // Parse decimals without eating trailing periods
                    )),
                )
                .parse(input)
            } else {
                // Normal mode: try fraction, text number, or decimal
                context(
                    "number",
                    alt((
                        fraction_number,  // Parse fractions like "½" or "1/2"
                        text_number,      // Parse text numbers like "one" or "a"
                        thousands_number, // Parse "1,000" before double stops at the comma
                        finite_double,    // Parse decimals like "2.5" (rejects inf/nan)
                    )),
                )
                .parse(input)
            },
            |v: &f64| format!("{v}"),
            "no number"
        )
    }

    /// Parse a multiplier expression like "2 x" (meaning multiply the following value by 2)
    pub(super) fn parse_multiplier<'b>(&self, input: &'b str) -> Res<&'b str, f64> {
        // Define the format of a multiplier: number + space + "x" + space
        // Note: We intentionally DON'T include × (multiplication sign) here because
        // in UK cookbook format "1 × 400g tin" the × is a separator, not a multiplier.
        let multiplier_format = (
            |a| self.parse_number(a), // The multiplier value
            space1,                   // Required whitespace
            tag("x"),                 // The "x" character
            space1,                   // Required whitespace
        );

        traced_parser!(
            "parse_multiplier",
            input,
            context("multiplier", multiplier_format).parse(input).map(
                |(next_input, (multiplier_value, _, _, _))| {
                    // Return just the numeric value
                    (next_input, multiplier_value)
                },
            ),
            |v: &f64| format!("{v}x"),
            "no multiplier"
        )
    }

    /// Parse a value that may have a range, returning (value, optional_upper_range)
    pub(super) fn parse_value<'b>(&self, input: &'b str) -> Res<&'b str, (f64, Option<f64>)> {
        traced_parser!(
            "parse_value",
            input,
            context(
                "value_with_range",
                alt((
                    |a| self.parse_upper_bound_only(a), // "up to X" or "at most X"
                    |a| self.parse_value_with_optional_range(a), // A value possibly with a range
                )),
            )
            .parse(input),
            |(val, upper): &(f64, Option<f64>)| match upper {
                Some(u) => format!("{val}-{u}"),
                None => format!("{val}"),
            },
            "no value"
        )
    }

    /// Parse a single value possibly followed by a range
    fn parse_value_with_optional_range<'b>(
        &self,
        input: &'b str,
    ) -> Res<&'b str, (f64, Option<f64>)> {
        // Format: numeric value + optional range
        let format = (
            |a| self.parse_number(a),         // The main value
            opt(|a| self.parse_range_end(a)), // Optional range end
        );

        traced_parser!(
            "parse_value_with_optional_range",
            input,
            context("value_with_optional_range", format).parse(input),
            |(val, upper): &(f64, Option<f64>)| match upper {
                Some(u) => format!("{val}-{u}"),
                None => format!("{val}"),
            },
            "no value"
        )
    }

    /// Parse expressions like "up to 5" or "at most 10"
    pub(super) fn parse_upper_bound_only<'b>(
        &self,
        input: &'b str,
    ) -> Res<&'b str, (f64, Option<f64>)> {
        // Format: prefix + number, mapped to (0.0, Some(upper_value))
        // Note: We don't consume leading space here - let the caller handle spacing
        let format = (
            alt((tag("up to"), tag("at most"))), // Upper bound keywords
            nom::character::complete::space0,    // Optional space after keyword
            |a| self.parse_number(a),            // The upper bound value
        )
            .map(|(_, _, upper_value)| (0.0, Some(upper_value)));

        traced_parser!(
            "parse_upper_bound_only",
            input,
            context("upper_bound_only", format).parse(input),
            // Note: upper is always Some when this succeeds (see line 128 above)
            |(_, upper): &(f64, Option<f64>)| format!("up to {}", upper.unwrap_or(0.0)),
            "no upper bound"
        )
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::super::test_support::units;
    use super::super::MeasurementParser;
    use rstest::{fixture, rstest};
    use std::collections::HashSet;

    #[fixture]
    fn units_fx() -> HashSet<String> {
        units()
    }

    #[rstest]
    #[case::up_to("up to 5", 5.0)]
    #[case::at_most("at most 10", 10.0)]
    fn test_upper_bound_only(
        units_fx: HashSet<String>,
        #[case] input: &str,
        #[case] expected: f64,
    ) {
        let parser = MeasurementParser::new(&units_fx, false);
        let result = parser.parse_upper_bound_only(input);
        assert!(result.is_ok());
        let (_, (lower, upper)) = result.unwrap();
        assert_eq!(lower, 0.0);
        assert_eq!(upper, Some(expected));
    }

    #[rstest]
    #[case::decimal("2.5", 2.5)]
    #[case::fraction("1/2", 0.5)]
    #[case::unicode_fraction("½", 0.5)]
    fn test_rich_text_mode(units_fx: HashSet<String>, #[case] input: &str, #[case] expected: f64) {
        let parser = MeasurementParser::new(&units_fx, true);
        let result = parser.parse_number(input);
        assert!(result.is_ok());
        let (_, val) = result.unwrap();
        assert!((val - expected).abs() < 0.001);
    }

    #[rstest]
    fn test_multiplier(units_fx: HashSet<String>) {
        let parser = MeasurementParser::new(&units_fx, false);
        let result = parser.parse_multiplier("2 x ");
        assert!(result.is_ok());
        let (_, mult) = result.unwrap();
        assert_eq!(mult, 2.0);
    }
}
