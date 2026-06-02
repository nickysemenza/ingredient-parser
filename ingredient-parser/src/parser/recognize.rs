//! Whole-line special-form recognizers.
//!
//! Before the general grammar runs, a few ingredient lines have a shape the
//! grammar can't capture directly: a fully parenthesized "(optional)" ingredient,
//! a trailing "Name — AMOUNT" form, or an "X of/from N item" construction. Each
//! recognizer returns `Some(Ingredient)` when it matches and `None` to fall
//! through to the next recognizer / the core parse. Also houses
//! `lift_inline_descriptive_paren`, which pulls a descriptive parenthetical out
//! from between name words before the core parse.

use crate::parser::MeasurementParser;
use crate::unit;
use crate::{Ingredient, IngredientParser};

impl IngredientParser {
    /// Try to parse an optional ingredient format: "(amount ingredient, modifier)"
    ///
    /// When an entire ingredient line is wrapped in parentheses, it indicates
    /// the ingredient is optional. This is common in cookbooks like Joy of Cooking.
    pub(super) fn try_parse_optional_ingredient(&self, input: &str) -> Option<Ingredient> {
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
    pub(super) fn try_parse_trailing_amount_format(&self, input: &str) -> Option<Ingredient> {
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
    /// "Grated zest of 2 limes", "Finely grated zest from 1 lemon", "Peel of 1
    /// grapefruit", "Seeds scraped from 1 vanilla bean", or "Leaves from 3 sprigs
    /// thyme". These describe a component derived from a countable item; the item
    /// becomes the name (with its count), and the leading phrase ("juice of",
    /// "seeds scraped from", ...) moves into the modifier.
    pub(super) fn try_parse_x_of_construction(&self, input: &str) -> Option<Ingredient> {
        let trimmed = input.trim();

        // Find the leading "… of " / "… from " clause whose pivot is immediately
        // followed by a number (e.g. "Seeds scraped from 1 …"). Requiring the
        // number keeps normal names with "of"/"from" (e.g. "cream of tartar",
        // "heart of palm") from being captured. Use the LAST such pivot before a
        // number so multi-word leads ("finely grated zest of") are kept whole.
        let lower = trimmed.to_lowercase();
        let pivot_end = [" of ", " from "]
            .iter()
            .filter_map(|sep| {
                lower.find(sep).and_then(|pos| {
                    let after = pos + sep.len();
                    // A number must follow the separator: a digit/vulgar fraction
                    // or a spelled-out count ("one lemon"). This keeps normal
                    // names with "of"/"from" (e.g. "cream of tartar", "heart of
                    // palm") from being captured.
                    let tail = &trimmed[after..];
                    let starts_number = tail
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_ascii_digit() || crate::fraction::is_vulgar(c))
                        || crate::parser::text_number(tail).is_ok();
                    starts_number.then_some(after)
                })
            })
            .min()?;

        let phrase = trimmed[..pivot_end].trim();
        // Guard against a bare leading pivot ("of 1 lemon") with no descriptor.
        if phrase.is_empty() || phrase.split_whitespace().count() > 5 {
            return None;
        }

        let rest = trimmed[pivot_end..].trim_start();
        let mut parsed = self.parse_core_ingredient(rest)?;

        // Only treat this as the construction when the remainder actually carried a
        // quantity and an item (e.g. "1 lemon"); otherwise fall through to normal
        // parsing so "zest of lemon" (no count) stays name-only.
        if parsed.amounts.is_empty() || parsed.name.trim().is_empty() {
            return None;
        }

        let phrase_lower = phrase.to_lowercase();
        parsed.modifier = match parsed.modifier.take() {
            Some(existing) if !existing.trim().is_empty() => {
                Some(format!("{phrase_lower}, {existing}"))
            }
            _ => Some(phrase_lower),
        };
        Some(parsed)
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
pub(super) fn lift_inline_descriptive_paren(input: &str) -> Option<(String, String)> {
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

fn is_temperature_unit(unit: &unit::Unit) -> bool {
    matches!(unit, unit::Unit::Fahrenheit | unit::Unit::Celsius)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

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
