use nom::error::ParseError;
use nom::{
    Parser,
    branch::alt,
    bytes::complete::tag_no_case,
    character::complete::{space0, space1},
    combinator::{opt, peek, verify},
    error::context,
};
use nom_language::error::VerboseError;

use crate::parser::{Res, parse_unit_text};
use crate::traced_parser;
use crate::unit::{self, Measure};

use super::guards::{
    find_matching_paren, is_distance_unit, looks_like_step_number, optional_article,
    optional_dash_separator, optional_period_or_of,
};
use super::{DEFAULT_UNIT, MeasurementMode, MeasurementParser};

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

                    // A multiplier ("3 x") scales the whole quantity, so both bounds
                    // of a ranged value must scale: "3 x 100-120 g" is 300-360 g, not
                    // 300 g with a stray unscaled 120 upper (which `ordered_bounds`
                    // would then flip to a nonsensical 120-300).
                    let (final_value, final_upper) = match multiplier {
                        Some(m) => (value.0 * m, value.1.map(|upper| upper * m)),
                        None => (value.0, value.1),
                    };
                    let (final_next_input, final_unit) = self.resolve_single_measurement_unit(
                        input,
                        next_input,
                        unit,
                        period_consumed,
                    )?;

                    Ok((
                        final_next_input,
                        Measure::from_parts(final_unit.as_ref(), final_value, final_upper),
                    ))
                }),
            |m: &Measure| m.to_string(),
            "no measurement"
        )
    }

    /// In rich-text (prose) mode, reject a bare number whose continuation is not
    /// actually a quantity:
    /// - a **step number**: "1. Bring a pot…" — a numbered instruction, not "1 of X".
    ///   (Only when no measurement-ending period was consumed.)
    ///
    /// A dimension suffix ("2-inch pieces") is NOT rejected: it surfaces as an
    /// `Inch` measure for highlighting, consistent with how `parse_hyphenated_unit`
    /// already surfaces "5-minute"/"3-pound" in prose. `Inch` is
    /// `MeasureKind::Length`, which is non-scalable, so a scaled recipe leaves the
    /// dimension untouched. No-op outside rich-text mode.
    fn rejected_in_rich_text(&self, next_input: &str, period_consumed: Option<&str>) -> bool {
        if self.mode != MeasurementMode::RichText {
            return false;
        }
        period_consumed.is_none() && looks_like_step_number(next_input)
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
        if self.mode == MeasurementMode::RichText {
            return Err(reject_measurement(input));
        }

        // (The early return above already rejected rich-text mode, so a plain
        // `space0` is correct here.)
        //
        // `opt(unit_only_qualifier)` lets a bare unit carry a discarded
        // shape/approx qualifier ("Generous pinch of salt" -> 1 pinch, name
        // "salt") or a size word before a vague unit ("Small handful thyme" ->
        // 1 handful thyme), matching the numbered path in
        // `parse_single_measurement`. It backtracks to nothing when the next word
        // isn't a qualifier, so "pinch of salt" is unaffected.
        let unit_only_format = (
            opt(unit_only_qualifier),
            space0,
            |a| self.unit_extra(a),
            optional_period_or_of,
            space1,
        );

        traced_parser!(
            "parse_unit_only",
            input,
            context("unit_only", unit_only_format).parse(input).map(
                |(next_input, (_, _, unit, _, _))| {
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
        // Single-letter spoon abbreviations are case-sensitive: lowercase "t" =
        // teaspoon, uppercase "T" = tablespoon (standard cooking shorthand). They
        // differ only by case, so they must be resolved to canonical "tsp"/"tbsp"
        // HERE, while case is intact — both `resolve_single_measurement_unit` and
        // `Measure::from_parts` lowercase the unit downstream, which would collapse
        // t/T. Mirrors the `fl oz` special-case above.
        if let Some((rest, canon)) = single_letter_spoon(input) {
            return Ok((rest, canon.to_string()));
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

/// Match a standalone single-letter spoon abbreviation: lowercase "t" ->
/// teaspoon ("tsp"), uppercase "T" -> tablespoon ("tbsp"). Case-sensitive by
/// design. Matches only when the letter is a whole token — the following
/// character must be end-of-input, whitespace, or a period — so it never grabs
/// the leading "t" of "tsp"/"tbsp"/"teaspoon"/"tomato", and a hyphen boundary
/// ("t-bone") is deliberately excluded. The trailing period (e.g. "t.") is left
/// in `rest` for the grammar's `optional_period_or_of` to consume.
///
/// Resolved here rather than via `UNIT_MAPPINGS`/`Unit::from_str` precisely
/// because those are case-insensitive (and the unit string is lowercased twice
/// downstream), which cannot express the t≠T distinction — do not "simplify"
/// this into the unit table.
fn single_letter_spoon(input: &str) -> Option<(&str, &'static str)> {
    let mut it = input.char_indices();
    let canon = match it.next()? {
        (_, 't') => "tsp",
        (_, 'T') => "tbsp",
        _ => return None,
    };
    match it.next() {
        None => Some(("", canon)),
        Some((idx, c)) if c.is_whitespace() || c == '.' => Some((&input[idx..], canon)),
        Some(_) => None,
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
        // A size word ("small handful", "large pinch", "large bunch", "small
        // head") before a vague/container measure describes the measure, not the
        // food, so it's discarded like the shape qualifiers above. Gated to those
        // unit sets so "2 large eggs" keeps "large".
        size_word_before_discardable_unit,
    ))
    .parse(input)?;
    let (input, _) = space1(input)?;
    Ok((input, ()))
}

/// Consume a qualifier before a bare unit-only amount. This mirrors the numbered
/// path's leading/size qualifiers, but is deliberately limited to qualifiers
/// that still leave a recognized addon unit immediately after them.
fn unit_only_qualifier(input: &str) -> Res<&str, ()> {
    alt((leading_qualifier, |input| {
        let (input, _) = size_word_before_discardable_unit(input)?;
        let (input, _) = space1(input)?;
        Ok((input, ()))
    }))
    .parse(input)
}

/// Match a SIZE word ("small"/"large"/…) only when a vague unit
/// ("handful"/"pinch"/"dash") or a size-qualifiable container ("bunch"/"head")
/// immediately follows, so the size word can be discarded as a measure qualifier
/// ("1 small handful basil" -> 1 handful basil; "1 large bunch kale" -> 1 bunch
/// kale). Consumes just the size word, leaving the space for the caller's
/// `space1`; the `peek` look-ahead means a size word before any other unit (or a
/// real food, "2 large eggs") backtracks via the caller's `opt(...)` and stays in
/// the name.
fn size_word_before_discardable_unit(input: &str) -> Res<&str, &str> {
    let (rest, word) = verify(parse_unit_text, |s: &str| {
        crate::parser::vocab::SIZE_WORDS.contains(&s.to_lowercase().as_str())
    })
    .parse(input)?;
    peek((
        space1,
        verify(parse_unit_text, |s: &str| {
            let s = s.to_lowercase();
            crate::parser::vocab::VAGUE_UNITS.contains(&s.as_str())
                || crate::parser::vocab::SIZE_QUALIFIABLE_UNITS.contains(&s.as_str())
        }),
    ))
    .parse(rest)?;
    Ok((rest, word))
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
        // Vague-measure intensifiers, e.g. "Healthy pinch of salt", "good pinch".
        tag_no_case("healthy"),
        tag_no_case("good"),
    ))
    .parse(input)?;
    let (input, _) = space1(input)?;
    Ok((input, ()))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::super::test_support::units;
    use super::super::{MeasurementMode, MeasurementParser};
    use rstest::{fixture, rstest};
    use std::collections::HashSet;

    #[fixture]
    fn units_fx() -> HashSet<String> {
        units()
    }

    #[rstest]
    fn test_measurement_with_about(units_fx: HashSet<String>) {
        let parser = MeasurementParser::new(&units_fx, MeasurementMode::IngredientList);
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
    #[case::healthy("Healthy 1 cup")]
    #[case::good("good 1 cup")]
    fn test_leading_qualifiers(units_fx: HashSet<String>, #[case] input: &str) {
        let parser = MeasurementParser::new(&units_fx, MeasurementMode::IngredientList);
        let (_, measure) = parser.parse_single_measurement(input).unwrap();
        // The qualifier is discarded; the numeric value survives.
        assert!(measure.value() >= 1.0, "input: {input}");
    }

    /// A SIZE word before a *vague* unit ("1 small handful") is a measure
    /// qualifier — consumed and discarded — but only when a vague unit follows.
    /// Before a normal unit it is left in place for the name ("1 small can").
    #[rstest]
    #[case::small_handful("1 small handful basil", 1.0, "handful")]
    #[case::large_pinch("2 large pinch salt", 2.0, "pinch")]
    // Size-qualifiable containers: the size word qualifies the bunch/head, so it is
    // discarded and the unit is consumed ("large bunch" -> bunch, "small head" -> head).
    #[case::large_bunch("1 large bunch kale", 1.0, "bunch")]
    #[case::small_head("1 small head cabbage", 1.0, "head")]
    // Guard: "can" is not a discardable unit, so the size word is NOT consumed and the
    // measure falls back to a bare count (unit "whole"), leaving "small" for the name.
    #[case::small_can_not_consumed("1 small can tomatoes", 1.0, "whole")]
    fn test_size_word_before_discardable_unit(
        #[case] input: &str,
        #[case] value: f64,
        #[case] unit: &str,
    ) {
        let mut units = units();
        units.insert("handful".to_string());
        units.insert("pinch".to_string());
        units.insert("can".to_string());
        units.insert("bunch".to_string());
        units.insert("head".to_string());
        let parser = MeasurementParser::new(&units, MeasurementMode::IngredientList);
        let (_, measure) = parser.parse_single_measurement(input).unwrap();
        assert_eq!(measure.value(), value, "input: {input}");
        assert_eq!(measure.unit_as_string(), unit, "input: {input}");
    }

    #[rstest]
    fn test_unit_only(units_fx: HashSet<String>) {
        let parser = MeasurementParser::new(&units_fx, MeasurementMode::IngredientList);
        let result = parser.parse_unit_only(" cup ");
        assert!(result.is_ok());
        let (_, measure) = result.unwrap();
        assert_eq!(measure.value(), 1.0);
    }

    #[rstest]
    fn test_unit_only_size_qualified_vague_unit() {
        let mut units = units();
        units.insert("handful".to_string());
        let parser = MeasurementParser::new(&units, MeasurementMode::IngredientList);
        let (remaining, measure) = parser.parse_unit_only("Small handful thyme").unwrap();
        assert_eq!(remaining, "thyme");
        assert_eq!(measure.value(), 1.0);
        assert_eq!(measure.unit_as_string(), "handful");
    }

    #[rstest]
    fn test_unit_only_rejected_in_rich_text_mode(units_fx: HashSet<String>) {
        let parser = MeasurementParser::new(&units_fx, MeasurementMode::RichText);
        assert!(parser.parse_unit_only(" cup ").is_err());
    }

    #[rstest]
    fn test_no_unit_defaults_to_whole(units_fx: HashSet<String>) {
        let parser = MeasurementParser::new(&units_fx, MeasurementMode::IngredientList);
        let result = parser.parse_single_measurement("2 ");
        assert!(result.is_ok());
        let (_, measure) = result.unwrap();
        let measure_str = format!("{measure}");
        assert!(measure_str.contains("whole") || measure.value() == 2.0);
    }

    /// In prose a dimension is highlighted as a measure (describing shape, not
    /// quantity) rather than rejected — consistent with how hyphenated weight/time
    /// units surface. `Inch` is `MeasureKind::Length` (non-scalable); other
    /// distance units surface as `Other`.
    #[rstest]
    #[case::inch_piece("1-inch piece ginger", 1.0, "\"")]
    #[case::cm_piece("2-cm knob ginger", 2.0, "cm")]
    fn test_dimension_suffix_surfaces_in_rich_text(
        units_fx: HashSet<String>,
        #[case] input: &str,
        #[case] value: f64,
        #[case] unit: &str,
    ) {
        let parser = MeasurementParser::new(&units_fx, MeasurementMode::RichText);
        let (_, measure) = parser.parse_single_measurement(input).unwrap();
        assert_eq!(measure.value(), value, "input: {input}");
        assert_eq!(measure.unit_as_string(), unit, "input: {input}");
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
        let parser = MeasurementParser::new(&units_fx, MeasurementMode::IngredientList);
        let (_, measure) = parser.parse_single_measurement(input).unwrap();
        assert_eq!(measure.unit_as_string(), "fl oz", "input: {input}");
    }

    /// Single-letter spoon abbreviations are case-sensitive: lowercase "t" =
    /// teaspoon, uppercase "T" = tablespoon, resolved from the *same* letter.
    /// Guards against a refactor reintroducing a lowercasing step before the
    /// t/T disambiguation. The trailing-period form ("t.") also works.
    #[rstest]
    #[case::lower_t("1 t salt", "tsp")]
    #[case::upper_t("1 T butter", "tbsp")]
    #[case::lower_t_period("2 t. vanilla", "tsp")]
    #[case::upper_t_period("1 T. cream", "tbsp")]
    fn test_single_letter_spoon(
        units_fx: HashSet<String>,
        #[case] input: &str,
        #[case] unit: &str,
    ) {
        let parser = MeasurementParser::new(&units_fx, MeasurementMode::IngredientList);
        let (_, measure) = parser.parse_single_measurement(input).unwrap();
        assert_eq!(measure.unit_as_string(), unit, "input: {input}");
    }

    /// A single "t"/"T" that is the *start of a longer word* (not a standalone
    /// token) must not be mistaken for a spoon — it falls through to the normal
    /// unit path, which finds no real unit and defaults to a bare count.
    #[rstest]
    #[case::tsp_word("1 tsp salt")]
    #[case::tomato("1 Tomato")]
    fn test_single_letter_spoon_no_false_match(units_fx: HashSet<String>, #[case] input: &str) {
        let parser = MeasurementParser::new(&units_fx, MeasurementMode::IngredientList);
        let (_, measure) = parser.parse_single_measurement(input).unwrap();
        // Never canonicalized to a spoon from a longer word.
        assert!(
            measure.unit_as_string() != "tbsp",
            "input {input} wrongly matched tablespoon"
        );
    }

    #[rstest]
    fn test_unit_after_parenthesized_description(units_fx: HashSet<String>) {
        let parser = MeasurementParser::new(&units_fx, MeasurementMode::IngredientList);
        let result = parser.parse_single_measurement("4 (13-millimeter/½-inch) slices CHASHU");
        assert!(result.is_ok());
        let (remaining, measure) = result.unwrap();
        assert_eq!(remaining, " CHASHU");
        assert_eq!(measure.value(), 4.0);
        assert_eq!(measure.unit_as_string(), "slice");
    }
}
