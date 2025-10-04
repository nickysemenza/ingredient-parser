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
                write!(f, "Failed to parse ingredient '{}': {}", input, context)
            }
            IngredientError::AmountParseError { input, reason } => {
                write!(f, "Failed to parse amount '{}': {}", input, reason)
            }
            IngredientError::MeasureError { operation, reason } => {
                write!(f, "Measure operation '{}' failed: {}", operation, reason)
            }
            IngredientError::Generic { message } => {
                write!(f, "Ingredient parsing error: {}", message)
            }
        }
    }
}

impl std::error::Error for IngredientError {}

/// Result type for ingredient parsing operations
pub type IngredientResult<T> = Result<T, IngredientError>;

/// Convert anyhow::Error to IngredientError
impl From<anyhow::Error> for IngredientError {
    fn from(err: anyhow::Error) -> Self {
        IngredientError::Generic {
            message: err.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
