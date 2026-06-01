use std::fmt;

use crate::{from_str, unit::Measure};

#[cfg_attr(feature = "serde-derive", derive(Serialize, Deserialize))]
#[derive(Clone, PartialEq, PartialOrd, Debug, Default)]
/// A parsed ingredient with structured components
///
/// This struct represents an ingredient that has been parsed from a text string
/// into its constituent parts: name, measurements, and optional modifiers.
///
/// # Fields
///
/// * `name` - The main ingredient name (e.g., "all-purpose flour", "olive oil")
/// * `amounts` - Vector of measurements with units (e.g., 2 cups, 150g)
/// * `modifier` - Optional preparation instructions (e.g., "sifted", "chopped", "room temperature")
/// * `optional` - Whether this ingredient is optional (wrapped in parentheses)
///
/// # Examples
///
/// ```
/// use ingredient::from_str;
///
/// let ingredient = from_str("2 cups all-purpose flour, sifted");
/// assert_eq!(ingredient.name, "all-purpose flour");
/// assert_eq!(ingredient.amounts.len(), 1);
/// assert_eq!(ingredient.modifier, Some("sifted".to_string()));
///
/// // Multiple measurements
/// let ingredient = from_str("1¼ cups / 155.5g flour");
/// assert_eq!(ingredient.amounts.len(), 2);
///
/// // No measurements
/// let ingredient = from_str("salt to taste");
/// assert_eq!(ingredient.name, "salt");
/// assert_eq!(ingredient.modifier, Some("to taste".to_string()));
///
/// // Optional ingredients (wrapped in parentheses)
/// let ingredient = from_str("(½ cup chopped walnuts)");
/// assert_eq!(ingredient.name, "walnuts");
/// assert!(ingredient.optional);
/// ```
pub struct Ingredient {
    /// The main ingredient name
    pub name: String,
    /// Vector of measurements with units
    pub amounts: Vec<Measure>,
    /// Optional preparation instructions or modifiers
    pub modifier: Option<String>,
    /// Whether this ingredient is optional (e.g., wrapped in parentheses)
    #[cfg_attr(
        feature = "serde-derive",
        serde(default, skip_serializing_if = "std::ops::Not::not")
    )]
    pub optional: bool,
}

impl Ingredient {
    /// Create a new ingredient with the given components
    ///
    /// # Arguments
    /// * `name` - The ingredient name
    /// * `amounts` - Vector of measurements
    /// * `modifier` - Optional preparation instructions
    ///
    /// # Example
    /// ```
    /// use ingredient::{ingredient::Ingredient, unit::Measure};
    ///
    /// let ingredient = Ingredient::new(
    ///     "flour",
    ///     vec![Measure::new("cups", 2.0)],
    ///     Some("sifted"),
    /// );
    /// assert_eq!(ingredient.name, "flour");
    /// ```
    pub fn new(name: &str, amounts: Vec<Measure>, modifier: Option<&str>) -> Self {
        Ingredient {
            name: name.to_string(),
            amounts,
            modifier: modifier.map(String::from),
            optional: false,
        }
    }

    /// Create a new optional ingredient with the given components
    ///
    /// # Example
    /// ```
    /// use ingredient::{ingredient::Ingredient, unit::Measure};
    ///
    /// let ingredient = Ingredient::new_optional(
    ///     "walnuts",
    ///     vec![Measure::new("cup", 0.5)],
    ///     Some("chopped"),
    /// );
    /// assert!(ingredient.optional);
    /// ```
    pub fn new_optional(name: &str, amounts: Vec<Measure>, modifier: Option<&str>) -> Self {
        Ingredient {
            name: name.to_string(),
            amounts,
            modifier: modifier.map(String::from),
            optional: true,
        }
    }
}

impl From<&str> for Ingredient {
    fn from(value: &str) -> Ingredient {
        from_str(value)
    }
}

impl std::str::FromStr for Ingredient {
    /// Parsing never fails — unparseable input falls back to a name-only
    /// ingredient — so the error type is [`Infallible`](std::convert::Infallible).
    type Err = std::convert::Infallible;

    /// Enables the idiomatic `str::parse` form alongside [`From<&str>`]:
    ///
    /// ```
    /// use ingredient::ingredient::Ingredient;
    ///
    /// let ing: Ingredient = "2 cups flour, sifted".parse().unwrap();
    /// assert_eq!(ing.name, "flour");
    /// ```
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(from_str(s))
    }
}

impl fmt::Display for Ingredient {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let amounts: Vec<String> = self.amounts.iter().map(|id| id.to_string()).collect();
        let modifier = self
            .modifier
            .as_ref()
            .map_or_else(String::new, |m| format!(", {m}"));

        let amount_list = if amounts.is_empty() {
            "n/a ".to_string()
        } else {
            format!("{} ", amounts.join(" / "))
        };

        let optional_marker = if self.optional { " (optional)" } else { "" };

        write!(
            f,
            "{}{}{}{}",
            amount_list, self.name, modifier, optional_marker
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::unit::Measure;

    #[test]
    fn test_ingredient_display() {
        // With amounts
        let ingredient = Ingredient {
            name: "flour".to_string(),
            amounts: vec![Measure::new("cups", 2.0)],
            modifier: None,
            optional: false,
        };
        assert_eq!(ingredient.to_string(), "2 cups flour");

        // With modifier
        let ingredient = Ingredient {
            name: "flour".to_string(),
            amounts: vec![Measure::new("cups", 2.0)],
            modifier: Some("sifted".to_string()),
            optional: false,
        };
        assert_eq!(ingredient.to_string(), "2 cups flour, sifted");

        // Multiple amounts
        let ingredient = Ingredient {
            name: "water".to_string(),
            amounts: vec![Measure::new("cup", 1.0), Measure::new("ml", 240.0)],
            modifier: None,
            optional: false,
        };
        assert_eq!(ingredient.to_string(), "1 cup / 240 ml water");

        // No amounts
        let ingredient = Ingredient {
            name: "salt".to_string(),
            amounts: vec![],
            modifier: Some("to taste".to_string()),
            optional: false,
        };
        assert_eq!(ingredient.to_string(), "n/a salt, to taste");

        // Optional ingredient
        let ingredient = Ingredient {
            name: "walnuts".to_string(),
            amounts: vec![Measure::new("cup", 0.5)],
            modifier: Some("chopped".to_string()),
            optional: true,
        };
        assert_eq!(
            ingredient.to_string(),
            "0.5 cup walnuts, chopped (optional)"
        );
    }
}
