//! Property-based tests to ensure parser robustness
//!
//! These tests generate random inputs and verify that the parser:
//! 1. Never panics
//! 2. Always produces valid output structures
//! 3. Maintains expected invariants

#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use ingredient::from_str;
use proptest::prelude::*;

// Generate reasonable ingredient strings for testing
prop_compose! {
    fn arb_ingredient_string()(
        amount in prop::option::of("[0-9]+\\.?[0-9]*"),
        unit in prop::option::of(r"[a-zA-Z]+"),
        name in r"[a-zA-Z ]+",
        modifier in prop::option::of(r", [a-zA-Z ]+")
    ) -> String {
        let mut result = String::new();

        if let Some(amount) = amount {
            result.push_str(&amount);
            result.push(' ');
        }

        if let Some(unit) = unit {
            result.push_str(&unit);
            result.push(' ');
        }

        result.push_str(&name);

        if let Some(modifier) = modifier {
            result.push_str(&modifier);
        }

        result
    }
}

// Generate arbitrary text strings
prop_compose! {
    fn arb_text_input()(input in r"[a-zA-Z0-9 .,;/\-\(\)½¼¾]*") -> String {
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
