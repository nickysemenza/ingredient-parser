//! Tests for Ingredient and Error types

#![allow(clippy::unwrap_used, clippy::panic)]

use ingredient::{ingredient::Ingredient, unit::Measure, IngredientError, IngredientResult};

// ============================================================================
// Ingredient Struct Tests
// ============================================================================

#[test]
fn test_ingredient_struct() {
    // try_from parsing
    let ingredient = Ingredient::from("2 cups flour");
    assert_eq!(ingredient.name, "flour");
    assert_eq!(ingredient.amounts.len(), 1);
    assert_eq!(ingredient.modifier, None);

    // Default trait
    let default = Ingredient::default();
    assert_eq!(default.name, "");
    assert_eq!(default.amounts.len(), 0);
    assert_eq!(default.modifier, None);

    // Clone and PartialEq
    let ingredient1 = Ingredient::new("flour", vec![Measure::new("cups", 2.0)], Some("sifted"));
    let ingredient2 = ingredient1.clone();
    assert_eq!(ingredient1, ingredient2);

    let ingredient3 = Ingredient::new("sugar", vec![Measure::new("cups", 2.0)], Some("sifted"));
    assert_ne!(ingredient1, ingredient3);
}

#[test]
fn test_ingredient_display() {
    let test_cases: Vec<(Ingredient, &str)> = vec![
        // With amounts
        (
            Ingredient::new("flour", vec![Measure::new("cups", 2.0)], None),
            "2 cups flour",
        ),
        // With modifier
        (
            Ingredient::new("flour", vec![Measure::new("cups", 2.0)], Some("sifted")),
            "2 cups flour, sifted",
        ),
        // Multiple amounts
        (
            Ingredient::new(
                "water",
                vec![Measure::new("cup", 1.0), Measure::new("ml", 240.0)],
                None,
            ),
            "1 cup / 240 ml water",
        ),
        // No amounts
        (
            Ingredient::new("salt", vec![], Some("to taste")),
            "n/a salt, to taste",
        ),
    ];

    for (ingredient, expected) in test_cases {
        assert_eq!(ingredient.to_string(), expected);
    }
}

// ============================================================================
// Error Type Tests
// ============================================================================

#[test]
fn test_ingredient_error() {
    // Display for each error variant
    let error_cases: Vec<(IngredientError, &str)> = vec![
        (
            IngredientError::ParseError {
                input: "bad input".to_string(),
                context: "invalid format".to_string(),
            },
            "Failed to parse ingredient 'bad input': invalid format",
        ),
        (
            IngredientError::AmountParseError {
                input: "2x cups".to_string(),
                reason: "unexpected character".to_string(),
            },
            "Failed to parse amount '2x cups': unexpected character",
        ),
        (
            IngredientError::MeasureError {
                operation: "add".to_string(),
                reason: "incompatible units".to_string(),
            },
            "Measure operation 'add' failed: incompatible units",
        ),
        (
            IngredientError::Generic {
                message: "something went wrong".to_string(),
            },
            "Ingredient parsing error: something went wrong",
        ),
    ];

    for (err, expected) in error_cases {
        assert_eq!(err.to_string(), expected);
    }

    // Clone and PartialEq
    let err1 = IngredientError::ParseError {
        input: "test".to_string(),
        context: "test context".to_string(),
    };
    let err2 = err1.clone();
    assert_eq!(err1, err2);

    let err3 = IngredientError::Generic {
        message: "different error".to_string(),
    };
    assert_ne!(err1, err3);

    // IngredientResult type alias
    let result: IngredientResult<i32> = Err(IngredientError::Generic {
        message: "error".to_string(),
    });
    assert!(result.is_err());
}
