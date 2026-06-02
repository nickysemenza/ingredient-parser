use nom::error::ParseError;
#[allow(deprecated)]
use nom::sequence::tuple;
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
    #[allow(deprecated)]
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
            context("single_measurement", tuple(measurement_parser))
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

        if self.is_rich_text && period_consumed.is_none() && looks_like_step_number(next_input) {
            return Err(reject_measurement(input));
        }

        // In rich text (prose), a hyphenated dimension like "1-inch" in
        // "1-inch piece ginger" is descriptive, not a quantity, so reject it. In
        // ingredient-list mode the dimension IS the amount (e.g. "2-inch piece
        // ginger" → 2"), parsed below by parse_dimension_unit.
        if self.is_rich_text && starts_with_dimension_suffix(next_input) {
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
        let after_hyphen = input.strip_prefix('-')?;
        let end = after_hyphen
            .find(|c: char| !c.is_alphabetic())
            .unwrap_or(after_hyphen.len());
        let unit = &after_hyphen[..end];
        if unit.is_empty() || !unit::is_valid(self.units, unit) {
            return None;
        }
        Some((&after_hyphen[end..], unit.to_lowercase()))
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

        let unit_only_format = (
            |a| {
                if self.is_rich_text {
                    space1(a)
                } else {
                    space0(a)
                }
            },
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
    let after_hyphen = input.strip_prefix('-')?;
    let end = after_hyphen
        .find(|c: char| !c.is_alphabetic())
        .unwrap_or(after_hyphen.len());
    let unit = &after_hyphen[..end];
    if unit.is_empty() || !is_distance_unit(unit) {
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

/// Consume a leading approximation qualifier ("about", "generous", "scant",
/// "heaping", …), optionally preceded by an article ("a"/"an"), so the amount
/// after it still parses. Case-insensitive; the qualifier text is discarded.
///
/// Wrapped in `opt(...)` by the caller, so a partial match (e.g. consuming "a "
/// then failing) backtracks and consumes nothing.
/// Consume a leading approximation/size qualifier ("about", "roughly",
/// "generous", …) with an optional indefinite article, e.g. the "about " in
/// "about 3 minutes". Exposed for `rich_text`, which re-emits the consumed span
/// as prose instead of discarding it.
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
