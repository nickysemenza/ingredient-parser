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
    looks_like_step_number, optional_dash_separator, optional_period_or_of,
    starts_with_dimension_suffix,
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
            opt(|a| self.unit(a)),
            optional_period_or_of,
        );

        traced_parser!(
            "parse_single_measurement",
            input,
            context("single_measurement", tuple(measurement_parser))
                .parse(input)
                .and_then(|(next_input, res)| {
                    let (_estimate_prefix, multiplier, value, _, _dash, unit, period_consumed) =
                        res;

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

        if starts_with_dimension_suffix(next_input) {
            return Err(reject_measurement(input));
        }

        Ok((next_input, DEFAULT_UNIT.to_string()))
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

        let mut depth = 0;
        let mut close_pos = None;
        for (index, character) in input.char_indices() {
            match character {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        close_pos = Some(index);
                        break;
                    }
                }
                _ => {}
            }
        }

        let close_pos = close_pos?;
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
            context("unit", verify(parse_unit_text, |s: &str| predicate(s))).parse(input),
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

/// Consume a leading approximation qualifier ("about", "generous", "scant",
/// "heaping", …), optionally preceded by an article ("a"/"an"), so the amount
/// after it still parses. Case-insensitive; the qualifier text is discarded.
///
/// Wrapped in `opt(...)` by the caller, so a partial match (e.g. consuming "a "
/// then failing) backtracks and consumes nothing.
fn leading_qualifier(input: &str) -> Res<&str, ()> {
    let (input, _) = opt(alt((tag_no_case("a "), tag_no_case("an ")))).parse(input)?;
    let (input, _) = alt((
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
    ))
    .parse(input)?;
    let (input, _) = space1(input)?;
    Ok((input, ()))
}
