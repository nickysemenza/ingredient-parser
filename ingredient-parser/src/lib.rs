//! # Ingredient Parser
//!
//! A Rust library for parsing recipe ingredient lines into structured data using
//! [nom](https://github.com/Geal/nom) parser combinators.
//!
//! ## Features
//!
//! - Parse complex ingredient strings with multiple units and values
//! - Support for fractions, ranges, and text numbers ("one", "a")
//! - Extract ingredient names and modifiers (preparation instructions)
//! - Handle common recipe notation and edge cases gracefully
//! - Support for Unicode fractions (½, ¼, etc.) in rich text mode
//! - Customizable units and adjectives
//!
//! ## Design Decisions
//!
//! ### Size descriptors are part of the ingredient name, not units
//!
//! Words like "large", "medium", "small" are treated as part of the ingredient name
//! rather than as units of measurement:
//!
//! ```text
//! "2 large eggs" → qty=2, unit=whole, name="large eggs"
//! "3 small potatoes" → qty=3, unit=whole, name="small potatoes"
//! ```
//!
//! **Rationale:**
//! - Size describes *which* variant of an ingredient, not *how much* (like "red onion" vs "yellow onion")
//! - "Large eggs" is a distinct product (different SKU, nutrition facts) from "medium eggs"
//! - Avoids needing blocklists for phrases like "medium heat" or "large pot"
//! - Normalization (e.g., "large eggs" → "eggs") is a separate concern for downstream consumers
//!
//! ### The "whole" unit means individual countable items
//!
//! When no unit is specified, items default to the "whole" unit meaning individual items:
//!
//! ```text
//! "2 eggs" → qty=2, unit=whole, name="eggs"
//! "1 whole chicken" → qty=1, unit=whole, name="chicken"
//! ```
//!
//! Note: "1 whole chicken" consumes "whole" as the unit, while "whole chicken" (no number)
//! keeps "whole" in the name. This is usually fine since "1 whole chicken" and "1 chicken"
//! mean the same thing.
//!
//! ### Preparation words become modifiers, not part of the name
//!
//! Words like "chopped", "diced", "minced", "sifted" are extracted into the `modifier` field:
//!
//! ```text
//! "2 cups flour, sifted" → name="flour", modifier="sifted"
//! "1 cup chopped onion" → name="onion", modifier="chopped"
//! ```
//!
//! This keeps the ingredient name clean for matching/normalization while preserving
//! preparation instructions.
//!
//! ### Multiple units are preserved as separate amounts
//!
//! When a recipe provides multiple unit formats, each becomes a separate entry in `amounts`:
//!
//! ```text
//! "1 cup / 240ml flour" → amounts=[1 cup, 240 ml], name="flour"
//! "150g | 1 cup sugar" → amounts=[150 g, 1 cup], name="sugar"
//! ```
//!
//! This preserves both metric and imperial measurements for downstream use.
//!
//! ### Ranges are a single Measure with upper bound
//!
//! Range expressions become one `Measure` with both `value` and `upper_value`:
//!
//! ```text
//! "2-3 cups flour" → amounts=[Measure { value: 2.0, upper_value: Some(3.0), unit: cup }]
//! "1 to 2 tablespoons" → same structure
//! ```
//!
//! This preserves the range semantics rather than creating two separate amounts.
//!
//! ### Parsing never fails - graceful fallback
//!
//! If an input can't be parsed as a structured ingredient, the entire input becomes the
//! ingredient name with empty amounts:
//!
//! ```text
//! "mystery ingredient xyz" → name="mystery ingredient xyz", amounts=[]
//! ```
//!
//! This ensures the parser always returns something useful rather than erroring.
//!
//! ### Rich text mode for parsing prose
//!
//! Two parsing modes exist:
//! - **Ingredient list mode** (default): Expects "amount unit ingredient" format
//! - **Rich text mode**: Parses measurements from recipe prose/instructions
//!
//! Rich text mode handles things like step numbers ("1. Bring a pot...") and embedded
//! measurements ("cook for 30 minutes at 350°F"). See [`rich_text::RichParser`] for details.
//!
//! ## Quick Start
//!
//! The simplest way to parse an ingredient is using [`from_str`]:
//!
//! ```
//! use ingredient::from_str;
//!
//! let ingredient = from_str("2 cups all-purpose flour, sifted");
//! assert_eq!(ingredient.name, "all-purpose flour");
//! assert_eq!(ingredient.amounts.len(), 1);
//! assert_eq!(ingredient.modifier, Some("sifted".to_string()));
//! ```
//!
//! ## Advanced Usage
//!
//! For more control, use [`IngredientParser`] directly:
//!
//! ```
//! use ingredient::IngredientParser;
//!
//! let parser = IngredientParser::new();
//! let ingredient = parser.from_str("1¼ cups / 155.5g flour");
//! assert_eq!(ingredient.amounts.len(), 2); // Multiple units parsed
//! ```

use std::collections::HashSet;

pub use crate::error::{IngredientError, IngredientResult};
pub use crate::ingredient::Ingredient;
use parser::MeasurementParser;
use unit::Measure;

#[cfg(feature = "serde-derive")]
#[macro_use]
extern crate serde;

// =============================================================================
// Default Adjective/Modifier Constants
// =============================================================================

/// Default preparation adjectives that get extracted to the modifier field.
/// These describe how an ingredient is prepared before use.
const DEFAULT_PREPARATION_ADJECTIVES: &[&str] = &[
    "chopped",
    "minced",
    "diced",
    "freshly ground",
    "freshly grated",
    "finely chopped",
    "coarsely chopped",
    "thinly sliced",
    "sliced",
    "plain",
    "to taste",
    // State/prep words that describe how an ingredient is brought to the recipe
    // (e.g. "melted butter", "softened butter"). Like other prep words they
    // belong in the modifier, not the name, whether they lead or trail.
    "melted",
    "softened",
    // Measurement/preparation qualifiers that often appear *before* the name
    // (e.g. "1 cup packed brown sugar", "2 cups sifted flour"). They describe how
    // the ingredient is measured/prepared, not which product it is, so they belong
    // in the modifier. Multi-word forms (e.g. "firmly packed") win over their
    // single-word substring ("packed") via the longest-match-first ordering in
    // extract_adjectives_from_name.
    "firmly packed",
    "loosely packed",
    "lightly packed",
    "packed",
    "sifted",
    // Temperature/state qualifier that describes how the ingredient should be,
    // not which product it is (e.g. "room-temperature butter"). Both spellings
    // reduce to the same modifier; the parser already pulls the *trailing* form
    // ("egg, room temperature") into the modifier, so this also covers the
    // *leading* form ("room-temperature water" → water, "room temperature").
    "room temperature",
    "room-temperature",
];

/// Default purpose phrases that get extracted to the modifier field.
/// These describe what the ingredient is used for (e.g., "for garnish").
const DEFAULT_PURPOSE_PHRASES: &[&str] = &[
    "for dusting",
    "for garnish",
    "for garnishing",
    "for serving",
    "for decoration",
    "for topping",
    "for dipping",
    "for drizzling",
    "for sprinkling",
    "for rolling",
    "for coating",
    "for frying",
    "for greasing",
];

pub mod error;
pub(crate) mod fraction;
pub mod ingredient;
pub(crate) mod parser;
pub mod rich_text;
pub mod trace;
pub mod unit;
pub mod unit_mapping;
pub mod util;

pub(crate) use parser::Res;

/// Parse an ingredient string using default settings
///
/// This is the simplest way to parse an ingredient string. It uses default
/// units and adjectives, and handles most common ingredient formats gracefully.
///
/// # Arguments
///
/// * `input` - The ingredient string to parse (e.g. "2 cups flour, sifted")
///
/// # Returns
///
/// An [`Ingredient`] struct containing the parsed name, amounts, and modifier.
/// If parsing fails completely, returns an ingredient with just the input as the name.
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
/// // Handles fractions and multiple units
/// let ingredient = from_str("1¼ cups / 155.5g flour");
/// assert_eq!(ingredient.amounts.len(), 2);
///
/// // Gracefully handles unparseable input
/// let ingredient = from_str("some weird ingredient");
/// assert_eq!(ingredient.name, "some weird ingredient");
/// ```
///
/// Use [`IngredientParser`] directly for more control over parsing behavior.
pub fn from_str(input: &str) -> Ingredient {
    IngredientParser::new().from_str(input)
}

/// Customizable ingredient parser with configurable units and adjectives
///
/// This parser allows you to customize which units and adjectives are recognized
/// during parsing. For parsing recipe instructions (with rich text support),
/// use [`RichParser`](crate::rich_text::RichParser) instead.
///
/// # Examples
///
/// ```
/// use ingredient::IngredientParser;
///
/// // Create parser with custom units
/// let parser = IngredientParser::new()
///     .with_units(&["handful", "handfuls"]);
///
/// let ingredient = parser.from_str("2 handfuls of nuts");
/// assert_eq!(ingredient.name, "nuts");
/// ```
#[derive(Clone, PartialEq, Debug, Default)]
pub struct IngredientParser {
    /// Set of recognized measurement units
    units: HashSet<String>,
    /// Set of recognized adjectives that get moved to modifier field
    adjectives: HashSet<String>,
    /// Whether to parse rich text characters (Unicode fractions, etc.)
    is_rich_text: bool,
}
impl IngredientParser {
    /// Create a new ingredient parser with default units and adjectives
    ///
    /// # Returns
    ///
    /// A new `IngredientParser` with sensible defaults for common cooking units
    /// and adjectives like "chopped", "minced", "sifted", etc.
    ///
    /// # Examples
    ///
    /// ```
    /// use ingredient::IngredientParser;
    ///
    /// // Standard parser
    /// let parser = IngredientParser::new();
    ///
    /// // With custom units
    /// let parser = IngredientParser::new()
    ///     .with_units(&["sprig", "sprigs"]);
    /// ```
    pub fn new() -> Self {
        // Non-standard units that aren't really convertible for the most part.
        // Note: "whole" is NOT included here because it's a built-in Unit::Whole.
        // Including it here would cause unit_extra() to incorrectly parse "whole wheat flour"
        // as having unit "whole" instead of treating "whole wheat" as part of the name.
        let units: HashSet<String> = [
            "recipe", "packet", "sticks", "stick", "cloves", "clove", "bunch", "head", "pinch",
            "package", "slice", "slices", "standard", "can", "leaf", "leaves", "strand", "tin",
            "rib", "ribs", "sprig", "sprigs", "pint", "pints",
        ]
        .iter()
        .map(|&s| s.into())
        .collect();

        // Combine preparation adjectives and purpose phrases
        let adjectives: HashSet<String> = DEFAULT_PREPARATION_ADJECTIVES
            .iter()
            .chain(DEFAULT_PURPOSE_PHRASES.iter())
            .map(|&s| s.into())
            .collect();

        IngredientParser {
            units,
            adjectives,
            is_rich_text: false,
        }
    }

    /// Add custom units to the parser (chainable)
    ///
    /// Note: You should add both singular and plural forms if applicable.
    ///
    /// # Example
    /// ```
    /// use ingredient::IngredientParser;
    ///
    /// let parser = IngredientParser::new()
    ///     .with_units(&["sprig", "sprigs"]);
    ///
    /// let ingredient = parser.from_str("3 sprigs thyme");
    /// assert_eq!(ingredient.name, "thyme");
    /// ```
    pub fn with_units(mut self, units: &[&str]) -> Self {
        for unit in units {
            self.units.insert((*unit).to_string());
        }
        self
    }

    /// Get a reference to the units set (crate-internal use only)
    pub(crate) fn units(&self) -> &HashSet<String> {
        &self.units
    }

    /// wrapper for [self.parse_ingredient]
    /// ```
    /// use ingredient::{from_str};
    /// assert_eq!(from_str("one whole egg").to_string(),"1 whole egg");
    /// ```
    /// Parse an ingredient string into an Ingredient object
    ///
    /// This method never panics and provides fallback behavior for unparseable input
    pub fn from_str(&self, input: &str) -> Ingredient {
        self.parse_ingredient_line(input)
    }

    /// Parse an ingredient string with debug tracing enabled
    ///
    /// This method returns both the parsed result and a trace of which
    /// parser functions were called, including which `alt()` branches
    /// were tried and their outcomes.
    ///
    /// # Arguments
    ///
    /// * `input` - The ingredient string to parse
    ///
    /// # Returns
    ///
    /// A [`ParseWithTrace`](trace::ParseWithTrace) containing:
    /// - `result`: The parsed [`Ingredient`], preserving [`from_str`] fallback behavior
    /// - `trace`: A [`ParseTrace`](trace::ParseTrace) that can be formatted as a tree
    ///
    /// # Examples
    ///
    /// ```
    /// use ingredient::IngredientParser;
    ///
    /// let parser = IngredientParser::new();
    /// let result = parser.parse_with_trace("2 cups flour");
    ///
    /// // Print the trace tree
    /// println!("{}", result.trace.format_tree(false));
    ///
    /// // Access the parsed ingredient
    /// if let Ok(ingredient) = result.result {
    ///     println!("Parsed: {}", ingredient.name);
    /// }
    /// ```
    pub fn parse_with_trace(&self, input: &str) -> trace::ParseWithTrace<Ingredient> {
        self.parse_ingredient_line_with_trace(input)
    }

    /// Parses one or two amounts, e.g. `12 grams` or `120 grams / 1 cup`. Used by [self.parse_ingredient].
    /// ```
    /// use ingredient::{IngredientParser,unit::Measure};
    /// let ip = IngredientParser::new();
    /// assert_eq!(
    ///    ip.parse_amount("120 grams").unwrap(),
    ///    vec![Measure::new("grams",120.0)]
    ///  );
    /// assert_eq!(
    ///    ip.parse_amount("120 grams / 1 cup").unwrap(),
    ///    vec![Measure::new("grams",120.0),Measure::new("cup", 1.0)]
    ///  );
    /// assert_eq!(
    ///    ip.parse_amount("120 grams / 1 cup / 1 whole").unwrap(),
    ///    vec![Measure::new("grams",120.0),Measure::new("cup", 1.0),Measure::new("whole", 1.0)]
    ///  );
    /// ```
    /// Parse a string containing one or more measurements
    ///
    /// Returns a Result with a Vec of Measures, or an error if parsing fails
    #[tracing::instrument(name = "parse_amount")]
    pub fn parse_amount(&self, input: &str) -> IngredientResult<Vec<Measure>> {
        let mp = MeasurementParser::new(&self.units, self.is_rich_text);
        match mp.parse_measurement_list(input) {
            Ok((_, measurements)) => Ok(measurements),
            Err(e) => Err(IngredientError::AmountParseError {
                input: input.to_string(),
                reason: match e {
                    nom::Err::Incomplete(_) => "incomplete input".to_string(),
                    nom::Err::Error(_) | nom::Err::Failure(_) => {
                        "no recognizable measurement found".to_string()
                    }
                },
            }),
        }
    }
}
