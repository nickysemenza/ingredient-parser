//! Range parsing for measurements

use nom::{
    Parser,
    branch::alt,
    bytes::complete::tag,
    character::complete::{space0, space1},
    error::context,
};
use tracing::info;

use crate::parser::Res;
use crate::traced_parser;
use crate::unit::{Measure, Unit};

use super::{DEFAULT_UNIT, MeasurementParser, optional_period_or_of};

/// Canonical [`Unit`] for a raw unit spelling, so range endpoints compare by
/// unit identity rather than spelling ("tsp" == "teaspoons", "g" == "G").
fn canonical_unit(s: &str) -> Unit {
    use std::str::FromStr;
    // `Unit::from_str` is infallible; `normalize` singularizes unknown units.
    Unit::from_str(s)
        .unwrap_or(Unit::Other(s.to_string()))
        .normalize()
}

impl<'a> MeasurementParser<'a> {
    /// Parse a range with an explicit unit on *both* endpoints, like
    /// "2 teaspoons to 2 tablespoons" or "2 tsp to 3 teaspoons".
    ///
    /// Endpoints with *different* canonical units can't collapse into one
    /// `Measure { upper_value }`, so both are returned as separate amounts:
    /// `[2 tsp, 2 tbsp]`. Endpoints with the *same* canonical unit — however
    /// spelled ("tsp" vs "teaspoons") — fold into one ranged measure; handling
    /// that here (rather than failing over to the unitless-upper range parser)
    /// is what consumes the second unit token out of the name.
    pub(super) fn parse_cross_unit_range<'b>(&self, input: &'b str) -> Res<&'b str, Vec<Measure>> {
        let format = (
            |a| self.parse_number(a), // lower value
            space0,
            |a| self.unit(a), // lower unit (required)
            space1,
            alt((tag("to"), tag("through"), tag("–"), tag("-"))), // range keyword
            space1,
            |a| self.parse_number(a), // upper value
            space1,
            |a| self.unit(a), // upper unit (required)
            optional_period_or_of,
        );

        traced_parser!(
            "parse_cross_unit_range",
            input,
            context("cross_unit_range", format)
                .parse(input)
                .map(|(next_input, res)| {
                    let (low_val, _, low_unit, _, _, _, high_val, _, high_unit, _) = res;
                    // Same canonical unit ("2 tsp to 3 teaspoons") → one ranged
                    // measure; different units → two separate amounts.
                    let measures = if canonical_unit(&low_unit) == canonical_unit(&high_unit) {
                        vec![Measure::from_parts(
                            low_unit.to_lowercase().as_ref(),
                            low_val,
                            Some(high_val),
                        )]
                    } else {
                        vec![
                            Measure::from_parts(low_unit.to_lowercase().as_ref(), low_val, None),
                            Measure::from_parts(high_unit.to_lowercase().as_ref(), high_val, None),
                        ]
                    };
                    (next_input, measures)
                }),
            |measures: &Vec<Measure>| measures
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" + "),
            "no cross-unit range"
        )
    }

    /// Parse the upper end of a range like "-3", "to 5", "through 10", or "or 2".
    ///
    /// Also handles the cookbook "attached-unit" notation "3½- to 4-pound", where a
    /// hyphen is glued to the *lower* bound (and the unit to the upper bound). Here
    /// the real separator is the keyword, not the dash, so the leading dash is
    /// consumed as cruft; the upper unit ("-pound") is resolved downstream by the
    /// single-measurement unit chain (`parse_hyphenated_unit`).
    pub(super) fn parse_range_end<'b>(&self, input: &'b str) -> Res<&'b str, f64> {
        // 1. Dash syntax: space + dash + space + number ("-3", "– 3").
        let dash_range = (
            space0,                    // Optional space
            alt((tag("-"), tag("–"))), // Dash (including em-dash)
            space0,                    // Optional space
            |a| self.parse_number(a),  // Upper bound number
        )
            .map(|(_, _, _, upper)| upper);

        // 2. Attached-unit notation: a dash glued to the lower bound, then the
        //    keyword + number ("3½- to 4" → upper 4, leaving "-pound …"). Tried
        //    before the word form because the leading dash would fail `space1`.
        let attached_dash_range = (
            alt((tag("-"), tag("–"))),        // Dash glued to the lower bound
            space1,                           // Required space
            alt((tag("to"), tag("through"))), // Range keyword
            space1,                           // Required space
            |a| self.parse_number(a),         // Upper bound number
        )
            .map(|(_, _, _, _, upper)| upper);

        // 3. Word syntax: space + keyword + space + number ("to 5", "through 10").
        let word_range = (
            space1,                                      // Required space
            alt((tag("to"), tag("through"), tag("or"))), // Range keywords
            space1,                                      // Required space
            |a| self.parse_number(a),                    // Upper bound number
        )
            .map(|(_, _, _, upper)| upper);

        traced_parser!(
            "parse_range_end",
            input,
            context(
                "range_end",
                alt((dash_range, attached_dash_range, word_range))
            )
            .parse(input),
            |v: &f64| format!("{v}"),
            "no range end"
        )
    }

    /// Parse a range with units, like "78g to 104g" or "2-3 cups"
    pub(super) fn parse_range_with_units<'b>(
        &self,
        input: &'b str,
    ) -> Res<&'b str, Option<Measure>> {
        // Format for a measurement with a range
        let range_format = (
            // Optional approximation qualifier ("about", "roughly", …, any case)
            nom::combinator::opt(super::single::leading_qualifier),
            |a| self.parse_value(a),                // The lower value
            space0,                                 // Optional whitespace
            nom::combinator::opt(|a| self.unit(a)), // Optional unit for lower value
            |a| self.parse_range_end(a),            // The upper range value
            nom::combinator::opt(|a| self.unit(a)), // Optional unit for upper value
            optional_period_or_of,                  // Optional period or "of"
        );

        traced_parser!(
            "parse_range_with_units",
            input,
            context("range_with_units", range_format)
                .parse(input)
                .map(|(next_input, res)| {
                    let (_, lower_value, _, lower_unit, upper_val, upper_unit, _) = res;

                    // Both units, when specified, must canonicalize to the same
                    // unit ("1g-2G", "1g-2grams" are fine; "1g-2tbsp" is not).
                    let mismatch = match (&lower_unit, &upper_unit) {
                        (Some(lo), Some(hi)) => canonical_unit(lo) != canonical_unit(hi),
                        (None, Some(_)) => true,
                        _ => false,
                    };
                    if mismatch {
                        info!(
                            "unit mismatch between range values: {:?} vs {:?}",
                            lower_unit, upper_unit
                        );
                        return (next_input, None);
                    }

                    // Create the measurement with range
                    (
                        next_input,
                        Some(Measure::from_parts(
                            // Use the lower unit, or default to "whole" if not specified
                            lower_unit
                                .unwrap_or_else(|| DEFAULT_UNIT.to_string())
                                .to_lowercase()
                                .as_ref(),
                            lower_value.0,
                            Some(upper_val),
                        )),
                    )
                }),
            |opt_m: &Option<Measure>| opt_m
                .as_ref()
                .map(|m| m.to_string())
                .unwrap_or_else(|| "unit mismatch".to_string()),
            "no range"
        )
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::super::test_support::units;
    use super::super::{MeasurementMode, MeasurementParser};
    use rstest::{fixture, rstest};
    use std::collections::HashSet;

    #[fixture]
    fn units_fx() -> HashSet<String> {
        units()
    }

    /// A cross-unit range "2 tsp to 2 tbsp" yields two separate amounts (it
    /// can't fold into one ranged Measure); same-CANONICAL-unit endpoints —
    /// however spelled — fold into one ranged measure, consuming both unit
    /// tokens so neither leaks into the name.
    #[rstest]
    fn test_cross_unit_range(units_fx: HashSet<String>) {
        let parser = MeasurementParser::new(&units_fx, MeasurementMode::IngredientList);
        let (_, measures) = parser
            .parse_cross_unit_range("2 teaspoons to 2 tablespoons")
            .unwrap();
        assert_eq!(measures.len(), 2);
        assert_eq!(measures[0].unit_as_string(), "tsp");
        assert_eq!(measures[1].unit_as_string(), "tbsp");
        // Same canonical unit on both sides → one ranged measure.
        for input in ["2 cups to 3 cups", "2 tsp to 3 teaspoons"] {
            let (_, measures) = parser.parse_cross_unit_range(input).unwrap();
            assert_eq!(measures.len(), 1, "input: {input}");
            assert_eq!(
                (measures[0].value(), measures[0].upper_value()),
                (2.0, Some(3.0)),
                "input: {input}"
            );
        }
    }

    /// Differently-spelled same units fold into a single ranged measure: the
    /// comparison is canonical-unit, not raw-string ("g" == "G" == "grams").
    #[rstest]
    #[case::alias("1g-2grams")]
    #[case::case_mixed("1g-2G")]
    fn test_range_same_canonical_unit(units_fx: HashSet<String>, #[case] input: &str) {
        let parser = MeasurementParser::new(&units_fx, MeasurementMode::IngredientList);
        let (_, measure) = parser.parse_range_with_units(input).unwrap();
        let measure = measure.unwrap(); // same canonical unit must parse as a range
        assert_eq!(measure.unit_as_string(), "g");
        assert_eq!((measure.value(), measure.upper_value()), (1.0, Some(2.0)));
    }

    /// Unit mismatch in dash-style ranges returns None (e.g. "1g-2tbsp"). Word-style
    /// ranges like "1 cup to 2 tbsp" don't detect mismatch because the space before
    /// the second unit prevents it from being parsed.
    #[rstest]
    #[case::dash_mismatch("1g-2tbsp")]
    fn test_range_unit_mismatch(units_fx: HashSet<String>, #[case] input: &str) {
        let parser = MeasurementParser::new(&units_fx, MeasurementMode::IngredientList);
        let result = parser.parse_range_with_units(input);
        assert!(result.is_ok(), "Failed to parse: {input}");
        let (remaining, opt_measure) = result.unwrap();
        assert!(
            opt_measure.is_none(),
            "Expected None for unit mismatch on '{input}', got {opt_measure:?}, remaining: '{remaining}'",
        );
    }

    /// `parse_range_end` accepts all three forms. The attached-unit case
    /// ("3½- to 4-pound") leaves the hyphen-glued upper unit ("-pound") for the
    /// single-measurement unit chain to resolve — it must NOT be consumed here.
    #[rstest]
    #[case::em_dash("–3", 3.0, "")]
    // Plain hyphen range: the dash IS the separator — the attached branch must
    // not steal it; the unit stays on the input.
    #[case::hyphen("-3 cups", 3.0, " cups")]
    #[case::word_to(" to 5 cups", 5.0, " cups")]
    #[case::word_through(" through 10", 10.0, "")]
    #[case::attached_dash("- to 4-pound chicken", 4.0, "-pound chicken")]
    fn test_range_end(
        units_fx: HashSet<String>,
        #[case] input: &str,
        #[case] expected_upper: f64,
        #[case] expected_remaining: &str,
    ) {
        let parser = MeasurementParser::new(&units_fx, MeasurementMode::IngredientList);
        let (remaining, upper) = parser.parse_range_end(input).unwrap();
        assert_eq!(upper, expected_upper, "upper bound for {input:?}");
        assert_eq!(remaining, expected_remaining, "remaining for {input:?}");
    }
}
