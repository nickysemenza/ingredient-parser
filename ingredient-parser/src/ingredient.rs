use std::{convert::TryFrom, fmt};

use crate::{from_str, unit::Measure};

/// Indicates how much structure the parser found in the input.
///
/// This is useful for classification tasks where you need to distinguish
/// between text that parsed as a structured ingredient vs text that the
/// parser couldn't extract structure from.
///
/// # Examples
///
/// ```
/// use ingredient::{from_str, ingredient::ParseQuality};
///
/// // Structured: has amounts
/// let ing = from_str("2 cups flour");
/// assert_eq!(ing.parse_quality(), ParseQuality::Structured);
///
/// // Structured: has modifier
/// let ing = from_str("flour, sifted");
/// assert_eq!(ing.parse_quality(), ParseQuality::Structured);
///
/// // Unstructured: just a name
/// let ing = from_str("salt");
/// assert_eq!(ing.parse_quality(), ParseQuality::Unstructured);
/// ```
#[cfg_attr(feature = "serde-derive", derive(Serialize, Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParseQuality {
    /// Parser found amounts, units, modifiers, or optional marker.
    /// High confidence this is an ingredient line.
    Structured,

    /// Parser returned the input as-is with no extracted structure.
    /// May or may not be an ingredient - needs other context.
    Unstructured,
}

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

    /// Returns the parse quality indicating how much structure was found.
    ///
    /// - `Structured`: Parser found amounts, modifiers, or optional marker
    /// - `Unstructured`: Parser just returned the input as the name
    ///
    /// This is useful for classification tasks where you need to distinguish
    /// ingredient lines from other text (titles, instructions, etc.).
    ///
    /// # Examples
    ///
    /// ```
    /// use ingredient::{from_str, ingredient::ParseQuality};
    ///
    /// // Has amounts -> Structured
    /// let ing = from_str("2 cups flour");
    /// assert_eq!(ing.parse_quality(), ParseQuality::Structured);
    ///
    /// // Has modifier -> Structured
    /// let ing = from_str("flour, sifted");
    /// assert_eq!(ing.parse_quality(), ParseQuality::Structured);
    ///
    /// // Optional ingredient -> Structured
    /// let ing = from_str("(walnuts)");
    /// assert_eq!(ing.parse_quality(), ParseQuality::Structured);
    ///
    /// // Just a name -> Unstructured
    /// let ing = from_str("salt");
    /// assert_eq!(ing.parse_quality(), ParseQuality::Unstructured);
    ///
    /// // Non-ingredient text -> Unstructured
    /// let ing = from_str("Chocolate Chip Cookies");
    /// assert_eq!(ing.parse_quality(), ParseQuality::Unstructured);
    /// ```
    pub fn parse_quality(&self) -> ParseQuality {
        if !self.amounts.is_empty() || self.modifier.is_some() || self.optional {
            ParseQuality::Structured
        } else {
            ParseQuality::Unstructured
        }
    }
}

impl TryFrom<&str> for Ingredient {
    type Error = String;
    fn try_from(value: &str) -> Result<Ingredient, Self::Error> {
        Ok(from_str(value))
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
    use rstest::rstest;

    #[rstest]
    #[case("2 cups flour", ParseQuality::Structured, "has amounts")]
    #[case("1/2 tsp salt", ParseQuality::Structured, "has amounts with fraction")]
    #[case("flour, sifted", ParseQuality::Structured, "has modifier")]
    #[case(
        "butter, at room temperature",
        ParseQuality::Structured,
        "has modifier phrase"
    )]
    #[case("(walnuts)", ParseQuality::Structured, "optional ingredient")]
    #[case(
        "(1/2 cup chopped nuts)",
        ParseQuality::Structured,
        "optional with amounts"
    )]
    #[case("salt", ParseQuality::Unstructured, "bare ingredient")]
    #[case("pepper", ParseQuality::Unstructured, "bare ingredient")]
    #[case("Chocolate Chip Cookies", ParseQuality::Unstructured, "recipe title")]
    #[case(
        "Add flour and mix well.",
        ParseQuality::Unstructured,
        "instruction text"
    )]
    #[case(
        "Preheat oven to 350°F",
        ParseQuality::Structured,
        "instruction with temp - parser finds °F modifier"
    )]
    #[case("FOR THE FILLING", ParseQuality::Unstructured, "section header")]
    fn test_parse_quality(
        #[case] input: &str,
        #[case] expected: ParseQuality,
        #[case] _description: &str,
    ) {
        let ingredient = from_str(input);
        assert_eq!(
            ingredient.parse_quality(),
            expected,
            "input: {:?}, got name={:?}, amounts={:?}, modifier={:?}, optional={}",
            input,
            ingredient.name,
            ingredient.amounts,
            ingredient.modifier,
            ingredient.optional
        );
    }

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
