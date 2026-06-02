//! Measurement parsing for ingredient strings
//!
//! This module contains all the parsers for extracting measurements from ingredient
//! strings, including single measurements, ranges, and combined expressions.
//!
//! ## Rich-text mode (`is_rich_text`)
//!
//! The same parsers serve two modes, selected by the `is_rich_text` flag on
//! [`MeasurementParser`]: **ingredient-list** mode (the default — "2 cups flour")
//! and **rich-text/prose** mode (measurements embedded in instructions — "cook for
//! 30 minutes"). The modes share ~90% of the logic; prose mode only adds a few
//! *rejections* so noise isn't mistaken for a quantity. Every fork point:
//!
//! - `number::parse_number` — prose mode excludes spelled-out text numbers
//!   ("one", "a") so words like "a pinch" or "one more" aren't read as counts.
//! - `single::rejected_in_rich_text` — prose mode rejects step numbers
//!   ("1. Bring…") and dimension suffixes ("1-inch piece"). See that method.
//! - `single::parse_unit_only` — disabled entirely in prose (a bare unit like
//!   "cup" in prose is a noun, not "1 cup"); only fires in ingredient-list mode.
//!
//! (Secondary-amount extraction in `refine` deliberately parses its parenthetical
//! in ingredient-list mode regardless — "(about 2 cups)" is always a quantity.)

mod composite;
pub(crate) mod guards;
mod number;
mod range;
pub(crate) mod single;

use std::collections::HashSet;

use nom::{branch::alt, bytes::complete::tag, error::context, multi::separated_list1, Parser};

use crate::parser::Res;
use crate::traced_parser;
use crate::unit::Measure;

use self::guards::optional_period_or_of;

/// Shared test fixture data for the measurement submodules' co-located tests.
#[cfg(test)]
pub(crate) mod test_support {
    use std::collections::HashSet;

    /// The unit set used across measurement tests.
    pub(crate) fn units() -> HashSet<String> {
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
            "slice",
            "slices",
            "ounce",
            "ounces",
            "piece",
            "pieces",
            "can",
            "cans",
        ]
        .iter()
        .map(|&s| s.to_string())
        .collect()
    }
}

/// Default unit for amounts without a specified unit (e.g., "2 eggs")
pub(super) const DEFAULT_UNIT: &str = "whole";

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
    /// - "150 grams | 1 cup" (Bouchon format: metric | volume)
    /// - "1 tsp, 2 tbsp"
    #[tracing::instrument(name = "many_amount", skip(self))]
    pub fn parse_measurement_list<'b>(&self, input: &'b str) -> Res<&'b str, Vec<Measure>> {
        // Define the separators between measurements
        let amount_separators = alt((
            tag("; "),  // semicolon with space
            tag(" / "), // slash with spaces
            tag(" /"),  // slash, space before only ("175 grams /1¾ cups")
            tag("/ "),  // slash, space after only
            tag(" | "), // pipe with spaces (Bouchon format: metric | volume)
            tag(" × "), // multiplication sign with spaces (UK format: "1 × 400g tin")
            tag("× "),  // multiplication sign when leading space was consumed
            tag("/"),   // bare slash
            tag(", "),  // comma with space
            tag(" "),   // just a space
        ));

        // Define the different types of measurements we can parse
        let amount_parsers = alt((
            // "1 cup plus 2 tbsp" -> sums compatible measures (else keeps both)
            |input| self.parse_plus_expression(input),
            // Cross-unit range "2 tsp to 2 tbsp" -> [2 tsp, 2 tbsp] (two amounts,
            // since differing units can't fold into one range Measure). Must come
            // before the same-unit range parser, which would otherwise swallow the
            // first unit and drop the second.
            |input| self.parse_cross_unit_range(input),
            // Range with units on both sides: "2-3 cups" or "1 to 2 tbsp"
            |input| {
                self.parse_range_with_units(input)
                    .map(|(next, opt_measure)| {
                        (next, opt_measure.map_or_else(Vec::new, |m| vec![m]))
                    })
            },
            // "1 (1-ounce) piece" -> [1 piece, 1 oz] (hoist hyphenated size)
            |input| self.parse_count_with_parenthetical_size(input),
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
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::test_support::units;
    use super::*;
    use rstest::{fixture, rstest};

    #[fixture]
    fn units_fx() -> HashSet<String> {
        units()
    }

    #[rstest]
    fn test_measurement_parser(units_fx: HashSet<String>) {
        let parser = MeasurementParser::new(&units_fx, false);
        let result = parser.parse_measurement_list("2 cups");
        assert!(result.is_ok());
        let (remaining, measures) = result.unwrap();
        assert_eq!(remaining, "");
        assert_eq!(measures.len(), 1);
    }

    #[rstest]
    #[case::hyphen("2-3 cups")]
    #[case::to("2 to 3 cups")]
    #[case::through("2 through 3 cups")]
    #[case::or("2 or 3 cups")]
    fn test_range_formats(units_fx: HashSet<String>, #[case] input: &str) {
        let parser = MeasurementParser::new(&units_fx, false);
        let result = parser.parse_measurement_list(input);
        assert!(result.is_ok(), "Failed to parse: {input}");
        let (_, measures) = result.unwrap();
        assert!(!measures.is_empty());
    }

    #[rstest]
    #[case::semicolon("2 cups; 1 tbsp", 2)]
    #[case::slash("1 cup / 240 ml", 2)]
    #[case::comma("1 cup, 2 tbsp", 2)]
    #[case::pipe("150 grams | 1 cup", 2)]
    #[case::multiplication_sign("1 × 400 grams", 2)]
    fn test_measurement_list_separators(
        units_fx: HashSet<String>,
        #[case] input: &str,
        #[case] expected_count: usize,
    ) {
        let parser = MeasurementParser::new(&units_fx, false);
        let result = parser.parse_measurement_list(input);
        assert!(result.is_ok());
        let (_, measures) = result.unwrap();
        assert_eq!(measures.len(), expected_count);
    }

    #[rstest]
    #[case::step_number("1 Bring a pot of water to a boil.")]
    #[case::numbered_instruction("2 Set out 4 ramen bowls.")]
    fn test_step_numbers_not_parsed_as_measurements(
        units_fx: HashSet<String>,
        #[case] input: &str,
    ) {
        let parser = MeasurementParser::new(&units_fx, true);
        assert!(parser.parse_measurement_list(input).is_err());
    }
}
