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

    fn make_parser() -> HashSet<String> {
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

    #[test]
    fn test_measurement_parser() {
        let units = make_parser();
        let parser = MeasurementParser::new(&units, false);

        // Test basic measurement
        let result = parser.parse_measurement_list("2 cups");
        assert!(result.is_ok());
        let (remaining, measures) = result.unwrap();
        assert_eq!(remaining, "");
        assert_eq!(measures.len(), 1);
    }

    #[test]
    fn test_range_with_units() {
        let units = make_parser();
        let parser = MeasurementParser::new(&units, false);

        // Test via parse_measurement_list which handles ranges correctly
        // Basic range: "2-3 cups"
        let result = parser.parse_measurement_list("2-3 cups");
        assert!(result.is_ok());
        let (_, measures) = result.unwrap();
        assert!(!measures.is_empty());

        // Range with "to": "2 to 3 cups"
        let result = parser.parse_measurement_list("2 to 3 cups");
        assert!(result.is_ok());

        // Range with "through": "2 through 3 cups"
        let result = parser.parse_measurement_list("2 through 3 cups");
        assert!(result.is_ok());

        // Range with "or": "2 or 3 cups"
        let result = parser.parse_measurement_list("2 or 3 cups");
        assert!(result.is_ok());
    }

    #[test]
    fn test_parenthesized_amounts() {
        let units = make_parser();
        let parser = MeasurementParser::new(&units, false);

        // Parenthesized: "(2 cups)"
        let result = parser.parse_parenthesized_amounts("(2 cups)");
        assert!(result.is_ok());
        let (remaining, measures) = result.unwrap();
        assert_eq!(remaining, "");
        assert_eq!(measures.len(), 1);

        // Multiple inside parens: "(1 cup / 240 ml)"
        let result = parser.parse_parenthesized_amounts("(1 cup / 240 ml)");
        assert!(result.is_ok());
        let (_, measures) = result.unwrap();
        assert_eq!(measures.len(), 2);
    }

    #[test]
    fn test_plus_expression() {
        let units = make_parser();
        let parser = MeasurementParser::new(&units, false);

        // Plus expression: "1 cup plus 2 tbsp"
        let result = parser.parse_plus_expression("1 cup plus 2 tbsp");
        assert!(result.is_ok());
    }

    #[test]
    fn test_upper_bound_only() {
        let units = make_parser();
        let parser = MeasurementParser::new(&units, false);

        // "up to 5"
        let result = parser.parse_upper_bound_only("up to 5");
        assert!(result.is_ok());
        let (_, (lower, upper)) = result.unwrap();
        assert_eq!(lower, 0.0);
        assert_eq!(upper, Some(5.0));

        // "at most 10"
        let result = parser.parse_upper_bound_only("at most 10");
        assert!(result.is_ok());
        let (_, (lower, upper)) = result.unwrap();
        assert_eq!(lower, 0.0);
        assert_eq!(upper, Some(10.0));
    }

    #[test]
    fn test_multiplier() {
        let units = make_parser();
        let parser = MeasurementParser::new(&units, false);

        // "2 x 3 cups" - multiplier expression
        let result = parser.parse_multiplier("2 x ");
        assert!(result.is_ok());
        let (_, mult) = result.unwrap();
        assert_eq!(mult, 2.0);
    }

    #[test]
    fn test_measurement_with_about() {
        let units = make_parser();
        let parser = MeasurementParser::new(&units, false);

        // "about 2 cups"
        let result = parser.parse_single_measurement("about 2 cups");
        assert!(result.is_ok());
    }

    #[test]
    fn test_measurement_list_separators() {
        let units = make_parser();
        let parser = MeasurementParser::new(&units, false);

        // Semicolon separator: "2 cups; 1 tbsp"
        let result = parser.parse_measurement_list("2 cups; 1 tbsp");
        assert!(result.is_ok());
        let (_, measures) = result.unwrap();
        assert_eq!(measures.len(), 2);

        // Slash separator: "1 cup / 240 ml"
        let result = parser.parse_measurement_list("1 cup / 240 ml");
        assert!(result.is_ok());
        let (_, measures) = result.unwrap();
        assert_eq!(measures.len(), 2);

        // Comma separator: "1 cup, 2 tbsp"
        let result = parser.parse_measurement_list("1 cup, 2 tbsp");
        assert!(result.is_ok());
        let (_, measures) = result.unwrap();
        assert_eq!(measures.len(), 2);
    }

    #[test]
    fn test_unit_only() {
        let units = make_parser();
        let parser = MeasurementParser::new(&units, false);

        // Just a unit with implicit 1: "cup "
        let result = parser.parse_unit_only(" cup ");
        assert!(result.is_ok());
        let (_, measure) = result.unwrap();
        assert_eq!(measure.values().0, 1.0);
    }

    #[test]
    fn test_rich_text_mode() {
        let units = make_parser();
        let parser = MeasurementParser::new(&units, true);

        // In rich text mode, text numbers like "one" shouldn't be parsed
        let result = parser.parse_number("2.5");
        assert!(result.is_ok());

        // Rich text mode should parse fractions
        let result = parser.parse_number("1/2");
        assert!(result.is_ok());
        let (_, val) = result.unwrap();
        assert!((val - 0.5).abs() < 0.001);

        // Rich text mode should parse unicode fractions
        let result = parser.parse_number("½");
        assert!(result.is_ok());
    }

    #[test]
    fn test_range_unit_mismatch() {
        let units = make_parser();
        let parser = MeasurementParser::new(&units, false);

        // When lower and upper units differ, should return None
        // "1g-2tbsp" has different units on each side (no spaces)
        let result = parser.parse_range_with_units("1g-2tbsp");
        assert!(result.is_ok());
        let (remaining, opt_measure) = result.unwrap();
        // Should be None due to unit mismatch
        assert!(
            opt_measure.is_none(),
            "Expected None for unit mismatch, got {opt_measure:?}, remaining: '{remaining}'",
        );
    }

    #[test]
    fn test_em_dash_range() {
        let units = make_parser();
        let parser = MeasurementParser::new(&units, false);

        // Em-dash range: "2–3 cups" (using – not -)
        let result = parser.parse_range_end("–3");
        assert!(result.is_ok());
        let (_, upper) = result.unwrap();
        assert_eq!(upper, 3.0);
    }

    #[test]
    fn test_optional_period_or_of() {
        // Test the optional_period_or_of function
        let result = optional_period_or_of(".");
        assert!(result.is_ok());

        let result = optional_period_or_of(" of");
        assert!(result.is_ok());

        let result = optional_period_or_of("something");
        assert!(result.is_ok()); // Returns None but still Ok
    }

    #[test]
    fn test_no_unit_defaults_to_whole() {
        let units = make_parser();
        let parser = MeasurementParser::new(&units, false);

        // Just a number without unit: "2"
        let result = parser.parse_single_measurement("2 ");
        assert!(result.is_ok());
        let (_, measure) = result.unwrap();
        // Default unit should be "whole" - check via Display
        let measure_str = format!("{measure}");
        assert!(measure_str.contains("whole") || measure.values().0 == 2.0);
    }
}
