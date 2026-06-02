//! Range parsing for measurements

use nom::{
    branch::alt,
    bytes::complete::tag,
    character::complete::{space0, space1},
    error::{context, ParseError},
    Parser,
};
use tracing::info;

use crate::parser::Res;
use crate::traced_parser;
use crate::unit::Measure;

use super::{optional_period_or_of, MeasurementParser, DEFAULT_UNIT};

impl<'a> MeasurementParser<'a> {
    /// Parse a *cross-unit* range like "2 teaspoons to 2 tablespoons" — a range
    /// whose two endpoints carry *different* units. Unlike a same-unit range
    /// ("2 to 3 cups"), this can't collapse into one `Measure { upper_value }`,
    /// so both endpoints are returned as separate amounts: `[2 tsp, 2 tbsp]`.
    ///
    /// Requires an explicit unit on *both* sides and that the two units differ;
    /// otherwise it fails so the same-unit range parser handles it.
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
                .and_then(|(next_input, res)| {
                    let (low_val, _, low_unit, _, _, _, high_val, _, high_unit, _) = res;
                    // Same unit → not a cross-unit range; let the range parser fold
                    // it into a single Measure with an upper bound instead.
                    if low_unit.to_lowercase() == high_unit.to_lowercase() {
                        return Err(nom::Err::Error(
                            nom_language::error::VerboseError::from_error_kind(
                                input,
                                nom::error::ErrorKind::Verify,
                            ),
                        ));
                    }
                    Ok((
                        next_input,
                        vec![
                            Measure::from_parts(low_unit.to_lowercase().as_ref(), low_val, None),
                            Measure::from_parts(high_unit.to_lowercase().as_ref(), high_val, None),
                        ],
                    ))
                }),
            |measures: &Vec<Measure>| measures
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" + "),
            "no cross-unit range"
        )
    }

    /// Parse the upper end of a range like "-3", "to 5", "through 10", or "or 2"
    pub(super) fn parse_range_end<'b>(&self, input: &'b str) -> Res<&'b str, f64> {
        // Two possible formats for range syntax:

        // 1. Dash syntax: space + dash + space + number
        let dash_range = (
            space0,                    // Optional space
            alt((tag("-"), tag("–"))), // Dash (including em-dash)
            space0,                    // Optional space
            |a| self.parse_number(a),  // Upper bound number
        );

        // 2. Word syntax: space + keyword + space + number
        let word_range = (
            nom::character::complete::space1,            // Required space
            alt((tag("to"), tag("through"), tag("or"))), // Range keywords
            nom::character::complete::space1,            // Required space
            |a| self.parse_number(a),                    // Upper bound number
        );

        traced_parser!(
            "parse_range_end",
            input,
            context("range_end", alt((dash_range, word_range)))
                .parse(input)
                .map(|(next_input, (_, _, _, upper_value))| {
                    // Return just the upper value
                    (next_input, upper_value)
                }),
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
            nom::combinator::opt(tag("about ")), // Optional "about" for estimates
            |a| self.parse_value(a),             // The lower value
            space0,                              // Optional whitespace
            nom::combinator::opt(|a| self.unit(a)), // Optional unit for lower value
            |a| self.parse_range_end(a),         // The upper range value
            nom::combinator::opt(|a| self.unit(a)), // Optional unit for upper value
            optional_period_or_of,               // Optional period or "of"
        );

        traced_parser!(
            "parse_range_with_units",
            input,
            context("range_with_units", range_format)
                .parse(input)
                .map(|(next_input, res)| {
                    let (_, lower_value, _, lower_unit, upper_val, upper_unit, _) = res;

                    // Check for unit mismatch - both units must be the same if both are specified
                    if upper_unit.is_some() && lower_unit != upper_unit {
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
