use std::{convert::TryFrom, fmt};

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
/// ```
pub struct Ingredient {
    /// The main ingredient name
    pub name: String,
    /// Vector of measurements with units
    pub amounts: Vec<Measure>,
    /// Optional preparation instructions or modifiers
    pub modifier: Option<String>,
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

        write!(f, "{}{}{}", amount_list, self.name, modifier)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::unit::Measure;

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
            amounts: vec![Measure::parse_new("cups", 2.0)],
            modifier: None,
        };
        assert_eq!(ingredient.to_string(), "2 cups flour");

        // With modifier
        let ingredient = Ingredient {
            name: "flour".to_string(),
            amounts: vec![Measure::parse_new("cups", 2.0)],
            modifier: Some("sifted".to_string()),
        };
        assert_eq!(ingredient.to_string(), "2 cups flour, sifted");

        // Multiple amounts
        let ingredient = Ingredient {
            name: "water".to_string(),
            amounts: vec![
                Measure::parse_new("cup", 1.0),
                Measure::parse_new("ml", 240.0),
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
            amounts: vec![Measure::parse_new("cups", 2.0)],
            modifier: Some("sifted".to_string()),
        };
        
        let ingredient2 = ingredient1.clone();
        assert_eq!(ingredient1, ingredient2);
        
        let ingredient3 = Ingredient {
            name: "sugar".to_string(),
            amounts: vec![Measure::parse_new("cups", 2.0)],
            modifier: Some("sifted".to_string()),
        };
        assert_ne!(ingredient1, ingredient3);
    }
}
