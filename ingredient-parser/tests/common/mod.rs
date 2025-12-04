//! Shared test macros for ingredient-parser tests

#![allow(clippy::unwrap_used)]

/// Unified macro for ingredient parsing tests
///
/// # Variants
///
/// ## Test exact ingredient output
/// ```ignore
/// test_ingredient!(test_name, "2 cups flour", expected_ingredient);
/// ```
///
/// ## Test two inputs parse to equal results
/// ```ignore
/// test_ingredient!(eq: test_name, "1/2 cup", "0.5 cup");
/// ```
#[macro_export]
macro_rules! test_ingredient {
    ($test_name:ident, $input:expr, $expected:expr) => {
        #[test]
        fn $test_name() {
            assert_eq!(
                ingredient::IngredientParser::new(false).from_str($input),
                $expected
            );
        }
    };

    (eq: $test_name:ident, $left:expr, $right:expr) => {
        #[test]
        fn $test_name() {
            assert_eq!(
                ingredient::IngredientParser::new(false).from_str($left),
                ingredient::IngredientParser::new(false).from_str($right),
            );
        }
    };
}
