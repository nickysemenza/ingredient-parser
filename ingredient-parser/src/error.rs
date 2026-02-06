use std::fmt;

/// Error types for ingredient parsing operations
#[derive(Debug, Clone, PartialEq)]
pub enum IngredientError {
    /// Failed to parse ingredient string
    ParseError { input: String, context: String },
    /// Failed to parse measurement/amount
    AmountParseError { input: String, reason: String },
    /// Measure operation error (adding incompatible measures, etc.)
    MeasureError { operation: String, reason: String },
    /// Generic parsing error with context
    Generic { message: String },
}

impl fmt::Display for IngredientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IngredientError::ParseError { input, context } => {
                write!(f, "Failed to parse ingredient '{input}': {context}")
            }
            IngredientError::AmountParseError { input, reason } => {
                write!(f, "Failed to parse amount '{input}': {reason}")
            }
            IngredientError::MeasureError { operation, reason } => {
                write!(f, "Measure operation '{operation}' failed: {reason}")
            }
            IngredientError::Generic { message } => {
                write!(f, "Ingredient parsing error: {message}")
            }
        }
    }
}

impl std::error::Error for IngredientError {}

/// Result type for ingredient parsing operations
pub type IngredientResult<T> = Result<T, IngredientError>;
