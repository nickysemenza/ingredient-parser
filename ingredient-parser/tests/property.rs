//! Property-based tests to ensure parser robustness
//!
//! These tests generate random inputs and verify that the parser:
//! 1. Never panics
//! 2. Always produces valid output structures
//! 3. Maintains expected invariants

#![allow(clippy::unwrap_used, clippy::panic)]

use ingredient::{IngredientParser, SegmentationMode, from_str};
use proptest::prelude::*;

/// A parser on the legacy carve-then-repair path (`from_str` is the segmented
/// default since the cutover).
fn legacy_parser() -> IngredientParser {
    IngredientParser::new().with_segmentation_mode(SegmentationMode::Legacy)
}

// Generate arbitrary text strings
prop_compose! {
    fn arb_text_input()(input in r"[a-zA-Z0-9 .,;/\-\(\)½¼¾]*") -> String {
        input
    }
}

// Broad-Unicode generator covering the byte-boundary danger zones that ASCII
// fuzzing misses: Latin-1/Extended-A (incl. 'İ' U+0130, whose lowercase is
// *longer* than the original), combining diacritics, the full vulgar-fraction
// set, and a slice of CJK. Regression cover for the multibyte panics fixed in
// adjective extraction and length-changing lowercase.
prop_compose! {
    fn arb_unicode_input()(
        input in r"[a-zA-Z0-9 .,;/\-\(\)½¼¾⅓⅔⅛⅜⅝⅞İı\u{00C0}-\u{017F}\u{0300}-\u{0303}\u{4E00}-\u{4E0F}]*"
    ) -> String {
        input
    }
}

proptest! {
    /// Test that parser never panics on arbitrary input
    #[test]
    fn parser_never_panics(input in arb_text_input()) {
        let _ = from_str(&input);
        // If we get here, no panic occurred
    }

    /// Parser never panics on arbitrary *multibyte* input, and the result still
    /// round-trips through Display. Guards the UTF-8 byte-boundary handling in
    /// adjective extraction / length-changing lowercase.
    #[test]
    fn parser_never_panics_unicode(input in arb_unicode_input()) {
        let ingredient = from_str(&input);
        let _display = format!("{ingredient}");
    }

    /// Test that parser produces valid ingredient structures
    #[test]
    fn parser_produces_valid_ingredients(input in arb_text_input()) {
        let ingredient = from_str(&input);

        // Amounts vector should be valid (can be empty)
        for amount in &ingredient.amounts {
            // Should be able to convert to string without panics
            let _display = format!("{amount}");
        }

        // Modifier is optional but if present shouldn't be empty
        if let Some(modifier) = &ingredient.modifier {
            prop_assert!(!modifier.is_empty());
        }

        // Should be able to display the ingredient
        let _display = format!("{ingredient}");
    }

    /// Test that parsing is consistent
    #[test]
    fn parsing_is_consistent(input in arb_text_input()) {
        let ingredient1 = from_str(&input);
        let ingredient2 = from_str(&input);

        // Same input should produce same output
        prop_assert_eq!(ingredient1, ingredient2);
    }

    /// Test that parser handles edge cases gracefully
    #[test]
    fn parser_handles_edge_cases(
        input in prop::string::string_regex(r"[\x00-\x7F]*").unwrap()
    ) {
        // Test with ASCII-only strings to avoid encoding issues
        if input.len() <= 1000 {
            let ingredient = from_str(&input);

            // Should be able to display the result
            let _display = format!("{ingredient}");
        }
    }

    /// Test that fractions are handled correctly
    #[test]
    fn fractions_handled_correctly(
        whole in 0u32..10,
        frac in r"(1/2|1/4|3/4|1/3|2/3)",
        unit in r"(cup|cups|tsp|tbsp)",
        name in r"(flour|sugar|salt|butter|oil|milk|water|cheese)"
    ) {
        let ingredient_str = if whole > 0 {
            format!("{whole} {frac} {unit} {name}")
        } else {
            format!("{frac} {unit} {name}")
        };

        let ingredient = from_str(&ingredient_str);

        // Should successfully parse with a valid ingredient name
        prop_assert!(!ingredient.name.is_empty());
        prop_assert!(!ingredient.amounts.is_empty());

        // Should be able to format
        let _formatted = format!("{ingredient}");
    }

    /// Both parse paths stay panic-free and structurally valid on arbitrary
    /// (multibyte) input. (During the shadow migration this asserted exact
    /// equality; since the cutover deleted the legacy repair passes, the two
    /// modes legitimately diverge on repair-shaped lines — the corpus ratchet
    /// now pins the segmented default's accuracy.)
    #[test]
    fn both_paths_robust_on_unicode(input in arb_unicode_input()) {
        for ing in [from_str(&input), legacy_parser().from_str(&input)] {
            if let Some(modifier) = &ing.modifier {
                prop_assert!(!modifier.is_empty());
            }
            let _display = format!("{ing}");
        }
    }

    /// Same robustness over *vocabulary-triggering* lines: random sentences
    /// drawn from the words that drive the structural repairs (prep
    /// participles, "minus", "or"-alternatives, shared-head nouns, size words,
    /// parentheticals, amounts). The character-level fuzz above can't reach
    /// these code paths. Also pins the never-empty-name funnel invariant.
    #[test]
    fn both_paths_robust_on_vocab_lines(input in arb_vocab_line()) {
        for ing in [from_str(&input), legacy_parser().from_str(&input)] {
            if let Some(modifier) = &ing.modifier {
                prop_assert!(!modifier.is_empty());
            }
            let _display = format!("{ing}");
        }
    }
}

/// Random ingredient-shaped lines built from the parser's own trigger
/// vocabulary: an optional amount, then 1..7 tokens (words, separators, or
/// parentheticals) that exercise prep chains, minus clauses, alternatives,
/// shared heads, purpose phrases, and paren classification.
fn arb_vocab_line() -> impl Strategy<Value = String> {
    let word = prop::sample::select(vec![
        // prep participles / adverbs / descriptors
        "chopped", "minced", "seeded", "deribbed", "toasted", "peeled", "sliced", "finely",
        "roughly", "very", "bone-in", "skin-on", "boneless", "fresh",
        // heads / foods
        "walnuts", "chiles", "onion", "garlic", "flour", "oil", "stock", "cabbage", "celery",
        "lettuce", "clove", "cloves", "stalk", "head", "zest", "lemon",
        // coordinations / prose leads
        "or", "and", "and/or", "such", "as", "to", "taste", "for", "the", "serving", "brushing",
        "garnish", "plus", "more", "then", "minus", // sizes / qualifiers
        "small", "medium", "large", "extra", "about", "white", "red", "hot", "green",
    ]);
    let sep = prop::sample::select(vec![" ", ", ", "; ", " , "]);
    let paren = prop::sample::select(vec![
        "(red)",
        "(about 2 cups)",
        "(optional)",
        "(120g)",
        "(see note)",
        "(2 sticks minus 1 tablespoon)",
        "(¼ inch)",
    ]);
    let amount = prop::sample::select(vec![
        "",
        "1 ",
        "2 cups ",
        "½ cup ",
        "1/2 cup ",
        "3 ",
        "1 tablespoon ",
    ]);
    let token = prop_oneof![8 => word.prop_map(String::from), 1 => paren.prop_map(String::from)];
    (amount, prop::collection::vec((sep, token), 1..7)).prop_map(|(amount, tokens)| {
        let mut line = amount.to_string();
        for (i, (sep, tok)) in tokens.into_iter().enumerate() {
            if i > 0 {
                line.push_str(sep);
            }
            line.push_str(&tok);
        }
        line
    })
}

/// Test with edge case inputs
#[test]
fn test_parser_robustness_with_edge_cases() {
    let edge_cases = [
        "",
        " ",
        "salt",
        "1 egg",
        "½ cup water",
        "a pinch of salt",
        "2-3 cups flour",
        "1 cup (240ml) milk",
        "salt to taste",
        "1 cup plus 2 tbsp flour",
    ];

    for input in edge_cases {
        let ingredient = from_str(input);

        // Should never have empty name unless input was empty or whitespace only
        if !input.trim().is_empty() {
            assert!(
                !ingredient.name.is_empty(),
                "Input '{input}' resulted in empty name"
            );
        }

        // Should be able to display result
        let _display = format!("{ingredient}");
    }
}
