//! Whole-line special-form recognizers.
//!
//! Before the general grammar runs, a few ingredient lines have a shape the
//! grammar can't capture directly: a fully parenthesized "(optional)" ingredient,
//! a trailing "Name — AMOUNT" form, or an "X of/from N item" construction. Each
//! recognizer returns `Some(Ingredient)` when it matches and `None` to fall
//! through to the next recognizer / the core parse.

use crate::parser::{MeasurementMode, MeasurementParser};
use crate::unit;
use crate::usage::classify_usage;
use crate::{Ingredient, IngredientParser};

impl IngredientParser {
    /// Try each whole-line special-form recognizer in order, returning the first
    /// that matches (or `None` to fall through to the core parse).
    pub(super) fn run_recognizers(&self, input: &str) -> Option<Ingredient> {
        RECOGNIZERS.iter().find_map(|recognizer| {
            let _phase = recognizer.phase;
            if !crate::trace::is_tracing_enabled() {
                return (recognizer.run)(self, input);
            }
            // Trace each recognizer attempt so the egui tree shows which matched
            // (and which were skipped).
            crate::trace::trace_enter(recognizer.id.as_str(), input);
            match (recognizer.run)(self, input) {
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

            return Some(Ingredient {
                name: name_part.trim().to_string(),
                amounts,
                modifier: None,
                optional: false,
                usage: classify_usage(name_part.trim(), None, Some(input), None),
                // Overwritten at the parse funnel (`parse_ingredient_line`).
                parse_notes: Default::default(),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum RecognizerId {
    OptionalWrapped,
    TrailingAmount,
    XOfConstruction,
}

impl RecognizerId {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            RecognizerId::OptionalWrapped => "optional_wrapped",
            RecognizerId::TrailingAmount => "trailing_amount",
            RecognizerId::XOfConstruction => "x_of_construction",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum RecognizerPhase {
    Wrapper,
    AmountPosition,
    DerivedPart,
}

#[derive(Clone, Copy)]
struct RecognizerEntry {
    id: RecognizerId,
    phase: RecognizerPhase,
    run: Recognizer,
}

impl RecognizerEntry {
    const fn new(id: RecognizerId, phase: RecognizerPhase, run: Recognizer) -> Self {
        Self { id, phase, run }
    }
}

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
const RECOGNIZERS: &[RecognizerEntry] = &[
    RecognizerEntry::new(
        RecognizerId::OptionalWrapped,
        RecognizerPhase::Wrapper,
        IngredientParser::try_parse_optional_ingredient,
    ),
    RecognizerEntry::new(
        RecognizerId::TrailingAmount,
        RecognizerPhase::AmountPosition,
        IngredientParser::try_parse_trailing_amount_format,
    ),
    RecognizerEntry::new(
        RecognizerId::XOfConstruction,
        RecognizerPhase::DerivedPart,
        IngredientParser::try_parse_x_of_construction,
    ),
];

pub(crate) const RECOGNIZER_TRACE_NAMES: &[&str] = &[
    RecognizerId::OptionalWrapped.as_str(),
    RecognizerId::TrailingAmount.as_str(),
    RecognizerId::XOfConstruction.as_str(),
];

fn is_temperature_unit(unit: &unit::Unit) -> bool {
    matches!(unit, unit::Unit::Fahrenheit | unit::Unit::Celsius)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use rstest::rstest;
    use std::collections::HashSet;

    const EXPECTED_RECOGNIZERS: &[(RecognizerId, RecognizerPhase)] = &[
        (RecognizerId::OptionalWrapped, RecognizerPhase::Wrapper),
        (
            RecognizerId::TrailingAmount,
            RecognizerPhase::AmountPosition,
        ),
        (RecognizerId::XOfConstruction, RecognizerPhase::DerivedPart),
    ];

    const EXPECTED_RECOGNIZER_LABELS: &[&str] =
        &["optional_wrapped", "trailing_amount", "x_of_construction"];

    #[test]
    fn recognizer_order_is_locked() {
        let actual: Vec<_> = RECOGNIZERS
            .iter()
            .map(|recognizer| (recognizer.id, recognizer.phase))
            .collect();
        assert_eq!(actual, EXPECTED_RECOGNIZERS);
    }

    #[test]
    fn recognizer_ids_are_unique() {
        let ids: HashSet<_> = RECOGNIZERS.iter().map(|recognizer| recognizer.id).collect();
        assert_eq!(ids.len(), RECOGNIZERS.len());
    }

    #[test]
    fn recognizer_trace_labels_are_stable() {
        let labels: Vec<_> = RECOGNIZERS
            .iter()
            .map(|recognizer| recognizer.id.as_str())
            .collect();
        assert_eq!(labels, EXPECTED_RECOGNIZER_LABELS);
        assert_eq!(RECOGNIZER_TRACE_NAMES, EXPECTED_RECOGNIZER_LABELS);
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
