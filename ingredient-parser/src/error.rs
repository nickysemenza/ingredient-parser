use thiserror::Error;

/// Error types for ingredient parsing operations.
///
/// Note: `from_str` is infallible by design (see lib.rs "Design Decisions"), so
/// the only variants here are the ones actually produced — by `parse_amount` and
/// measure arithmetic.
#[derive(Error, Debug, Clone, PartialEq)]
pub enum IngredientError {
    /// Failed to parse measurement/amount
    #[error("Failed to parse amount '{input}': {reason}")]
    AmountParseError { input: String, reason: String },
    /// Measure operation error (adding incompatible measures, etc.)
    #[error("Measure operation '{operation}' failed: {reason}")]
    MeasureError { operation: String, reason: String },
}

/// Result type for ingredient parsing operations
pub type IngredientResult<T> = Result<T, IngredientError>;
