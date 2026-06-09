use thiserror::Error;

/// Error types for ingredient parsing operations
#[derive(Error, Debug, Clone, PartialEq)]
pub enum IngredientError {
    /// Failed to parse ingredient string
    #[deprecated(note = "never produced by this crate; from_str cannot fail")]
    #[error("Failed to parse ingredient '{input}': {context}")]
    ParseError { input: String, context: String },
    /// Failed to parse measurement/amount
    #[error("Failed to parse amount '{input}': {reason}")]
    AmountParseError { input: String, reason: String },
    /// Measure operation error (adding incompatible measures, etc.)
    #[error("Measure operation '{operation}' failed: {reason}")]
    MeasureError { operation: String, reason: String },
    /// Generic parsing error with context
    #[deprecated(note = "never produced by this crate")]
    #[error("Ingredient parsing error: {message}")]
    Generic { message: String },
}

/// Result type for ingredient parsing operations
pub type IngredientResult<T> = Result<T, IngredientError>;
