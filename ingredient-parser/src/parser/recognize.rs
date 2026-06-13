//! Whole-line special-form recognizers.
//!
//! Before the general grammar runs, a few ingredient lines have a shape the
//! grammar can't capture directly: a fully parenthesized "(optional)" ingredient,
//! a trailing "Name — AMOUNT" form, or an "X of/from N item" construction. Each
//! recognizer returns `Some(Ingredient)` when it matches and `None` to fall
//! through to the next recognizer / the core parse.

use crate::parser::MeasurementParser;
use crate::unit;
use crate::usage::classify_usage;
use crate::{Ingredient, IngredientParser};

impl IngredientParser {
    /// Try each whole-line special-form recognizer in order, returning the first
    /// that matches (or `None` to fall through to the core parse).
    pub(super) fn run_recognizers(&self, input: &str) -> Option<Ingredient> {
        RECOGNIZERS.iter().find_map(|(name, recognize)| {
            if !crate::trace::is_tracing_enabled() {
                return recognize(self, input);
            }
            // Trace each recognizer attempt so the egui tree shows which matched
            // (and which were skipped).
            crate::trace::trace_enter(name, input);
            match recognize(self, input) {
                Some(ingredient) => {
                    crate::trace::trace_exit_success(0, &ingredient.name);
                    Some(ingredient)
                }
                None => {
                    crate::trace::trace_exit_failure("no match");
                    None
                }
            }
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
        let mp = MeasurementParser::new(&self.units, false);

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
                usage: classify_usage(name_part.trim(), None, Some(input), None),
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
        // followed by a number (e.g. "Seeds scraped from 1 …"). Uses the EARLIEST
        // qualifying pivot across both separators.
        let lower = trimmed.to_lowercase();
        // Lowercasing can change byte lengths for some Unicode (e.g. 'İ' ->
        // "i̇"), so offsets found in `lower` would misalign with `trimmed` and
        // slicing could split a char (panic). Bail for such rare inputs, the
        // same defense extract_adjectives_from_name uses.
        if lower.len() != trimmed.len() {
            return None;
        }
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

/// The ordered recognizer list, tried first-match before the core parse. Order
/// matters: the optional-wrapped check must precede the others (it strips the
/// outer parens), and x-of-construction is last (most permissive).
//
// TODO(parse_multi): an "X and Y" line with two distinct heads ("Kosher salt
// and freshly ground black pepper") — and a no-quantity "X or Y" contrast
// ("fresh or frozen blueberries") — is really TWO ingredients. There is no
// multi-ingredient splitter yet, so `from_str` returns a single Ingredient:
// refine's " and " guard keeps the and-line as one clean name (rather than
// doing mid-seam adjective surgery), and the or-line keeps its reconstructed
// primary + alternative. A future `parse_multi` recognizer would split these
// into a `Vec<Ingredient>`. Corpus rows for these carry a matching TODO.
const RECOGNIZERS: &[(&str, Recognizer)] = &[
    (
        "optional_wrapped",
        IngredientParser::try_parse_optional_ingredient,
    ),
    (
        "trailing_amount",
        IngredientParser::try_parse_trailing_amount_format,
    ),
    (
        "x_of_construction",
        IngredientParser::try_parse_x_of_construction,
    ),
];

fn is_temperature_unit(unit: &unit::Unit) -> bool {
    matches!(unit, unit::Unit::Fahrenheit | unit::Unit::Celsius)
}
