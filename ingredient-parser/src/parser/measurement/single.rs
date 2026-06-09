use nom::error::ParseError;
use nom::{
    branch::alt,
    bytes::complete::tag_no_case,
    character::complete::{space0, space1},
    combinator::{opt, verify},
    error::context,
    Parser,
};
use nom_language::error::VerboseError;

use crate::parser::{parse_unit_text, Res};
use crate::traced_parser;
use crate::unit::{self, Measure};

use super::guards::{
    find_matching_paren, is_distance_unit, looks_like_step_number, optional_article,
    optional_dash_separator, optional_period_or_of, starts_with_dimension_suffix,
};
use super::{MeasurementParser, DEFAULT_UNIT};

impl<'a> MeasurementParser<'a> {
    /// Parse a single measurement like "2 cups" or "about 3 tablespoons".
    ///
    /// Also handles format: "4 (13-millimeter/½-inch) slices" where a parenthesized
    /// description appears between the number and unit.
    pub(super) fn parse_single_measurement<'b>(&self, input: &'b str) -> Res<&'b str, Measure> {
        let measurement_parser = (
            opt(leading_qualifier),
            opt(|a| self.parse_multiplier(a)),
            |a| self.parse_value(a),
            space0,
            optional_dash_separator,
            optional_article,
            opt(amount_qualifier_between),
            opt(|a| self.unit(a)),
            optional_period_or_of,
        );

        traced_parser!(
            "parse_single_measurement",
            input,
            context("single_measurement", measurement_parser)
                .parse(input)
                .and_then(|(next_input, res)| {
                    let (
                        _estimate_prefix,
                        multiplier,
                        value,
                        _,
                        _dash,
                        _article,
                        _mid_qualifier,
                        unit,
                        period_consumed,
                    ) = res;

                    let final_value = multiplier.map_or(value.0, |multiplier| value.0 * multiplier);
                    let (final_next_input, final_unit) = self.resolve_single_measurement_unit(
                        input,
                        next_input,
                        unit,
                        period_consumed,
                    )?;

                    Ok((
                        final_next_input,
                        Measure::from_parts(final_unit.as_ref(), final_value, value.1),
                    ))
                }),
            |m: &Measure| m.to_string(),
            "no measurement"
        )
    }

    /// In rich-text (prose) mode, reject a bare number whose continuation is not
    /// actually a quantity. Two cases (no-op outside rich-text mode):
    /// - a **step number**: "1. Bring a pot…" — a numbered instruction, not "1 of X".
    ///   (Only when no measurement-ending period was consumed.)
    /// - a **dimension suffix**: "1-inch piece ginger" — "1-inch" is descriptive in
    ///   prose, whereas in ingredient-list mode the dimension IS the amount (→ 1").
    fn rejected_in_rich_text(&self, next_input: &str, period_consumed: Option<&str>) -> bool {
        if !self.is_rich_text {
            return false;
        }
        (period_consumed.is_none() && looks_like_step_number(next_input))
            || starts_with_dimension_suffix(next_input)
    }

    fn resolve_single_measurement_unit<'b>(
        &self,
        input: &'b str,
        next_input: &'b str,
        unit: Option<String>,
        period_consumed: Option<&str>,
    ) -> Result<(&'b str, String), nom::Err<VerboseError<&'b str>>> {
        if let Some(unit) = unit {
            return Ok((next_input, unit.to_lowercase()));
        }

        if let Some((after_paren, unit)) = self.parse_unit_after_parens(next_input) {
            return Ok((after_paren, unit));
        }

        if self.rejected_in_rich_text(next_input, period_consumed) {
            return Err(reject_measurement(input));
        }

        if let Some((after_dim, unit)) = parse_dimension_unit(next_input) {
            return Ok((after_dim, unit));
        }

        // A hyphenated unit attached to the number, e.g. the "3-pound" in
        // "1 whole 3-pound fish". The hyphen otherwise blocks the unit parser,
        // leaving a spurious "whole" amount and a "-pound …" name.
        if let Some((after_hyphen_unit, unit)) = self.parse_hyphenated_unit(next_input) {
            return Ok((after_hyphen_unit, unit));
        }

        Ok((next_input, DEFAULT_UNIT.to_string()))
    }

    /// Consume a unit hyphenated to the preceding number, e.g. the "-pound" in
    /// "3-pound fish" → unit "pound", leaving " fish". Distance units are handled
    /// earlier by [`parse_dimension_unit`]; this covers weight/volume units
    /// ("3-pound", "5-ounce") that a hyphen would otherwise hide from the unit
    /// parser. Requires the token after the hyphen to be a recognized unit so a
    /// hyphenated *name* ("five-spice") is left alone.
    fn parse_hyphenated_unit<'b>(&self, input: &'b str) -> Option<(&'b str, String)> {
        parse_hyphen_unit_where(input, |unit| unit::is_valid(self.units, unit))
    }

    /// Try to find a unit after skipping a parenthesized description.
    ///
    /// For input like "(13-millimeter/½-inch) slices CHASHU", this skips the
    /// parentheses and returns ("CHASHU", "slices").
    fn parse_unit_after_parens<'b>(&self, input: &'b str) -> Option<(&'b str, String)> {
        let input = input.trim_start();
        if !input.starts_with('(') {
            return None;
        }

        let close_pos = find_matching_paren(input)?;
        let after_paren = input[close_pos + 1..].trim_start();
        let Ok((remaining, unit)) = self.unit(after_paren) else {
            return None;
        };

        let remaining = if let Some(stripped) = remaining.strip_prefix('.') {
            stripped
        } else if let Some(stripped) = remaining.strip_prefix(" of") {
            stripped
        } else {
            remaining
        };

        Some((remaining, unit.to_lowercase()))
    }

    /// Parse a standalone unit with implicit quantity of 1, like "cup" or "tablespoons".
    ///
    /// This is disabled in rich text mode to prevent false positives like
    /// "bullet-proof recipe" being parsed as "1 recipe". In prose, measurements
    /// should always have explicit numbers.
    pub(super) fn parse_unit_only<'b>(&self, input: &'b str) -> Res<&'b str, Measure> {
        if self.is_rich_text {
            return Err(reject_measurement(input));
        }

        // (The early return above already rejected rich-text mode, so a plain
        // `space0` is correct here.)
        let unit_only_format = (
            space0,
            |a| self.unit_extra(a),
            optional_period_or_of,
            space1,
        );

        traced_parser!(
            "parse_unit_only",
            input,
            context("unit_only", unit_only_format).parse(input).map(
                |(next_input, (_, unit, _, _))| {
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

    /// Parse and validate a unit string using the given predicate.
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
            context("unit", verify(parse_unit_text, |s: &str| predicate(s)))
                .parse(input)
                .map(|(rest, s)| (rest, s.to_string())),
            |s: &String| s.clone(),
            err_msg
        )
    }

    /// Parse and validate a unit string.
    pub(super) fn unit<'b>(&self, input: &'b str) -> Res<&'b str, String> {
        // Fluid ounce is the only built-in *multi-word* unit; the generic
        // `parse_unit_text` (a single run of letters) stops at the space in
        // "fl oz", so "18 fl oz water" would lose its unit and fall back to a bare
        // count. Match its spellings explicitly and normalize to canonical "fl oz".
        if let Ok((rest, _)) = fluid_ounce_text(input) {
            return Ok((rest, "fl oz".to_string()));
        }
        self.parse_unit_with(
            input,
            |s| unit::is_valid(self.units, s),
            "unit",
            "not a valid unit",
        )
    }

    /// Parse an addon unit (only units in the custom set, not built-in units).
    ///
    /// This is used for implicit quantity parsing like "cup of flour" where we want
    /// to only match addon units, not built-in units like "whole".
    pub(super) fn unit_extra<'b>(&self, input: &'b str) -> Res<&'b str, String> {
        self.parse_unit_with(
            input,
            |s| unit::is_addon_unit(self.units, s),
            "unit_extra",
            "not an addon unit",
        )
    }
}

/// Recognize the spellings of the fluid-ounce unit ("fl oz", "fl. oz.", "fluid
/// ounce(s)", "fluid oz"). Longest forms first so a prefix isn't matched short.
/// Returns the consumed span; the caller normalizes it to canonical "fl oz".
fn fluid_ounce_text(input: &str) -> Res<&str, &str> {
    alt((
        tag_no_case("fluid ounces"),
        tag_no_case("fluid ounce"),
        tag_no_case("fluid oz"),
        tag_no_case("fl. oz."),
        tag_no_case("fl oz"),
    ))
    .parse(input)
}

fn reject_measurement(input: &str) -> nom::Err<VerboseError<&str>> {
    nom::Err::Error(VerboseError::from_error_kind(
        input,
        nom::error::ErrorKind::Verify,
    ))
}

/// Consume a leading hyphenated dimension unit ("-inch", "-cm", …) and return it
/// as the measurement's unit, leaving the rest of the input. This lets the
/// hyphen form "2-inch piece ginger" parse the dimension as the amount (2"),
/// mirroring the space form "2 inch piece ginger". Returns `None` when the input
/// doesn't start with a hyphen + distance unit.
fn parse_dimension_unit(input: &str) -> Option<(&str, String)> {
    parse_hyphen_unit_where(input, is_distance_unit)
}

/// Shared core of [`parse_dimension_unit`] and
/// [`MeasurementParser::parse_hyphenated_unit`]: consume a leading
/// `-<alphabetic-unit>` and return the lowercased unit plus the rest, accepting
/// the unit only when `is_unit` validates it (distance vs. any valid unit). The
/// validity predicate is the sole difference between the two callers.
fn parse_hyphen_unit_where(input: &str, is_unit: impl Fn(&str) -> bool) -> Option<(&str, String)> {
    let after_hyphen = input.strip_prefix('-')?;
    let end = after_hyphen
        .find(|c: char| !c.is_alphabetic())
        .unwrap_or(after_hyphen.len());
    let unit = &after_hyphen[..end];
    if unit.is_empty() || !is_unit(unit) {
        return None;
    }
    Some((&after_hyphen[end..], unit.to_lowercase()))
}

/// Consume an amount-shape qualifier ("generous", "scant", "heaping", …) that
/// sits *between* the number and the unit, as in "2 generous tablespoons". The
/// qualifier describes how full the measure is; like the leading form it is
/// discarded (the numeric amount is what's structured). Restricted to shape
/// qualifiers — "about"/"approximately" never appear in this position.
///
/// Wrapped in `opt(...)` by the caller, so a non-qualifier word (the real unit)
/// backtracks and is left for the unit parser.
fn amount_qualifier_between(input: &str) -> Res<&str, ()> {
    let (input, _) = alt((
        tag_no_case("generous"),
        tag_no_case("scant"),
        tag_no_case("heaping"),
        tag_no_case("heaped"),
        tag_no_case("rounded"),
        tag_no_case("brimming"),
    ))
    .parse(input)?;
    let (input, _) = space1(input)?;
    Ok((input, ()))
}

/// Consume a leading approximation/size qualifier ("about", "roughly",
/// "generous", "scant", …), optionally preceded by an article ("a"/"an"), so
/// the amount after it still parses. Case-insensitive; the qualifier text is
/// discarded — except by `rich_text`, which re-emits the consumed span as
/// prose (the reason this is pub(crate)).
///
/// Wrapped in `opt(...)` by the caller, so a partial match (e.g. consuming "a "
/// then failing) backtracks and consumes nothing.
pub(crate) fn leading_qualifier(input: &str) -> Res<&str, ()> {
    let (input, _) = opt(alt((tag_no_case("a "), tag_no_case("an ")))).parse(input)?;
    let (input, _) = alt((
        // Multi-word phrases first so the trailing word isn't mistaken for the unit.
        tag_no_case("less than"),
        tag_no_case("about"),
        tag_no_case("approximately"),
        tag_no_case("approx"),
        tag_no_case("roughly"),
        tag_no_case("around"),
        tag_no_case("generous"),
        tag_no_case("scant"),
        tag_no_case("heaping"),
        tag_no_case("heaped"),
        tag_no_case("rounded"),
        tag_no_case("brimming"),
    ))
    .parse(input)?;
    let (input, _) = space1(input)?;
    Ok((input, ()))
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
    fn test_measurement_with_about(units_fx: HashSet<String>) {
        let parser = MeasurementParser::new(&units_fx, false);
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
    fn test_leading_qualifiers(units_fx: HashSet<String>, #[case] input: &str) {
        let parser = MeasurementParser::new(&units_fx, false);
        let (_, measure) = parser.parse_single_measurement(input).unwrap();
        // The qualifier is discarded; the numeric value survives.
        assert!(measure.value() >= 1.0, "input: {input}");
    }

    #[rstest]
    fn test_unit_only(units_fx: HashSet<String>) {
        let parser = MeasurementParser::new(&units_fx, false);
        let result = parser.parse_unit_only(" cup ");
        assert!(result.is_ok());
        let (_, measure) = result.unwrap();
        assert_eq!(measure.value(), 1.0);
    }

    #[rstest]
    fn test_unit_only_rejected_in_rich_text_mode(units_fx: HashSet<String>) {
        let parser = MeasurementParser::new(&units_fx, true);
        assert!(parser.parse_unit_only(" cup ").is_err());
    }

    #[rstest]
    fn test_no_unit_defaults_to_whole(units_fx: HashSet<String>) {
        let parser = MeasurementParser::new(&units_fx, false);
        let result = parser.parse_single_measurement("2 ");
        assert!(result.is_ok());
        let (_, measure) = result.unwrap();
        let measure_str = format!("{measure}");
        assert!(measure_str.contains("whole") || measure.value() == 2.0);
    }

    #[rstest]
    #[case::inch_piece("1-inch piece ginger")]
    #[case::cm_piece("2-cm knob ginger")]
    fn test_dimension_suffix_rejected(units_fx: HashSet<String>, #[case] input: &str) {
        let parser = MeasurementParser::new(&units_fx, true);
        assert!(parser.parse_single_measurement(input).is_err());
    }

    /// The multi-word fluid-ounce unit is recognized across the space the generic
    /// unit-text parser would stop at, in its common spellings, and normalized to
    /// canonical "fl oz".
    #[rstest]
    #[case::abbrev("18 fl oz water")]
    #[case::attached("18fl oz water")]
    #[case::periods("18 fl. oz. water")]
    #[case::spelled("2 fluid ounces cream")]
    fn test_fluid_ounce_unit(units_fx: HashSet<String>, #[case] input: &str) {
        let parser = MeasurementParser::new(&units_fx, false);
        let (_, measure) = parser.parse_single_measurement(input).unwrap();
        assert_eq!(measure.unit_as_string(), "fl oz", "input: {input}");
    }

    #[rstest]
    fn test_unit_after_parenthesized_description(units_fx: HashSet<String>) {
        let parser = MeasurementParser::new(&units_fx, false);
        let result = parser.parse_single_measurement("4 (13-millimeter/½-inch) slices CHASHU");
        assert!(result.is_ok());
        let (remaining, measure) = result.unwrap();
        assert_eq!(remaining, " CHASHU");
        assert_eq!(measure.value(), 4.0);
        assert_eq!(measure.unit_as_string(), "slice");
    }
}
