//! Whole-line special-form recognizers.
//!
//! Before the general grammar runs, a few ingredient lines have a shape the
//! grammar can't capture directly: a fully parenthesized "(optional)" ingredient,
//! a trailing "Name — AMOUNT" form, or an "X of/from N item" construction. Each
//! recognizer returns `Some(Ingredient)` when it matches and `None` to fall
//! through to the next recognizer / the core parse.

use crate::parser::{MeasurementMode, MeasurementParser};
use crate::unit;
use crate::{Ingredient, IngredientParser};

impl IngredientParser {
    /// Try each whole-line special-form recognizer in order, returning the first
    /// that matches (or `None` to fall through to the core parse).
    pub(super) fn run_recognizers(&self, input: &str) -> Option<Ingredient> {
        RECOGNIZERS.iter().find_map(|recognizer| {
            let result = (recognizer.run)(self, input);
            crate::trace::trace_attempt(recognizer.id().as_str(), input, result, |ingredient| {
                ingredient.name.clone()
            })
        })
    }

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
        let mp = MeasurementParser::new(&self.units, MeasurementMode::IngredientList);

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

            return Some(Ingredient::from_parser_parts(
                name_part.trim(),
                amounts,
                None,
                false,
            ));
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
        // followed by a number (e.g. "Seeds scraped from 1 …"). Uses the EARLIEST
        // qualifying pivot across both separators.
        let lower = crate::parser::byte_aligned_lowercase(trimmed)?;
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

/// A whole-line special-form recognizer: maps a raw line to a finished
/// `Ingredient` when the line has its particular shape, else `None`.
type Recognizer = fn(&IngredientParser, &str) -> Option<Ingredient>;

crate::define_stage_pipeline! {
    pub(crate) enum RecognizerId,
    struct RecognizerEntry,
    const RECOGNIZERS: &[RecognizerEntry],
    type Recognizer = Recognizer,
    trace: pub(crate) RECOGNIZER_TRACE_NAMES,
    (OptionalWrapped, "optional_wrapped", IngredientParser::try_parse_optional_ingredient),
    (
        TrailingAmount,
        "trailing_amount",
        IngredientParser::try_parse_trailing_amount_format
    ),
    (
        XOfConstruction,
        "x_of_construction",
        IngredientParser::try_parse_x_of_construction
    ),
}

fn is_temperature_unit(unit: &unit::Unit) -> bool {
    matches!(unit, unit::Unit::Fahrenheit | unit::Unit::Celsius)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[test]
    fn recognizer_ids_are_unique() {
        crate::assert_stage_pipeline!(RECOGNIZERS);
    }

    // ── try_parse_optional_ingredient ───────────────────────────────────────
    #[rstest]
    // A fully parenthesized line is the optional form.
    #[case::wrapped("(1 cup walnuts)", true)]
    // No surrounding parens → fall through.
    #[case::not_wrapped("1 cup walnuts", false)]
    // Empty / whitespace-only parens carry no ingredient (the empty-name &
    // empty-amounts guard).
    #[case::empty_parens("()", false)]
    #[case::blank_parens("(   )", false)]
    fn optional_recognizer_matches(#[case] input: &str, #[case] matches: bool) {
        let got = IngredientParser::new().try_parse_optional_ingredient(input);
        assert_eq!(got.is_some(), matches, "input: {input}");
        if let Some(ing) = got {
            assert!(
                ing.optional,
                "matched optional form must set optional: {input}"
            );
        }
    }

    // ── try_parse_trailing_amount_format ────────────────────────────────────
    #[test]
    fn trailing_amount_basic() {
        let ing = IngredientParser::new()
            .try_parse_trailing_amount_format("Butter — 2 tablespoons")
            .expect("trailing em-dash amount should match");
        assert_eq!(ing.name, "Butter");
        assert_eq!(ing.amounts.len(), 1);
    }

    #[rstest]
    // No trailing-amount separator at all.
    #[case::no_separator("2 tablespoons butter")]
    // A trailing *temperature* describes a property, not a quantity, so the
    // recognizer must decline (the all-temperature guard) and let the line fall
    // through to the core parse.
    #[case::temp_only_f("Water — 100°F")]
    #[case::temp_only_c("Milk — 37°C")]
    fn trailing_amount_declines(#[case] input: &str) {
        assert!(
            IngredientParser::new()
                .try_parse_trailing_amount_format(input)
                .is_none(),
            "should not match: {input}"
        );
    }

    // ── try_parse_x_of_construction ─────────────────────────────────────────
    #[test]
    fn x_of_construction_basic() {
        let ing = IngredientParser::new()
            .try_parse_x_of_construction("Juice of 1 lemon")
            .expect("'X of N item' should match");
        assert_eq!(ing.name, "lemon");
        assert_eq!(ing.amounts.len(), 1);
        assert_eq!(ing.modifier.as_deref(), Some("juice of"));
    }

    #[test]
    fn x_of_construction_prepends_to_existing_modifier() {
        // The remainder carries its own modifier ("halved"); the leading phrase is
        // prepended, pinning the "<phrase>, <existing>" join format.
        let ing = IngredientParser::new()
            .try_parse_x_of_construction("Juice of 1 lemon, halved")
            .expect("should match");
        assert_eq!(ing.name, "lemon");
        assert_eq!(ing.modifier.as_deref(), Some("juice of, halved"));
    }

    #[rstest]
    // "of" not followed by a number is a normal name, not the construction.
    #[case::cream_of_tartar("cream of tartar")]
    // Bare leading pivot with no descriptor before "of".
    #[case::bare_pivot("of 1 lemon")]
    // No count/item after the pivot → not the construction ("zest of lemon").
    #[case::no_count("zest of lemon")]
    fn x_of_construction_declines(#[case] input: &str) {
        assert!(
            IngredientParser::new()
                .try_parse_x_of_construction(input)
                .is_none(),
            "should not match: {input}"
        );
    }
}
