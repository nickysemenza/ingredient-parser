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
#[allow(deprecated)]
use nom::{
    bytes::complete::tag,
    character::complete::{not_line_ending, space0, space1},
    combinator::{opt, verify},
    error::context,
    multi::many1,
    Parser,
};
use parser::{text, unitamt, MeasurementParser, Res};
use unit::Measure;

#[cfg(feature = "serde-derive")]
#[macro_use]
extern crate serde;

pub mod error;
mod fraction;
pub mod ingredient;
pub mod parser;
pub mod rich_text;
pub mod trace;
pub mod unit;
pub mod util;

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
        let units: Vec<String> = vec![
            // Non-standard units that aren't really convertible for the most part.
            // Note: "whole" is NOT included here because it's a built-in Unit::Whole.
            // Including it here would cause unit_extra() to incorrectly parse "whole wheat flour"
            // as having unit "whole" instead of treating "whole wheat" as part of the name.
            "packet", "sticks", "stick", "cloves", "clove", "bunch", "head", "large", "pinch",
            "small", "medium", "package", "recipe", "slice", "standard", "can", "leaf", "leaves",
            "strand", "tin",
        ]
        .iter()
        .map(|&s| s.into())
        .collect();
        let adjectives: Vec<String> = [
            "chopped",
            "minced",
            "diced",
            "freshly ground",
            "freshly grated",
            "finely chopped",
            "thinly sliced",
            "sliced",
            "plain",
            "to taste",
        ]
        .iter()
        .map(|&s| s.into())
        .collect();
        IngredientParser {
            units: HashSet::from_iter(units.iter().cloned()),
            adjectives: HashSet::from_iter(adjectives.iter().cloned()),
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

    /// Add custom adjectives to the parser (chainable)
    ///
    /// Adjectives are extracted from ingredient names and moved to the modifier field.
    ///
    /// # Example
    /// ```
    /// use ingredient::IngredientParser;
    ///
    /// let parser = IngredientParser::new()
    ///     .with_adjectives(&["roughly chopped", "finely diced"]);
    ///
    /// let ingredient = parser.from_str("1 cup roughly chopped onion");
    /// assert_eq!(ingredient.name, "onion");
    /// assert_eq!(ingredient.modifier, Some("roughly chopped".to_string()));
    /// ```
    pub fn with_adjectives(mut self, adjectives: &[&str]) -> Self {
        for adjective in adjectives {
            self.adjectives.insert((*adjective).to_string());
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
        match self.parse_ingredient(input) {
            Ok((_, ingredient)) => ingredient,
            Err(_) => {
                // Fallback: create an ingredient with just the name if parsing fails completely
                Ingredient {
                    name: input.trim().to_string(),
                    amounts: vec![],
                    modifier: None,
                }
            }
        }
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
    /// - `result`: The parsed [`Ingredient`] or error message
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
        use trace::{disable_tracing, enable_tracing};

        // Enable trace collection
        enable_tracing();

        // Parse the ingredient
        let result = match self.parse_ingredient(input) {
            Ok((_, ingredient)) => Ok(ingredient),
            Err(e) => Err(format!("{e:?}")),
        };

        // Collect the trace
        let trace = disable_tracing(input);

        trace::ParseWithTrace { result, trace }
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
                reason: format!("{e:?}"),
            }),
        }
    }

    /// Parse a complete ingredient line including amounts, name, and modifiers
    ///
    /// This handles formats like:
    /// - "2 cups flour"
    /// - "1-2 tbsp sugar, sifted"
    /// - "3 large eggs"
    /// - "1 cup (240ml) milk, room temperature"
    ///
    /// Supported formats include:
    /// * 1 g name
    /// * 1 g / 1g name, modifier
    /// * 1 g; 1 g name
    /// * ¼ g name
    /// * 1/4 g name
    /// * 1 ¼ g name
    /// * 1 1/4 g name
    /// * 1 g (1 g) name
    /// * 1 g name (about 1 g; 1 g)
    /// * name
    /// * 1 name
    #[tracing::instrument(name = "parse_ingredient")]
    pub(crate) fn parse_ingredient<'a>(&self, input: &'a str) -> Res<&'a str, Ingredient> {
        use trace::{trace_enter, trace_exit_failure, trace_exit_success};

        trace_enter("parse_ingredient", input);

        let mp = MeasurementParser::new(&self.units, self.is_rich_text);

        // Define the overall structure of an ingredient line
        let ingredient_format = (
            // Measurements at the beginning (optional)
            opt(|a| mp.parse_measurement_list(a)),
            // Space between measurements and name
            space0,
            // Optional adjective with required space after it
            opt((|a| self.adjective(a), space1)),
            // Name component - can be multiple words
            opt(many1(text)),
            // Optional measurements in parentheses after name
            opt(|a| mp.parse_parenthesized_amounts(a)),
            // Optional comma before modifier
            opt(tag(", ")),
            // Modifier - everything until end of line
            not_line_ending,
        );

        let result = context("ingredient", ingredient_format).parse(input).map(
            |(
                next_input,
                (primary_amounts, _, adjective, name_chunks, paren_amounts, _, modifier_text),
            )| {
                // Start with modifier from the trailing text
                let mut modifiers: String = modifier_text.to_owned();

                // Add adjective to modifiers if present
                if let Some((adj, _)) = adjective {
                    modifiers.push_str(&adj);
                }

                // Process the ingredient name
                let mut name: String = name_chunks
                    .unwrap_or_default()
                    .join("")
                    .trim_matches(' ')
                    .to_string();

                // Extract any adjectives from the name and move them to modifiers
                // Sort by length descending to match longer adjectives first
                // (e.g., "thinly sliced" before "sliced")
                let mut found_adjectives: Vec<&String> = self
                    .adjectives
                    .iter()
                    .filter(|adj| name.contains(adj.as_str()))
                    .collect();
                found_adjectives.sort_by_key(|a| std::cmp::Reverse(a.len()));

                for adj in found_adjectives {
                    // Only extract if the adjective is still in the name
                    // (it may have been removed as part of a longer adjective)
                    if name.contains(adj.as_str()) {
                        if !modifiers.is_empty() {
                            modifiers.push_str(", ");
                        }
                        modifiers.push_str(adj);
                        name = name.replace(adj.as_str(), " ");
                    }
                }
                // Clean up multiple spaces
                let name = name.split_whitespace().collect::<Vec<_>>().join(" ");

                // Combine all measurements
                let amounts = match (primary_amounts, paren_amounts) {
                    (Some(primary), Some(parenthesized)) => {
                        primary.into_iter().chain(parenthesized).collect()
                    }
                    (Some(primary), None) => primary,
                    (None, Some(parenthesized)) => parenthesized,
                    (None, None) => Vec::new(),
                };

                // Create the Ingredient
                (
                    next_input,
                    Ingredient {
                        name,
                        amounts,
                        // Only include modifier if non-empty
                        modifier: if modifiers.is_empty() {
                            None
                        } else {
                            Some(modifiers)
                        },
                    },
                )
            },
        );

        // Record trace outcome
        match &result {
            Ok((remaining, ingredient)) => {
                let consumed = input.len() - remaining.len();
                trace_exit_success(consumed, &ingredient.name);
            }
            Err(_) => {
                trace_exit_failure("parse failed");
            }
        }

        result
    }

    /// Parse and validate an adjective string
    fn adjective<'a>(&self, input: &'a str) -> Res<&'a str, String> {
        traced_parser!(
            "adjective",
            input,
            context(
                "adjective",
                verify(unitamt, |s: &str| {
                    self.adjectives.contains(&s.to_lowercase())
                }),
            )
            .parse(input),
            |s: &String| s.clone(),
            "not an adjective"
        )
    }
}
