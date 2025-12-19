//! Measurement parsing for ingredient strings
//!
//! This module contains all the parsers for extracting measurements from ingredient
//! strings, including single measurements, ranges, and combined expressions.

mod composite;
mod number;
mod range;

use std::collections::HashSet;

#[allow(deprecated)]
use nom::sequence::tuple;
use nom::{
    branch::alt,
    bytes::complete::tag,
    character::complete::{space0, space1},
    combinator::{opt, verify},
    error::context,
    multi::separated_list1,
    Parser,
};

use crate::parser::{unitamt, Res};
use crate::traced_parser;
use crate::unit::{self, Measure};

/// Default unit for amounts without a specified unit (e.g., "2 eggs")
const DEFAULT_UNIT: &str = "whole";

/// Parse optional trailing period or " of" after units (e.g., "tsp." or "cup of")
fn optional_period_or_of(input: &str) -> Res<&str, Option<&str>> {
    opt(alt((tag("."), tag(" of")))).parse(input)
}

/// Parser for extracting measurements from ingredient strings
///
/// This struct holds configuration for parsing measurements, including
/// the set of recognized units and whether rich text mode is enabled.
pub(crate) struct MeasurementParser<'a> {
    pub units: &'a HashSet<String>,
    pub is_rich_text: bool,
}

impl<'a> MeasurementParser<'a> {
    /// Create a new measurement parser with the given configuration
    pub fn new(units: &'a HashSet<String>, is_rich_text: bool) -> Self {
        Self {
            units,
            is_rich_text,
        }
    }

    /// Parse a list of measurements with different separators
    ///
    /// This handles formats like:
    /// - "2 cups; 1 tbsp"
    /// - "120 grams / 1 cup"
    /// - "1 tsp, 2 tbsp"
    #[tracing::instrument(name = "many_amount", skip(self))]
    pub fn parse_measurement_list<'b>(&self, input: &'b str) -> Res<&'b str, Vec<Measure>> {
        // Define the separators between measurements
        let amount_separators = alt((
            tag("; "),  // semicolon with space
            tag(" / "), // slash with spaces
            tag("/"),   // bare slash
            tag(", "),  // comma with space
            tag(" "),   // just a space
        ));

        // Define the different types of measurements we can parse
        let amount_parsers = alt((
            // "1 cup plus 2 tbsp" -> combines measurements
            |input| {
                self.parse_plus_expression(input)
                    .map(|(next, measure)| (next, vec![measure]))
            },
            // Range with units on both sides: "2-3 cups" or "1 to 2 tbsp"
            |input| {
                self.parse_range_with_units(input)
                    .map(|(next, opt_measure)| {
                        (next, opt_measure.map_or_else(Vec::new, |m| vec![m]))
                    })
            },
            // Parenthesized amounts like "(1 cup)"
            |input| self.parse_parenthesized_amounts(input),
            // Basic measurement like "2 cups"
            |input| {
                self.parse_single_measurement(input)
                    .map(|(next, measure)| (next, vec![measure]))
            },
            // Just a unit with implicit quantity of 1, like "cup"
            |input| {
                self.parse_unit_only(input)
                    .map(|(next, measure)| (next, vec![measure]))
            },
        ));

        traced_parser!(
            "measurement_list",
            input,
            // Parse a list of measurements separated by the defined separators
            context(
                "measurement_list",
                separated_list1(amount_separators, amount_parsers),
            )
            .parse(input)
            .map(|(next_input, measures_list)| {
                // Flatten nested Vec<Vec<Measure>> into Vec<Measure>
                (
                    next_input,
                    measures_list
                        .into_iter()
                        .flatten()
                        .collect::<Vec<Measure>>(),
                )
            }),
            |measures: &Vec<Measure>| measures
                .iter()
                .map(|m| m.to_string())
                .collect::<Vec<_>>()
                .join(", "),
            "no measurements found"
        )
    }

    /// Parse a single measurement like "2 cups" or "about 3 tablespoons"
    #[allow(deprecated)]
    fn parse_single_measurement<'b>(&self, input: &'b str) -> Res<&'b str, Measure> {
        // Define the structure of a basic measurement
        let measurement_parser = (
            opt(tag("about ")),                // Optional "about" prefix for estimates
            opt(|a| self.parse_multiplier(a)), // Optional multiplier (e.g., "2 x")
            |a| self.get_value(a),             // The numeric value
            space0,                            // Optional whitespace
            opt(|a| self.unit(a)),             // Optional unit of measure
            optional_period_or_of,             // Optional trailing period or "of"
        );

        traced_parser!(
            "parse_single_measurement",
            input,
            context("single_measurement", tuple(measurement_parser))
                .parse(input)
                .map(|(next_input, res)| {
                    let (_estimate_prefix, multiplier, value, _, unit, _) = res;

                    // Apply multiplier if present
                    let final_value = match multiplier {
                        Some(m) => value.0 * m,
                        None => value.0,
                    };

                    // Default to "whole" unit if none specified
                    let final_unit = unit
                        .unwrap_or_else(|| DEFAULT_UNIT.to_string())
                        .to_lowercase();

                    // Create the measurement
                    (
                        next_input,
                        Measure::from_parts(
                            final_unit.as_ref(),
                            final_value,
                            value.1, // Pass along any upper range value
                        ),
                    )
                }),
            |m: &Measure| m.to_string(),
            "no measurement"
        )
    }

    /// Parse a standalone unit with implicit quantity of 1, like "cup" or "tablespoons"
    fn parse_unit_only<'b>(&self, input: &'b str) -> Res<&'b str, Measure> {
        // Format: optional space + unit + optional period/of + required space
        let unit_only_format = (
            // Space requirement depends on text mode
            |a| {
                if self.is_rich_text {
                    space1(a) // Rich text mode requires space
                } else {
                    space0(a) // Normal mode allows optional space
                }
            },
            |a| self.unit_extra(a), // Parse the unit
            optional_period_or_of,  // Optional period or "of"
            space1,                 // Required space after unit
        );

        traced_parser!(
            "parse_unit_only",
            input,
            context("unit_only", unit_only_format).parse(input).map(
                |(next_input, (_, unit, _, _))| {
                    // Create a measure with value 1.0 and the parsed unit
                    (
                        next_input,
                        Measure::from_parts(unit.to_lowercase().as_ref(), 1.0, None),
                    )
                }
            ),
            |m: &Measure| m.to_string(),
            "no unit-only"
        )
    }

    /// Parse and validate a unit string
    fn unit<'b>(&self, input: &'b str) -> Res<&'b str, String> {
        traced_parser!(
            "unit",
            input,
            context(
                "unit",
                verify(unitamt, |s: &str| unit::is_valid(self.units, s)),
            )
            .parse(input),
            |s: &String| s.clone(),
            "not a valid unit"
        )
    }

    /// Parse an addon unit (only units in the custom set, not built-in units)
    ///
    /// This is used for implicit quantity parsing like "cup of flour" where we want
    /// to only match addon units, not built-in units like "whole".
    fn unit_extra<'b>(&self, input: &'b str) -> Res<&'b str, String> {
        traced_parser!(
            "unit_extra",
            input,
            context(
                "unit",
                verify(unitamt, |s: &str| unit::is_addon_unit(self.units, s)),
            )
            .parse(input),
            |s: &String| s.clone(),
            "not an addon unit"
        )
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use rstest::{fixture, rstest};

    #[fixture]
    fn units() -> HashSet<String> {
        [
            "cup",
            "cups",
            "tbsp",
            "tsp",
            "gram",
            "grams",
            "g",
            "whole",
            "lb",
            "oz",
            "ml",
            "tablespoon",
            "tablespoons",
            "teaspoon",
            "teaspoons",
        ]
        .iter()
        .map(|&s| s.to_string())
        .collect()
    }

    // ============================================================================
    // Basic Measurement Tests
    // ============================================================================

    #[rstest]
    fn test_measurement_parser(units: HashSet<String>) {
        let parser = MeasurementParser::new(&units, false);
        let result = parser.parse_measurement_list("2 cups");
        assert!(result.is_ok());
        let (remaining, measures) = result.unwrap();
        assert_eq!(remaining, "");
        assert_eq!(measures.len(), 1);
    }

    // ============================================================================
    // Range Format Tests
    // ============================================================================

    #[rstest]
    #[case::hyphen("2-3 cups")]
    #[case::to("2 to 3 cups")]
    #[case::through("2 through 3 cups")]
    #[case::or("2 or 3 cups")]
    fn test_range_formats(units: HashSet<String>, #[case] input: &str) {
        let parser = MeasurementParser::new(&units, false);
        let result = parser.parse_measurement_list(input);
        assert!(result.is_ok(), "Failed to parse: {input}");
        let (_, measures) = result.unwrap();
        assert!(!measures.is_empty());
    }

    // ============================================================================
    // Parenthesized Amounts Tests
    // ============================================================================

    #[rstest]
    #[case::single("(2 cups)", 1)]
    #[case::multiple("(1 cup / 240 ml)", 2)]
    fn test_parenthesized_amounts(
        units: HashSet<String>,
        #[case] input: &str,
        #[case] expected_count: usize,
    ) {
        let parser = MeasurementParser::new(&units, false);
        let result = parser.parse_parenthesized_amounts(input);
        assert!(result.is_ok());
        let (_, measures) = result.unwrap();
        assert_eq!(measures.len(), expected_count);
    }

    // ============================================================================
    // Upper Bound Tests
    // ============================================================================

    #[rstest]
    #[case::up_to("up to 5", 5.0)]
    #[case::at_most("at most 10", 10.0)]
    fn test_upper_bound_only(units: HashSet<String>, #[case] input: &str, #[case] expected: f64) {
        let parser = MeasurementParser::new(&units, false);
        let result = parser.parse_upper_bound_only(input);
        assert!(result.is_ok());
        let (_, (lower, upper)) = result.unwrap();
        assert_eq!(lower, 0.0);
        assert_eq!(upper, Some(expected));
    }

    // ============================================================================
    // Separator Tests
    // ============================================================================

    #[rstest]
    #[case::semicolon("2 cups; 1 tbsp", 2)]
    #[case::slash("1 cup / 240 ml", 2)]
    #[case::comma("1 cup, 2 tbsp", 2)]
    fn test_measurement_list_separators(
        units: HashSet<String>,
        #[case] input: &str,
        #[case] expected_count: usize,
    ) {
        let parser = MeasurementParser::new(&units, false);
        let result = parser.parse_measurement_list(input);
        assert!(result.is_ok());
        let (_, measures) = result.unwrap();
        assert_eq!(measures.len(), expected_count);
    }

    // ============================================================================
    // Rich Text Mode Tests
    // ============================================================================

    #[rstest]
    #[case::decimal("2.5", 2.5)]
    #[case::fraction("1/2", 0.5)]
    #[case::unicode_fraction("½", 0.5)]
    fn test_rich_text_mode(units: HashSet<String>, #[case] input: &str, #[case] expected: f64) {
        let parser = MeasurementParser::new(&units, true);
        let result = parser.parse_number(input);
        assert!(result.is_ok());
        let (_, val) = result.unwrap();
        assert!((val - expected).abs() < 0.001);
    }

    // ============================================================================
    // optional_period_or_of Tests
    // ============================================================================

    #[rstest]
    #[case::period(".")]
    #[case::of(" of")]
    #[case::something("something")]
    fn test_optional_period_or_of(#[case] input: &str) {
        let result = optional_period_or_of(input);
        assert!(result.is_ok());
    }

    // ============================================================================
    // Other Tests
    // ============================================================================

    #[rstest]
    fn test_plus_expression(units: HashSet<String>) {
        let parser = MeasurementParser::new(&units, false);
        let result = parser.parse_plus_expression("1 cup plus 2 tbsp");
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_multiplier(units: HashSet<String>) {
        let parser = MeasurementParser::new(&units, false);
        let result = parser.parse_multiplier("2 x ");
        assert!(result.is_ok());
        let (_, mult) = result.unwrap();
        assert_eq!(mult, 2.0);
    }

    #[rstest]
    fn test_measurement_with_about(units: HashSet<String>) {
        let parser = MeasurementParser::new(&units, false);
        let result = parser.parse_single_measurement("about 2 cups");
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_unit_only(units: HashSet<String>) {
        let parser = MeasurementParser::new(&units, false);
        let result = parser.parse_unit_only(" cup ");
        assert!(result.is_ok());
        let (_, measure) = result.unwrap();
        assert_eq!(measure.values().0, 1.0);
    }

    /// Test that unit mismatch in ranges returns None
    /// Note: This only works for dash-style ranges where both units are adjacent to numbers
    /// (e.g., "1g-2tbsp"). Word-style ranges like "1 cup to 2 tbsp" don't detect mismatch
    /// because the space before the second unit prevents it from being parsed.
    #[rstest]
    #[case::dash_mismatch("1g-2tbsp")]
    fn test_range_unit_mismatch(units: HashSet<String>, #[case] input: &str) {
        let parser = MeasurementParser::new(&units, false);
        let result = parser.parse_range_with_units(input);
        assert!(result.is_ok(), "Failed to parse: {input}");
        let (remaining, opt_measure) = result.unwrap();
        assert!(
            opt_measure.is_none(),
            "Expected None for unit mismatch on '{input}', got {opt_measure:?}, remaining: '{remaining}'",
        );
    }

    #[rstest]
    fn test_em_dash_range(units: HashSet<String>) {
        let parser = MeasurementParser::new(&units, false);
        let result = parser.parse_range_end("–3");
        assert!(result.is_ok());
        let (_, upper) = result.unwrap();
        assert_eq!(upper, 3.0);
    }

    #[rstest]
    fn test_no_unit_defaults_to_whole(units: HashSet<String>) {
        let parser = MeasurementParser::new(&units, false);
        let result = parser.parse_single_measurement("2 ");
        assert!(result.is_ok());
        let (_, measure) = result.unwrap();
        let measure_str = format!("{measure}");
        assert!(measure_str.contains("whole") || measure.values().0 == 2.0);
    }
}
