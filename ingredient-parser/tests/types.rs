//! Tests for Ingredient and Error types

#![allow(clippy::unwrap_used, clippy::panic)]

mod common;

use ingredient::{ingredient::Ingredient, unit::Measure, IngredientError, IngredientResult};

// ============================================================================
// Ingredient Struct Tests
// ============================================================================

#[test]
fn test_ingredient_try_from() {
    let ingredient = Ingredient::try_from("2 cups flour").unwrap();
    assert_eq!(ingredient.name, "flour");
    assert_eq!(ingredient.amounts.len(), 1);
    assert_eq!(ingredient.modifier, None);
}

#[test]
fn test_ingredient_display() {
    // With amounts
    let ingredient = Ingredient {
        name: "flour".to_string(),
        amounts: vec![Measure::new("cups", 2.0)],
        modifier: None,
    };
    assert_eq!(ingredient.to_string(), "2 cups flour");

    // With modifier
    let ingredient = Ingredient {
        name: "flour".to_string(),
        amounts: vec![Measure::new("cups", 2.0)],
        modifier: Some("sifted".to_string()),
    };
    assert_eq!(ingredient.to_string(), "2 cups flour, sifted");

    // Multiple amounts
    let ingredient = Ingredient {
        name: "water".to_string(),
        amounts: vec![
            Measure::new("cup", 1.0),
            Measure::new("ml", 240.0),
        ],
        modifier: None,
    };
    assert_eq!(ingredient.to_string(), "1 cup / 240 ml water");

    // No amounts
    let ingredient = Ingredient {
        name: "salt".to_string(),
        amounts: vec![],
        modifier: Some("to taste".to_string()),
    };
    assert_eq!(ingredient.to_string(), "n/a salt, to taste");
}

#[test]
fn test_ingredient_default() {
    let ingredient = Ingredient::default();
    assert_eq!(ingredient.name, "");
    assert_eq!(ingredient.amounts.len(), 0);
    assert_eq!(ingredient.modifier, None);
}

#[test]
fn test_ingredient_clone_and_partial_eq() {
    let ingredient1 = Ingredient {
        name: "flour".to_string(),
        amounts: vec![Measure::new("cups", 2.0)],
        modifier: Some("sifted".to_string()),
    };

    let ingredient2 = ingredient1.clone();
    assert_eq!(ingredient1, ingredient2);

    let ingredient3 = Ingredient {
        name: "sugar".to_string(),
        amounts: vec![Measure::new("cups", 2.0)],
        modifier: Some("sifted".to_string()),
    };
    assert_ne!(ingredient1, ingredient3);
}

// ============================================================================
// Error Type Tests
// ============================================================================

#[test]
fn test_ingredient_error_display() {
    let err = IngredientError::ParseError {
        input: "bad input".to_string(),
        context: "invalid format".to_string(),
    };
    assert_eq!(
        err.to_string(),
        "Failed to parse ingredient 'bad input': invalid format"
    );

    let err = IngredientError::AmountParseError {
        input: "2x cups".to_string(),
        reason: "unexpected character".to_string(),
    };
    assert_eq!(
        err.to_string(),
        "Failed to parse amount '2x cups': unexpected character"
    );

    let err = IngredientError::MeasureError {
        operation: "add".to_string(),
        reason: "incompatible units".to_string(),
    };
    assert_eq!(
        err.to_string(),
        "Measure operation 'add' failed: incompatible units"
    );

    let err = IngredientError::Generic {
        message: "something went wrong".to_string(),
    };
    assert_eq!(
        err.to_string(),
        "Ingredient parsing error: something went wrong"
    );
}

#[test]
fn test_ingredient_error_clone_and_partial_eq() {
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
}

#[test]
fn test_from_anyhow_error() {
    let anyhow_err = anyhow::anyhow!("test error");
    let ingredient_err: IngredientError = anyhow_err.into();

    match ingredient_err {
        IngredientError::Generic { message } => {
            assert_eq!(message, "test error");
        }
        _ => panic!("Expected Generic error"),
    }
}

#[test]
fn test_ingredient_result_type() {
    let result: IngredientResult<i32> = Err(IngredientError::Generic {
        message: "error".to_string(),
    });
    assert!(result.is_err());
}
