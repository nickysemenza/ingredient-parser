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

use nom::error::ParseError;
use nom_language::error::VerboseError;

use crate::parser::{parse_unit_text, Res};

/// Consume an optional em-dash or en-dash separator between amount and unit.
/// Some cookbooks use formats like "3–4 — tablespoons" where there's an extra
/// dash between the range and the unit.
fn optional_dash_separator(input: &str) -> Res<&str, Option<&str>> {
    opt(alt((
        tag("— "), // em-dash with trailing space
        tag("– "), // en-dash with trailing space
        tag("—"),  // bare em-dash
        tag("–"),  // bare en-dash
    )))
    .parse(input)
}
use crate::traced_parser;
use crate::unit::{self, Measure};

/// Default unit for amounts without a specified unit (e.g., "2 eggs")
const DEFAULT_UNIT: &str = "whole";

/// Distance unit base forms for dimension detection.
/// These are the canonical forms of distance units (singular, no hyphen).
const DISTANCE_UNIT_BASES: &[&str] = &[
    "inch",
    "in",
    "cm",
    "centimeter",
    "centimetre",
    "mm",
    "millimeter",
    "millimetre",
    "foot",
    "ft",
    "meter",
    "metre",
    "m",
    "yard",
    "yd",
];

/// Check if a string is a distance unit (used for dimension detection).
/// Handles both singular and plural forms automatically.
fn is_distance_unit(s: &str) -> bool {
    let lower = s.to_lowercase();

    // Check exact match
    for base in DISTANCE_UNIT_BASES {
        if lower == *base {
            return true;
        }
    }

    // Check common plural forms
    // Try removing "s" suffix
    if lower.ends_with('s') {
        let without_s = &lower[..lower.len() - 1];
        for base in DISTANCE_UNIT_BASES {
            if without_s == *base {
                return true;
            }
        }
        // Try removing "es" suffix (for "inches", etc.)
        if lower.ends_with("es") && lower.len() > 2 {
            let without_es = &lower[..lower.len() - 2];
            for base in DISTANCE_UNIT_BASES {
                if without_es == *base {
                    return true;
                }
            }
        }
    }

    false
}

/// Check if text starts with a dimension suffix (e.g., "-inch", "-cm", "-inches")
///
/// A dimension suffix is a hyphen followed by a distance unit.
/// For example, "1-inch" in "1-inch piece ginger" should not be parsed as quantity=1.
fn starts_with_dimension_suffix(text: &str) -> bool {
    let text = text.to_lowercase();
    if !text.starts_with('-') {
        return false;
    }

    // Extract the potential unit after the hyphen
    let after_hyphen = &text[1..];
    // Take alphanumeric chars until we hit a space, hyphen, or other delimiter
    let unit_part: String = after_hyphen
        .chars()
        .take_while(|c| c.is_alphabetic())
        .collect();

    if unit_part.is_empty() {
        return false;
    }

    is_distance_unit(&unit_part)
}

/// Parse optional trailing period or " of" after units (e.g., "tsp." or "cup of")
/// Also consumes a trailing space after a period (for sentence breaks like "375. Next")
fn optional_period_or_of(input: &str) -> Res<&str, Option<&str>> {
    opt(alt((tag(". "), tag("."), tag(" of")))).parse(input)
}

/// Check if a bare number looks like a step number in instructions.
///
/// Returns true if the remaining input starts with whitespace followed by
/// a capitalized word (likely an instruction verb like "Bring", "Set", "Add").
/// This helps avoid parsing step numbers as measurements (e.g., "1 Bring a pot").
fn looks_like_step_number(input: &str) -> bool {
    let trimmed = input.trim_start();
    if trimmed.is_empty() {
        return false;
    }
    // Check if first char is uppercase letter
    let first_char = trimmed.chars().next().unwrap_or(' ');
    if !first_char.is_ascii_uppercase() {
        return false;
    }
    // Check if the first word is more than 2 characters (not just an initial)
    // and is alphabetic (instruction verbs)
    let first_word: String = trimmed.chars().take_while(|c| c.is_alphabetic()).collect();
    first_word.len() >= 2
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
    /// - "150 grams | 1 cup" (Bouchon format: metric | volume)
    /// - "1 tsp, 2 tbsp"
    #[tracing::instrument(name = "many_amount", skip(self))]
    pub fn parse_measurement_list<'b>(&self, input: &'b str) -> Res<&'b str, Vec<Measure>> {
        // Define the separators between measurements
        let amount_separators = alt((
            tag("; "),  // semicolon with space
            tag(" / "), // slash with spaces
            tag(" | "), // pipe with spaces (Bouchon format: metric | volume)
            tag(" × "), // multiplication sign with spaces (UK format: "1 × 400g tin")
            tag("× "),  // multiplication sign when leading space was consumed
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
    ///
    /// Also handles format: "4 (13-millimeter/½-inch) slices" where a parenthesized
    /// description appears between the number and unit.
    #[allow(deprecated)]
    fn parse_single_measurement<'b>(&self, input: &'b str) -> Res<&'b str, Measure> {
        // Define the structure of a basic measurement
        let measurement_parser = (
            opt(tag("about ")),                // Optional "about" prefix for estimates
            opt(|a| self.parse_multiplier(a)), // Optional multiplier (e.g., "2 x")
            |a| self.parse_value(a),           // The numeric value
            space0,                            // Optional whitespace
            optional_dash_separator,           // Handle "3–4 — tablespoons" format
            opt(|a| self.unit(a)),             // Optional unit of measure
            optional_period_or_of,             // Optional trailing period or "of"
        );

        traced_parser!(
            "parse_single_measurement",
            input,
            context("single_measurement", tuple(measurement_parser))
                .parse(input)
                .and_then(|(next_input, res)| {
                    let (_estimate_prefix, multiplier, value, _, _dash, unit, period_consumed) =
                        res;

                    // Apply multiplier if present
                    let final_value = match multiplier {
                        Some(m) => value.0 * m,
                        None => value.0,
                    };

                    // If no unit was found, check if there's a parenthesized description
                    // followed by a unit, like "4 (13-millimeter/½-inch) slices"
                    let (final_next_input, final_unit) = if unit.is_none() {
                        if let Some((after_paren, found_unit)) =
                            self.parse_unit_after_parens(next_input)
                        {
                            // Found a unit after parentheses - use it
                            (after_paren, found_unit)
                        } else if self.is_rich_text
                            && period_consumed.is_none()
                            && looks_like_step_number(next_input)
                        {
                            // In rich text mode, don't parse bare numbers followed by
                            // capitalized words as measurements (e.g., "1 Bring a pot...")
                            // These are likely step numbers, not quantities.
                            // BUT: if a period was consumed (like "375. Combine"), this is
                            // a sentence break, not a step number pattern.
                            return Err(nom::Err::Error(VerboseError::from_error_kind(
                                input,
                                nom::error::ErrorKind::Verify,
                            )));
                        } else if starts_with_dimension_suffix(next_input) {
                            // Don't parse "1-inch" as "1 whole" - the number is part of
                            // a dimension descriptor like "1-inch piece ginger"
                            return Err(nom::Err::Error(VerboseError::from_error_kind(
                                input,
                                nom::error::ErrorKind::Verify,
                            )));
                        } else {
                            // No unit found, default to "whole"
                            (next_input, DEFAULT_UNIT.to_string())
                        }
                    } else if let Some(u) = unit {
                        (next_input, u.to_lowercase())
                    } else {
                        unreachable!("unit.is_none() was false but unit is None")
                    };

                    // Create the measurement
                    Ok((
                        final_next_input,
                        Measure::from_parts(
                            final_unit.as_ref(),
                            final_value,
                            value.1, // Pass along any upper range value
                        ),
                    ))
                }),
            |m: &Measure| m.to_string(),
            "no measurement"
        )
    }

    /// Try to find a unit after skipping a parenthesized description.
    ///
    /// For input like "(13-millimeter/½-inch) slices CHASHU", this skips the
    /// parentheses and returns ("CHASHU", "slices").
    ///
    /// Returns Some((remaining, unit)) if successful, None otherwise.
    fn parse_unit_after_parens<'b>(&self, input: &'b str) -> Option<(&'b str, String)> {
        let input = input.trim_start();

        // Must start with '('
        if !input.starts_with('(') {
            return None;
        }

        // Find matching closing parenthesis
        let mut depth = 0;
        let mut close_pos = None;
        for (i, c) in input.char_indices() {
            match c {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        close_pos = Some(i);
                        break;
                    }
                }
                _ => {}
            }
        }

        let close_pos = close_pos?;

        // Get the text after the closing paren
        let after_paren = &input[close_pos + 1..];
        let after_paren = after_paren.trim_start();

        // Try to parse a unit
        if let Ok((remaining, unit)) = self.unit(after_paren) {
            // Also consume optional period or " of"
            let remaining = if let Some(stripped) = remaining.strip_prefix('.') {
                stripped
            } else if let Some(stripped) = remaining.strip_prefix(" of") {
                stripped
            } else {
                remaining
            };
            Some((remaining, unit.to_lowercase()))
        } else {
            None
        }
    }

    /// Parse a standalone unit with implicit quantity of 1, like "cup" or "tablespoons"
    ///
    /// This is disabled in rich text mode to prevent false positives like
    /// "bullet-proof recipe" being parsed as "1 recipe". In prose, measurements
    /// should always have explicit numbers.
    fn parse_unit_only<'b>(&self, input: &'b str) -> Res<&'b str, Measure> {
        // In rich text mode, don't allow implicit quantity parsing
        // Prose like "bullet-proof recipe" shouldn't become "1 recipe"
        if self.is_rich_text {
            return Err(nom::Err::Error(VerboseError::from_error_kind(
                input,
                nom::error::ErrorKind::Verify,
            )));
        }

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

    /// Parse and validate a unit string using the given predicate
    fn parse_unit_with<'b>(
        &self,
        input: &'b str,
        predicate: impl Fn(&str) -> bool,
        name: &'static str,
        err_msg: &'static str,
    ) -> Res<&'b str, String> {
        traced_parser!(
            name,
            input,
            context("unit", verify(parse_unit_text, |s: &str| predicate(s)),).parse(input),
            |s: &String| s.clone(),
            err_msg
        )
    }

    /// Parse and validate a unit string
    fn unit<'b>(&self, input: &'b str) -> Res<&'b str, String> {
        self.parse_unit_with(
            input,
            |s| unit::is_valid(self.units, s),
            "unit",
            "not a valid unit",
        )
    }

    /// Parse an addon unit (only units in the custom set, not built-in units)
    ///
    /// This is used for implicit quantity parsing like "cup of flour" where we want
    /// to only match addon units, not built-in units like "whole".
    fn unit_extra<'b>(&self, input: &'b str) -> Res<&'b str, String> {
        self.parse_unit_with(
            input,
            |s| unit::is_addon_unit(self.units, s),
            "unit_extra",
            "not an addon unit",
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
    #[case::pipe("150 grams | 1 cup", 2)]
    #[case::multiplication_sign("1 × 400 grams", 2)]
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
    #[case::word("1 cup plus 2 tbsp")]
    #[case::symbol("½ cup + 2 tbsp")]
    fn test_plus_expression(units: HashSet<String>, #[case] input: &str) {
        let parser = MeasurementParser::new(&units, false);
        assert!(parser.parse_plus_expression(input).is_ok());
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
        assert_eq!(measure.value(), 1.0);
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
        assert!(measure_str.contains("whole") || measure.value() == 2.0);
    }

    // ============================================================================
    // Dimension Suffix Detection Tests
    // ============================================================================

    #[rstest]
    #[case::inch("-inch", true, "basic inch")]
    #[case::inches("-inches", true, "plural inches")]
    #[case::cm("-cm", true, "basic cm")]
    #[case::centimeter("-centimeter", true, "full centimeter")]
    #[case::centimeters("-centimeters", true, "plural centimeters")]
    #[case::mm("-mm", true, "basic mm")]
    #[case::foot("-foot", true, "basic foot")]
    #[case::feet("-feet", false, "irregular plural feet (not detected)")] // feet is irregular, not handled
    #[case::meter("-meter", true, "basic meter")]
    #[case::meters("-meters", true, "plural meters")]
    #[case::inch_piece("-inch piece", true, "inch with trailing text")]
    #[case::not_dimension("-ish", false, "not a dimension")]
    #[case::empty("-", false, "just hyphen")]
    #[case::no_hyphen("inch", false, "no leading hyphen")]
    #[case::yard("-yard", true, "basic yard")]
    #[case::yards("-yards", true, "plural yards")]
    fn test_dimension_suffix_detection(
        #[case] input: &str,
        #[case] expected: bool,
        #[case] _desc: &str,
    ) {
        assert_eq!(
            starts_with_dimension_suffix(input),
            expected,
            "Failed for input: {input}"
        );
    }

    #[rstest]
    #[case::inch("inch", true)]
    #[case::inches("inches", true)]
    #[case::cm("cm", true)]
    #[case::mm("mm", true)]
    #[case::foot("foot", true)]
    #[case::ft("ft", true)]
    #[case::meter("meter", true)]
    #[case::meters("meters", true)]
    #[case::cup("cup", false)]
    #[case::tsp("tsp", false)]
    fn test_is_distance_unit(#[case] input: &str, #[case] expected: bool) {
        assert_eq!(is_distance_unit(input), expected, "Failed for: {input}");
    }
}
