use std::fmt;

use crate::usage::{IngredientUsage, classify_usage};
use crate::{ParseNotes, from_str, unit::Measure};
use serde::{Deserialize, Serialize};

// `PartialEq`/`PartialOrd` are hand-written below (excluding `parse_notes`), so
// they're intentionally absent from this derive list.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
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
/// * `usage` - The role the line declares (e.g., "oil, for frying" → `FryingMedium`)
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
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub optional: bool,
    /// The role this line plays in the recipe (frying medium, garnish, …).
    /// Required on purpose — no serde default — so stale serialized data fails
    /// loudly instead of silently reading as `Normal`.
    pub usage: IngredientUsage,
    /// Non-failing metadata about *how* this line parsed (confidence, fallback,
    /// unparsed-digit). Runtime-only: `#[serde(skip)]` because it's derived on
    /// every parse and crosses to TypeScript via `WIngredient`, not the core
    /// JSON; and it's excluded from `PartialEq`/`PartialOrd` because it
    /// describes how we parsed, not what — two ingredients with identical data
    /// are equal regardless of their notes.
    #[serde(skip)]
    pub parse_notes: ParseNotes,
}

// Identity is the parsed *data* only; `parse_notes` (parse metadata) is excluded
// so a hand-built `Ingredient::new(...)` equals the parse of the same line and
// the corpus/test `assert_eq!`s compare data, not provenance.
//
// Both impls **exhaustively destructure** `Self` with no `..`: adding a field to
// `Ingredient` is then a compile error here until the author consciously decides
// whether it's part of identity (compare it) or metadata (`field: _`). This is
// the guard against the classic hand-written-`eq` footgun where a new field is
// silently ignored and unequal values compare equal.
impl PartialEq for Ingredient {
    fn eq(&self, other: &Self) -> bool {
        let Self {
            name,
            amounts,
            modifier,
            optional,
            usage,
            parse_notes: _,
        } = self;
        *name == other.name
            && *amounts == other.amounts
            && *modifier == other.modifier
            && *optional == other.optional
            && *usage == other.usage
    }
}

impl PartialOrd for Ingredient {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        let Self {
            name,
            amounts,
            modifier,
            optional,
            usage,
            parse_notes: _,
        } = self;
        (name, amounts, modifier, optional, usage).partial_cmp(&(
            &other.name,
            &other.amounts,
            &other.modifier,
            &other.optional,
            &other.usage,
        ))
    }
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
            // Classify here so a hand-built ingredient equals the parse of the
            // equivalent line (`Ingredient::new("oil", …, Some("for frying"))`
            // == `from_str("oil, for frying")`).
            usage: classify_usage(name, modifier, None, None),
            parse_notes: ParseNotes::default(),
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
            usage: classify_usage(name, modifier, None, None),
            parse_notes: ParseNotes::default(),
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
            usage: IngredientUsage::Normal,
            parse_notes: Default::default(),
        };
        assert_eq!(ingredient.to_string(), "2 cups flour");

        // With modifier
        let ingredient = Ingredient {
            name: "flour".to_string(),
            amounts: vec![Measure::new("cups", 2.0)],
            modifier: Some("sifted".to_string()),
            optional: false,
            usage: IngredientUsage::Normal,
            parse_notes: Default::default(),
        };
        assert_eq!(ingredient.to_string(), "2 cups flour, sifted");

        // Multiple amounts
        let ingredient = Ingredient {
            name: "water".to_string(),
            amounts: vec![Measure::new("cup", 1.0), Measure::new("ml", 240.0)],
            modifier: None,
            optional: false,
            usage: IngredientUsage::Normal,
            parse_notes: Default::default(),
        };
        assert_eq!(ingredient.to_string(), "1 cup / 240 ml water");

        // No amounts
        let ingredient = Ingredient {
            name: "salt".to_string(),
            amounts: vec![],
            modifier: Some("to taste".to_string()),
            optional: false,
            usage: IngredientUsage::Normal,
            parse_notes: Default::default(),
        };
        assert_eq!(ingredient.to_string(), "n/a salt, to taste");

        // Optional ingredient
        let ingredient = Ingredient {
            name: "walnuts".to_string(),
            amounts: vec![Measure::new("cup", 0.5)],
            modifier: Some("chopped".to_string()),
            optional: true,
            usage: IngredientUsage::Normal,
            parse_notes: Default::default(),
        };
        assert_eq!(ingredient.to_string(), "½ cup walnuts, chopped (optional)");
    }

    #[test]
    fn parse_notes_excluded_from_identity() {
        use crate::Confidence;
        // Two ingredients with identical parsed data but different parse notes
        // are equal — identity is the data, not how it was reached. (The
        // exhaustive destructure in `PartialEq` is the compile-time guard; this
        // locks the runtime behavior.)
        let mut a = Ingredient::new("flour", vec![Measure::new("cup", 1.0)], None);
        let mut b = a.clone();
        a.parse_notes = ParseNotes {
            confidence: Confidence::High,
            fell_back: false,
            unparsed_digit: false,
        };
        b.parse_notes = ParseNotes {
            confidence: Confidence::Low,
            fell_back: true,
            unparsed_digit: true,
        };
        assert_eq!(a, b, "differing only in parse_notes must stay equal");

        // But a genuine data difference is not equal.
        let c = Ingredient::new("sugar", vec![Measure::new("cup", 1.0)], None);
        assert_ne!(a, c, "a real name difference must be unequal");
    }
}
