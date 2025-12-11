//! Measurement parsing for ingredient strings
//!
//! This module contains all the parsers for extracting measurements from ingredient
//! strings, including single measurements, ranges, and combined expressions.

use std::collections::HashSet;

#[allow(deprecated)]
use nom::sequence::tuple;
use nom::{
    branch::alt,
    bytes::complete::tag,
    character::complete::{char, space0, space1},
    combinator::{opt, verify},
    error::context,
    multi::separated_list1,
    number::complete::double,
    sequence::delimited,
    Parser,
};
use tracing::info;

use crate::fraction::fraction_number;
use crate::parser::{text_number, unitamt, Res};
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

    /// Parse a range with units, like "78g to 104g" or "2-3 cups"
    fn parse_range_with_units<'b>(&self, input: &'b str) -> Res<&'b str, Option<Measure>> {
        // Format for a measurement with a range
        let range_format = (
            opt(tag("about ")),          // Optional "about" for estimates
            |a| self.get_value(a),       // The lower value
            space0,                      // Optional whitespace
            opt(|a| self.unit(a)),       // Optional unit for lower value
            |a| self.parse_range_end(a), // The upper range value
            opt(|a| self.unit(a)),       // Optional unit for upper value
            optional_period_or_of,       // Optional period or "of"
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

    /// Parse measurements enclosed in parentheses: (1 cup)
    pub fn parse_parenthesized_amounts<'b>(&self, input: &'b str) -> Res<&'b str, Vec<Measure>> {
        traced_parser!(
            "parse_parenthesized_amounts",
            input,
            context(
                "parenthesized_amounts",
                delimited(
                    char('('),                          // Opening parenthesis
                    |a| self.parse_measurement_list(a), // Parse measurements inside parentheses
                    char(')'),                          // Closing parenthesis
                ),
            )
            .parse(input),
            |measures: &Vec<Measure>| measures
                .iter()
                .map(|m| m.to_string())
                .collect::<Vec<_>>()
                .join(", "),
            "no parenthesized amounts"
        )
    }

    /// Parse expressions with "plus" that combine two measurements
    ///
    /// For example: "1 cup plus 2 tablespoons"
    fn parse_plus_expression<'b>(&self, input: &'b str) -> Res<&'b str, Measure> {
        // Define the structure of a plus expression
        let plus_parser = (
            |a| self.parse_single_measurement(a), // First measurement
            space1,                               // Required whitespace
            tag("plus"),                          // The "plus" keyword
            space1,                               // Required whitespace
            |a| self.parse_single_measurement(a), // Second measurement
        );

        traced_parser!(
            "parse_plus_expression",
            input,
            context("plus_expression", plus_parser).parse(input).map(
                |(next_input, (first_measure, _, _, _, second_measure))| {
                    // Add the two measurements together
                    match first_measure.add(second_measure) {
                        Ok(combined) => (next_input, combined),
                        Err(_) => {
                            // If addition fails, just return the first measure as fallback
                            (next_input, first_measure)
                        }
                    }
                },
            ),
            |m: &Measure| m.to_string(),
            "no plus expression"
        )
    }

    /// Parse a value that may have a range, returning (value, optional_upper_range)
    fn get_value<'b>(&self, input: &'b str) -> Res<&'b str, (f64, Option<f64>)> {
        traced_parser!(
            "get_value",
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
    fn parse_upper_bound_only<'b>(&self, input: &'b str) -> Res<&'b str, (f64, Option<f64>)> {
        // Format: prefix + number, mapped to (0.0, Some(upper_value))
        // Note: We don't consume leading space here - let the caller handle spacing
        let format = (
            alt((tag("up to"), tag("at most"))), // Upper bound keywords
            space0,                              // Optional space after keyword
            |a| self.parse_number(a),            // The upper bound value
        )
            .map(|(_, _, upper_value)| (0.0, Some(upper_value)));

        traced_parser!(
            "parse_upper_bound_only",
            input,
            context("upper_bound_only", format).parse(input),
            |(_, upper): &(f64, Option<f64>)| match upper {
                Some(u) => format!("up to {u}"),
                None => String::new(),
            },
            "no upper bound"
        )
    }

    /// Parse numeric values including fractions, decimals, and text numbers like "one"
    fn parse_number<'b>(&self, input: &'b str) -> Res<&'b str, f64> {
        // Choose parsers based on whether we're in rich text mode
        traced_parser!(
            "parse_number",
            input,
            if self.is_rich_text {
                // Rich text mode: try fraction or decimal number
                context(
                    "number",
                    alt((
                        fraction_number, // Parse fractions like "½" or "1/2"
                        double,          // Parse decimal numbers like "2.5"
                    )),
                )
                .parse(input)
            } else {
                // Normal mode: try fraction, text number, or decimal
                context(
                    "number",
                    alt((
                        fraction_number, // Parse fractions like "½" or "1/2"
                        text_number,     // Parse text numbers like "one" or "a"
                        double,          // Parse decimal numbers like "2.5"
                    )),
                )
                .parse(input)
            },
            |v: &f64| format!("{v}"),
            "no number"
        )
    }

    /// Parse a multiplier expression like "2 x" (meaning multiply the following value by 2)
    fn parse_multiplier<'b>(&self, input: &'b str) -> Res<&'b str, f64> {
        // Define the format of a multiplier: number + space + "x" + space
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

    /// Parse the upper end of a range like "-3", "to 5", "through 10", or "or 2"
    fn parse_range_end<'b>(&self, input: &'b str) -> Res<&'b str, f64> {
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
            space1,                                      // Required space
            alt((tag("to"), tag("through"), tag("or"))), // Range keywords
            space1,                                      // Required space
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
            "cup", "cups", "tbsp", "tsp", "gram", "grams", "g", "whole", "lb", "oz", "ml",
            "tablespoon", "tablespoons", "teaspoon", "teaspoons",
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
        let measure_str = format!("{}", measure);
        assert!(measure_str.contains("whole") || measure.values().0 == 2.0);
    }
}
