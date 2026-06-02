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
#[cfg(test)]
use self::guards::{is_distance_unit, starts_with_dimension_suffix};

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
    // Compatible kinds (volume + volume) are summed into a single measure.
    #[case::word("1 cup plus 2 tbsp", 1)]
    #[case::symbol("½ cup + 2 tbsp", 1)]
    // Incompatible kinds (volume + weight) keep both rather than dropping one.
    #[case::incompatible("1 cup plus 100 grams", 2)]
    fn test_plus_expression(
        units: HashSet<String>,
        #[case] input: &str,
        #[case] expected_len: usize,
    ) {
        let parser = MeasurementParser::new(&units, false);
        let (_, measures) = parser.parse_plus_expression(input).unwrap();
        assert_eq!(measures.len(), expected_len, "input: {input}");
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

    /// Leading approximation qualifiers (any case, optional article) are skipped
    /// so the amount after them still parses.
    #[rstest]
    #[case::lower_about("about 2 cups")]
    #[case::cap_about("About 2 cups")]
    #[case::generous("Generous 1 cup")]
    #[case::scant("Scant 1 cup")]
    #[case::heaping("Heaping 1 tablespoon")]
    #[case::article("A generous 1 cup")]
    fn test_leading_qualifiers(units: HashSet<String>, #[case] input: &str) {
        let parser = MeasurementParser::new(&units, false);
        let (_, measure) = parser.parse_single_measurement(input).unwrap();
        // The qualifier is discarded; the numeric value survives.
        assert!(measure.value() >= 1.0, "input: {input}");
    }

    /// A parenthetical size — hyphenated ("1-ounce") or space form ("14.5 oz") —
    /// is hoisted into a second measure while the count keeps the container unit:
    /// "1 (1-ounce) piece" -> [1 piece, 1 oz]; "2 (14.5 oz) cans" -> [2 can, 14.5 oz].
    #[rstest]
    #[case::piece("1 (1-ounce) piece ginger", 2, "piece")]
    #[case::can("1 (28-ounce) can tomatoes", 2, "can")]
    #[case::space_form("2 (14.5 oz) cans tomatoes", 2, "can")]
    fn test_count_with_parenthetical_size(
        units: HashSet<String>,
        #[case] input: &str,
        #[case] expected_len: usize,
        #[case] first_unit: &str,
    ) {
        let parser = MeasurementParser::new(&units, false);
        let (_, measures) = parser.parse_count_with_parenthetical_size(input).unwrap();
        assert_eq!(measures.len(), expected_len, "input: {input}");
        assert_eq!(measures[0].unit_as_string(), first_unit);
        assert_eq!(measures[1].unit_as_string(), "oz");
    }

    /// Count + parenthetical/hyphenated size with NO container noun: the count
    /// becomes a "whole" amount and the size a second amount, e.g.
    /// "1 (3 ounce) chicken" -> [1 whole, 3 oz] and "One 6-ounce carrot" ->
    /// [1 whole, 6 oz]. (With a container the first unit is the container; see
    /// the test above.)
    #[rstest]
    #[case::paren("1 (3 ounce) chicken")]
    #[case::hyphen("One 6-ounce carrot")]
    fn test_count_with_size_no_container(units: HashSet<String>, #[case] input: &str) {
        let parser = MeasurementParser::new(&units, false);
        let (_, measures) = parser.parse_count_with_parenthetical_size(input).unwrap();
        assert_eq!(measures.len(), 2, "input: {input}");
        assert_eq!(measures[0].unit_as_string(), "whole");
        assert_eq!(measures[1].unit_as_string(), "oz");
    }

    /// A cross-unit range "2 tsp to 2 tbsp" yields two separate amounts (it can't
    /// fold into one ranged Measure); a same-unit range falls through so the
    /// range parser keeps it as a single Measure with an upper bound.
    #[rstest]
    fn test_cross_unit_range(units: HashSet<String>) {
        let parser = MeasurementParser::new(&units, false);
        let (_, measures) = parser
            .parse_cross_unit_range("2 teaspoons to 2 tablespoons")
            .unwrap();
        assert_eq!(measures.len(), 2);
        assert_eq!(measures[0].unit_as_string(), "tsp");
        assert_eq!(measures[1].unit_as_string(), "tbsp");
        // Same unit on both sides → not a cross-unit range.
        assert!(parser.parse_cross_unit_range("2 cups to 3 cups").is_err());
    }

    /// A parenthetical that is NOT a size (no parseable measurement inside) is
    /// rejected even when a container noun follows, so it falls through to the
    /// plain parenthesized-amount / name paths.
    #[rstest]
    fn test_parenthetical_size_rejects_non_size(units: HashSet<String>) {
        let parser = MeasurementParser::new(&units, false);
        assert!(parser
            .parse_count_with_parenthetical_size("1 (not defrosted) can tomatoes")
            .is_err());
    }

    #[rstest]
    fn test_unit_only(units: HashSet<String>) {
        let parser = MeasurementParser::new(&units, false);
        let result = parser.parse_unit_only(" cup ");
        assert!(result.is_ok());
        let (_, measure) = result.unwrap();
        assert_eq!(measure.value(), 1.0);
    }

    #[rstest]
    fn test_unit_only_rejected_in_rich_text_mode(units: HashSet<String>) {
        let parser = MeasurementParser::new(&units, true);
        assert!(parser.parse_unit_only(" cup ").is_err());
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

    #[rstest]
    #[case::step_number("1 Bring a pot of water to a boil.")]
    #[case::numbered_instruction("2 Set out 4 ramen bowls.")]
    fn test_step_numbers_not_parsed_as_measurements(units: HashSet<String>, #[case] input: &str) {
        let parser = MeasurementParser::new(&units, true);
        assert!(parser.parse_measurement_list(input).is_err());
    }

    #[rstest]
    #[case::inch_piece("1-inch piece ginger")]
    #[case::cm_piece("2-cm knob ginger")]
    fn test_dimension_suffix_rejected(units: HashSet<String>, #[case] input: &str) {
        let parser = MeasurementParser::new(&units, true);
        assert!(parser.parse_single_measurement(input).is_err());
    }

    #[rstest]
    fn test_unit_after_parenthesized_description(units: HashSet<String>) {
        let parser = MeasurementParser::new(&units, false);
        let result = parser.parse_single_measurement("4 (13-millimeter/½-inch) slices CHASHU");
        assert!(result.is_ok());
        let (remaining, measure) = result.unwrap();
        assert_eq!(remaining, " CHASHU");
        assert_eq!(measure.value(), 4.0);
        assert_eq!(measure.unit_as_string(), "slice");
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
