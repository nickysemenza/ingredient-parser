use std::borrow::Cow;
use std::cmp::Reverse;

#[allow(deprecated)]
use nom::{
    bytes::complete::tag,
    character::complete::{not_line_ending, space0, space1},
    combinator::{opt, verify},
    error::context,
    multi::many1,
    Parser,
};

use crate::parser::{parse_ingredient_text, parse_unit_text, MeasurementParser, Res};
use crate::trace;
use crate::traced_parser;
use crate::unit::{self, Measure};
use crate::{Ingredient, IngredientParser};

impl IngredientParser {
    pub(crate) fn parse_ingredient_line(&self, input: &str) -> Ingredient {
        let normalized = normalize_input(input);
        self.parse_normalized_ingredient(normalized.as_ref())
    }

    pub(crate) fn parse_ingredient_line_with_trace(
        &self,
        input: &str,
    ) -> trace::ParseWithTrace<Ingredient> {
        let normalized = normalize_input(input);
        let input = normalized.as_ref();

        trace::enable_tracing();
        let result = self.parse_normalized_ingredient(input);
        let trace = trace::disable_tracing(input);

        trace::ParseWithTrace {
            result: Ok(result),
            trace,
        }
    }

    fn parse_normalized_ingredient(&self, input: &str) -> Ingredient {
        if let Some(ingredient) = self.try_parse_optional_ingredient(input) {
            return ingredient;
        }

        if let Some(ingredient) = self.try_parse_trailing_amount_format(input) {
            return ingredient;
        }

        if let Some(ingredient) = self.try_parse_x_of_construction(input) {
            return ingredient;
        }

        self.parse_core_ingredient(input)
            // Reject a "successful" parse that lost the ingredient name into the
            // modifier (seen on real recipes: a decimal comma in "1,000 grams
            // ... nectarines", a leading prep word, etc.) — the graceful
            // fallback is better than a name-less ingredient with garbled text.
            // A bare quantity like "1/2-1 cup" legitimately has no name, so only
            // fall back when the empty name coincides with leftover modifier text.
            .filter(|ingredient| {
                let name_empty = ingredient.name.trim().is_empty();
                let has_modifier = ingredient
                    .modifier
                    .as_deref()
                    .is_some_and(|m| !m.trim().is_empty());
                !(name_empty && has_modifier)
            })
            .unwrap_or_else(|| fallback_ingredient(input))
    }

    fn parse_core_ingredient(&self, input: &str) -> Option<Ingredient> {
        // A descriptive parenthetical sitting *between* name words — e.g. the
        // "(70° to 80°F)" in "room-temperature (70° to 80°F) water" or the
        // "(¼ inch / 6 mm)" in "sliced (¼ inch / 6 mm) green onions" — breaks the
        // name grammar. Lift it out to the modifier and parse the cleaned line,
        // so the real name and amounts survive. Scoped to temperature/distance
        // asides flanked by name text, so mass/volume parentheticals like
        // "(190 grams)" stay hoisted as amounts and "4 (½-inch) slices" (count +
        // size) is untouched.
        if let Some((cleaned, aside)) = lift_inline_descriptive_paren(input) {
            let mut ingredient = self
                .parse_ingredient(&cleaned)
                .ok()
                .map(|(_, ingredient)| self.postprocess_ingredient(ingredient))?;
            append_modifier(&mut ingredient.modifier, &aside);
            ingredient.modifier = clean_modifier(ingredient.modifier);
            return Some(ingredient);
        }

        self.parse_ingredient(input)
            .ok()
            .map(|(_, ingredient)| self.postprocess_ingredient(ingredient))
    }

    fn postprocess_ingredient(&self, mut ingredient: Ingredient) -> Ingredient {
        self.fix_leading_prep_phrase(&mut ingredient);
        self.extract_adjectives_from_name(&mut ingredient);
        ingredient.name = collapse_whitespace(&ingredient.name);
        self.extract_alternative_from_name(&mut ingredient);
        self.extract_secondary_amounts_from_modifier(&mut ingredient);
        ingredient.modifier = strip_wrapping_parens(clean_modifier(ingredient.modifier));
        ingredient
    }

    /// Recover from a leading prep phrase that displaced the ingredient name.
    ///
    /// A line like "2/3 cup finely chopped, raw pistachios" parses with the
    /// text *before* the comma as the name and the text *after* as the modifier,
    /// yielding name="finely chopped" / modifier="raw pistachios" — backwards.
    /// When the whole name is a single known prep phrase and a modifier is
    /// present, swap them so the prep phrase becomes the modifier and the real
    /// name is restored. The exact-match guard keeps descriptive names (e.g.
    /// "raw pistachios, finely chopped", where the name isn't a prep phrase) from
    /// ever being touched.
    fn fix_leading_prep_phrase(&self, ingredient: &mut Ingredient) {
        let name = ingredient.name.trim();
        if name.is_empty() || !self.adjectives.contains(&name.to_lowercase()) {
            return;
        }
        let Some(modifier) = ingredient
            .modifier
            .as_deref()
            .map(str::trim)
            .filter(|m| !m.is_empty())
        else {
            return;
        };
        let prep = name.to_string();
        ingredient.name = modifier.to_string();
        ingredient.modifier = Some(prep);
    }

    /// Try to parse an optional ingredient format: "(amount ingredient, modifier)"
    ///
    /// When an entire ingredient line is wrapped in parentheses, it indicates
    /// the ingredient is optional. This is common in cookbooks like Joy of Cooking.
    fn try_parse_optional_ingredient(&self, input: &str) -> Option<Ingredient> {
        let trimmed = input.trim();

        if !trimmed.starts_with('(') || !trimmed.ends_with(')') {
            return None;
        }

        let inner = &trimmed[1..trimmed.len() - 1];
        let mut ingredient = self.parse_core_ingredient(inner)?;
        if ingredient.name.is_empty() && ingredient.amounts.is_empty() {
            return None;
        }

        ingredient.optional = true;
        Some(ingredient)
    }

    /// Try to parse ingredient with trailing amount format: "Name — AMOUNT"
    ///
    /// This handles professional/European cookbook formats where the amount
    /// comes at the end after an em-dash, en-dash, or double hyphen.
    fn try_parse_trailing_amount_format(&self, input: &str) -> Option<Ingredient> {
        let separators = [" — ", " – ", " -- "];
        let mp = MeasurementParser::new(&self.units, self.is_rich_text);

        for sep in separators {
            let Some(pos) = input.rfind(sep) else {
                continue;
            };

            let name_part = &input[..pos];
            let amount_part = &input[pos + sep.len()..];

            let Ok((remaining, amounts)) = mp.parse_measurement_list(amount_part) else {
                continue;
            };

            if amounts.is_empty()
                || !remaining.trim().is_empty()
                || !amounts.iter().any(|m| !is_temperature_unit(m.unit()))
            {
                continue;
            }

            return Some(Ingredient {
                name: name_part.trim().to_string(),
                amounts,
                modifier: None,
                optional: false,
            });
        }

        None
    }

    /// Try to parse an "X of/from N item" construction such as "Juice of 1 lemon",
    /// "Grated zest of 2 limes", "Finely grated zest from 1 lemon", or
    /// "Peel of 1 grapefruit". These describe a component derived from a countable
    /// item; the item becomes the name (with its count), and the leading phrase
    /// ("juice of", "zest from", "peel of", ...) moves into the modifier.
    fn try_parse_x_of_construction(&self, input: &str) -> Option<Ingredient> {
        // Longest phrases first so e.g. "grated zest of" wins over "zest of".
        const PHRASES: &[&str] = &[
            "finely grated zest from",
            "finely grated zest of",
            "grated zest from",
            "grated zest of",
            "juice and zest of",
            "zest and juice of",
            "the juice of",
            "the zest of",
            "juice from",
            "zest from",
            "peel from",
            "juice of",
            "zest of",
            "peel of",
        ];

        let trimmed = input.trim();
        let lower = trimmed.to_lowercase();
        // Require a space right after the phrase so "zest often..." can't match.
        let phrase = PHRASES
            .iter()
            .find(|p| lower.starts_with(**p) && lower.as_bytes().get(p.len()) == Some(&b' '))?;

        // The phrase is ASCII, so its byte length indexes a char boundary in `trimmed`.
        let rest = trimmed[phrase.len()..].trim_start();
        let mut parsed = self.parse_core_ingredient(rest)?;

        // Only treat this as the construction when the remainder actually carried a
        // quantity and an item (e.g. "1 lemon"); otherwise fall through to normal
        // parsing so "zest of lemon" (no count) stays name-only.
        if parsed.amounts.is_empty() || parsed.name.trim().is_empty() {
            return None;
        }

        parsed.modifier = match parsed.modifier.take() {
            Some(existing) if !existing.trim().is_empty() => Some(format!("{phrase}, {existing}")),
            _ => Some((*phrase).to_string()),
        };
        Some(parsed)
    }

    /// Parse a complete ingredient line including amounts, name, and modifiers.
    ///
    /// This method only captures the raw grammar shape. Cleanup such as adjective
    /// extraction, alternative extraction, and secondary amount extraction happens
    /// in the higher-level ingredient pipeline.
    #[tracing::instrument(name = "parse_ingredient")]
    pub(crate) fn parse_ingredient<'a>(&self, input: &'a str) -> Res<&'a str, Ingredient> {
        let mp = MeasurementParser::new(&self.units, self.is_rich_text);

        let ingredient_format = (
            opt(|a| mp.parse_measurement_list(a)),
            space0,
            opt(|a| mp.parse_bracketed_amounts(a)),
            space0,
            opt((|a| self.adjective(a), space1)),
            opt(many1(parse_ingredient_text)),
            opt(|a| mp.parse_parenthesized_amounts(a)),
            opt(tag(", ")),
            not_line_ending,
        );

        traced_parser!(
            "parse_ingredient",
            input,
            context("ingredient", ingredient_format).parse(input).map(
                |(
                    next_input,
                    (
                        primary_amounts,
                        _,
                        bracketed_amounts,
                        _,
                        adjective,
                        name_chunks,
                        paren_amounts,
                        _,
                        modifier_text,
                    ),
                )| {
                    (
                        next_input,
                        Ingredient {
                            name: raw_name(name_chunks),
                            amounts: merge_amounts(
                                primary_amounts,
                                bracketed_amounts,
                                paren_amounts,
                            ),
                            modifier: raw_modifier(adjective, modifier_text),
                            optional: false,
                        },
                    )
                },
            ),
            |i: &Ingredient| i.name.clone(),
            "parse failed"
        )
    }

    /// Parse and validate an adjective string.
    fn adjective<'a>(&self, input: &'a str) -> Res<&'a str, String> {
        traced_parser!(
            "adjective",
            input,
            context(
                "adjective",
                verify(parse_unit_text, |s: &str| {
                    self.adjectives.contains(&s.to_lowercase())
                }),
            )
            .parse(input)
            .map(|(rest, s)| (rest, s.to_string())),
            |s: &String| s.clone(),
            "not an adjective"
        )
    }

    fn extract_adjectives_from_name(&self, ingredient: &mut Ingredient) {
        let mut name = ingredient.name.clone();
        let mut name_lower = name.to_lowercase();
        let mut found_adjectives: Vec<&String> = self
            .adjectives
            .iter()
            .filter(|adj| name_lower.contains(adj.as_str()))
            .collect();
        found_adjectives.sort_by_key(|adj| Reverse(adj.len()));

        for adjective in found_adjectives {
            let Some(pos) = name_lower.find(adjective.as_str()) else {
                continue;
            };

            let end = pos + adjective.len();
            // `pos`/`end` are byte offsets into the lowercased name. Lowercasing
            // can change byte lengths for some Unicode (e.g. 'İ' -> "i̇"), so these
            // offsets may not fall on char boundaries in the original `name`.
            // Skip rather than panic when slicing `name` would split a char.
            if !name.is_char_boundary(pos) || !name.is_char_boundary(end) {
                continue;
            }

            // Require a whitespace/string-edge boundary on both sides, so an
            // adjective embedded in a larger token is left alone (e.g. "chopped"
            // inside "well-chopped" must not corrupt the name into "well-").
            let before_boundary = name[..pos]
                .chars()
                .next_back()
                .is_none_or(char::is_whitespace);
            let after_boundary = name[end..].chars().next().is_none_or(char::is_whitespace);
            if !before_boundary || !after_boundary {
                continue;
            }

            append_modifier(&mut ingredient.modifier, adjective);

            let before = name[..pos].trim();
            let after = name[end..].trim();
            let mut new_name = String::with_capacity(name.len());
            if !before.is_empty() {
                new_name.push_str(before);
                if !after.is_empty() {
                    new_name.push(' ');
                }
            }
            if !after.is_empty() {
                new_name.push_str(after);
            }

            name = new_name.trim().to_string();
            name_lower = name.to_lowercase();
        }

        ingredient.name = name;
    }

    fn extract_alternative_from_name(&self, ingredient: &mut Ingredient) {
        let (name, alternative) = extract_alternative(&ingredient.name);
        ingredient.name = name;
        if let Some(alternative) = alternative {
            append_modifier(&mut ingredient.modifier, &alternative);
        }
    }

    fn extract_secondary_amounts_from_modifier(&self, ingredient: &mut Ingredient) {
        let Some(modifier) = ingredient.modifier.as_ref() else {
            return;
        };

        let (secondary_amounts, cleaned_modifier) =
            extract_secondary_amounts(modifier, &self.units);
        ingredient.amounts.extend(secondary_amounts);
        ingredient.modifier = clean_modifier(Some(cleaned_modifier));
    }
}

/// Detect a *descriptive* parenthetical wedged between name words — a
/// temperature ("70° to 80°F") or distance ("¼ inch / 6 mm") aside flanked by
/// alphabetic name text on both sides. Returns the line with that parenthetical
/// removed plus the aside text (to become a modifier), or `None` when no such
/// parenthetical is present.
///
/// Deliberately narrow: requires a letter immediately before the `(` and name
/// text after the `)`, and only fires for temperature/distance asides. This
/// keeps mass/volume parentheticals like "(190 grams)" hoisted as amounts, and
/// leaves the count+size form "4 (½-inch) slices" (digit before the paren) and
/// trailing parentheticals like "water (100°F) — 472 g" to their own paths.
fn lift_inline_descriptive_paren(input: &str) -> Option<(String, String)> {
    let open = input.find('(')?;
    // A letter must immediately precede the "(" (allowing one space): this is the
    // "name (aside) name" shape, not "<count> (size)" or a leading paren.
    let before = input[..open].trim_end();
    if !before.chars().next_back().is_some_and(char::is_alphabetic) {
        return None;
    }
    // Matching close paren (no nesting expected in these asides).
    let close_rel = input[open..].find(')')?;
    let close = open + close_rel;
    let inner = input[open + 1..close].trim();
    let after = input[close + 1..].trim_start();

    // Name text must follow the parenthetical (else it's a trailing paren).
    if !after.chars().next().is_some_and(char::is_alphabetic) {
        return None;
    }

    // Only lift descriptive asides: a temperature (°) or a distance unit token.
    let looks_descriptive = inner.contains('°')
        || inner
            .split(|c: char| !c.is_alphabetic())
            .any(|w| !w.is_empty() && super::measurement::guards::is_distance_unit(w));
    if !looks_descriptive {
        return None;
    }

    let cleaned = format!("{before} {after}");
    Some((cleaned, inner.to_string()))
}

fn normalize_input(input: &str) -> Cow<'_, str> {
    let normalized = if input.contains('\u{a0}') {
        Cow::Owned(input.replace('\u{a0}', " "))
    } else {
        Cow::Borrowed(input)
    };

    let has_multiple_spaces = normalized
        .as_bytes()
        .windows(2)
        .any(|w| w[0] == b' ' && w[1] == b' ');

    if has_multiple_spaces {
        Cow::Owned(collapse_whitespace(normalized.as_ref()))
    } else {
        normalized
    }
}

fn fallback_ingredient(input: &str) -> Ingredient {
    Ingredient {
        name: input.trim().to_string(),
        amounts: vec![],
        modifier: None,
        optional: false,
    }
}

fn raw_name(name_chunks: Option<Vec<&str>>) -> String {
    name_chunks.unwrap_or_default().join("").trim().to_string()
}

fn raw_modifier(adjective: Option<(String, &str)>, modifier_text: &str) -> Option<String> {
    let mut modifier = modifier_text.to_owned();
    if let Some((adjective, _)) = adjective {
        modifier.push_str(&adjective);
    }
    clean_modifier(Some(modifier))
}

fn merge_amounts(
    primary_amounts: Option<Vec<Measure>>,
    bracketed_amounts: Option<Vec<Measure>>,
    paren_amounts: Option<Vec<Measure>>,
) -> Vec<Measure> {
    let mut amounts = Vec::new();
    if let Some(primary_amounts) = primary_amounts {
        amounts.extend(primary_amounts);
    }
    if let Some(bracketed_amounts) = bracketed_amounts {
        amounts.extend(bracketed_amounts);
    }
    if let Some(paren_amounts) = paren_amounts {
        amounts.extend(paren_amounts);
    }
    amounts
}

fn append_modifier(modifier: &mut Option<String>, addition: &str) {
    if addition.is_empty() {
        return;
    }

    match modifier {
        Some(modifier) if !modifier.is_empty() => {
            modifier.push_str(", ");
            modifier.push_str(addition);
        }
        Some(modifier) => modifier.push_str(addition),
        None => *modifier = Some(addition.to_string()),
    }
}

/// Strip a single pair of parentheses that wraps the *entire* modifier, e.g.
/// "(softened)" -> "softened". Modifiers with internal parentheses or only
/// partial wrapping are left untouched.
fn strip_wrapping_parens(modifier: Option<String>) -> Option<String> {
    let modifier = modifier?;
    let trimmed = modifier.trim();
    if let Some(inner) = trimmed.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        if !inner.contains('(') && !inner.contains(')') {
            let inner = inner.trim();
            return (!inner.is_empty()).then(|| inner.to_string());
        }
    }
    Some(modifier)
}

fn clean_modifier(modifier: Option<String>) -> Option<String> {
    modifier.and_then(|modifier| {
        let trimmed = modifier.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn collapse_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Extract alternative ingredients from the name (e.g., "garlic or 1 teaspoon garlic powder")
///
/// Returns `(cleaned_name, optional_alternative)` where:
/// - `cleaned_name`: The ingredient name with alternative removed
/// - `optional_alternative`: The alternative portion to be added to modifier
fn extract_alternative(name: &str) -> (String, Option<String>) {
    use regex::Regex;
    use std::sync::LazyLock;

    static ALTERNATIVE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
        #[allow(clippy::expect_used)]
        Regex::new(r"(?i)\s+or\s+(\d+|a\s+|an\s+)").expect("invalid alternative pattern regex")
    });

    let Some(matched) = ALTERNATIVE_PATTERN.find(name) else {
        return (name.to_string(), None);
    };

    let (ingredient_part, alternative_part) = name.split_at(matched.start());
    let alternative = alternative_part.trim();
    if alternative.is_empty() {
        return (name.to_string(), None);
    }

    (
        ingredient_part.trim().to_string(),
        Some(alternative.to_string()),
    )
}

/// Extract secondary amounts from modifier patterns like "(from about 15 sprigs)".
///
/// Returns `(extracted_amounts, cleaned_modifier)` where:
/// - `extracted_amounts`: `Vec<Measure>` parsed from the pattern
/// - `cleaned_modifier`: The modifier with the pattern removed
fn extract_secondary_amounts(
    modifier: &str,
    units: &std::collections::HashSet<String>,
) -> (Vec<Measure>, String) {
    use regex::Regex;
    use std::sync::LazyLock;

    static SECONDARY_AMOUNT_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
        #[allow(clippy::expect_used)]
        Regex::new(r"\((?:from\s+)?(?:about|approximately|roughly|around)\s+([^)]+)\)")
            .expect("invalid secondary amount regex")
    });

    let Some(caps) = SECONDARY_AMOUNT_PATTERN.captures(modifier) else {
        return (vec![], modifier.to_string());
    };

    let Some(full_match) = caps.get(0) else {
        return (vec![], modifier.to_string());
    };
    let Some(amount_match) = caps.get(1) else {
        return (vec![], modifier.to_string());
    };
    let amount_text = amount_match.as_str().trim();

    let mp = MeasurementParser::new(units, false);
    let Ok((remaining, measures)) = mp.parse_measurement_list(amount_text) else {
        return (vec![], modifier.to_string());
    };

    let remaining_trimmed = remaining.trim();
    let is_simple_remaining = remaining_trimmed.is_empty()
        || (remaining_trimmed.split_whitespace().count() == 1
            && remaining_trimmed.chars().all(char::is_alphabetic));

    if !is_simple_remaining || measures.is_empty() {
        return (vec![], modifier.to_string());
    }

    let cleaned = format!(
        "{}{}",
        &modifier[..full_match.start()],
        &modifier[full_match.end()..]
    )
    .trim()
    .to_string();

    (measures, cleaned)
}

fn is_temperature_unit(unit: &unit::Unit) -> bool {
    matches!(unit, unit::Unit::Fahrenheit | unit::Unit::Celsius)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    // Fully wrapped: outer parens are stripped.
    #[case::simple("(sifted)", Some("sifted"))]
    #[case::with_percent("(70% cacao)", Some("70% cacao"))]
    #[case::inner_trimmed("(  softened  )", Some("softened"))]
    // Not wrapped, or only partially: left untouched.
    #[case::plain("softened", Some("softened"))]
    #[case::open_only("(partial", Some("(partial"))]
    #[case::close_only("partial)", Some("partial)"))]
    // Internal parens must NOT be collapsed (would merge distinct clauses).
    #[case::two_groups("(a) and (b)", Some("(a) and (b)"))]
    #[case::nested("(note (nested))", Some("(note (nested))"))]
    // An empty group collapses away entirely.
    #[case::empty("()", None)]
    fn test_strip_wrapping_parens(#[case] input: &str, #[case] expected: Option<&str>) {
        assert_eq!(
            strip_wrapping_parens(Some(input.to_string())),
            expected.map(str::to_string)
        );
    }

    #[test]
    fn test_strip_wrapping_parens_none() {
        assert_eq!(strip_wrapping_parens(None), None);
    }

    #[rstest]
    // Descriptive aside flanked by name words → lifted out.
    #[case::temp(
        "room-temperature (70° to 80°F) water",
        Some(("room-temperature water", "70° to 80°F"))
    )]
    #[case::distance(
        "sliced (¼ inch / 6 mm) green onions",
        Some(("sliced green onions", "¼ inch / 6 mm"))
    )]
    // Mass/volume parenthetical → left for the amount path.
    #[case::mass("flour (190 grams) sifted", None)]
    // Count + size ("4 (½-inch) slices"): digit before paren, not a name word.
    #[case::count_size("4 (½-inch) slices pork", None)]
    // Trailing paren (no name text after) → left for other paths.
    #[case::trailing("warm water (100°F)", None)]
    // Leading paren (optional-ingredient shape) → untouched.
    #[case::leading("(70°F) water", None)]
    fn test_lift_inline_descriptive_paren(
        #[case] input: &str,
        #[case] expected: Option<(&str, &str)>,
    ) {
        let got = lift_inline_descriptive_paren(input);
        assert_eq!(
            got,
            expected.map(|(c, a)| (c.to_string(), a.to_string())),
            "input: {input}"
        );
    }
}
