//! Public error text is a compatibility contract for library consumers.

use ingredient::IngredientError;

// ============================================================================
// Error Type Tests
// ============================================================================

#[test]
fn test_ingredient_error() {
    // Display for each variant the library actually produces.
    let error_cases: Vec<(IngredientError, &str)> = vec![
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
    ];

    for (err, expected) in error_cases {
        assert_eq!(err.to_string(), expected);
    }
}
