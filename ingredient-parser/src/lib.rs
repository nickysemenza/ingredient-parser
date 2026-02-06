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

use std::borrow::Cow;
use std::collections::HashSet;

pub use crate::error::{IngredientError, IngredientResult};
pub use crate::ingredient::{Ingredient, ParseQuality};
#[allow(deprecated)]
use nom::{
    bytes::complete::tag,
    character::complete::{not_line_ending, space0, space1},
    combinator::{opt, verify},
    error::context,
    multi::many1,
    Parser,
};
use parser::{parse_ingredient_text, parse_unit_text, MeasurementParser, Res};
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
    "thinly sliced",
    "sliced",
    "plain",
    "to taste",
];

/// Default purpose phrases that get extracted to the modifier field.
/// These describe what the ingredient is used for (e.g., "for garnish").
/// Use `with_purpose_phrases()` to add custom purpose phrases.
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

    /// Add custom purpose phrases to the parser (chainable)
    ///
    /// Purpose phrases like "for garnish" describe what the ingredient is used for.
    /// They are extracted from ingredient names and moved to the modifier field.
    ///
    /// # Example
    /// ```
    /// use ingredient::IngredientParser;
    ///
    /// let parser = IngredientParser::new()
    ///     .with_purpose_phrases(&["for blooming", "for tempering"]);
    ///
    /// let ingredient = parser.from_str("1 tbsp butter, for blooming");
    /// assert_eq!(ingredient.name, "butter");
    /// assert_eq!(ingredient.modifier, Some("for blooming".to_string()));
    /// ```
    pub fn with_purpose_phrases(mut self, phrases: &[&str]) -> Self {
        for phrase in phrases {
            self.adjectives.insert((*phrase).to_string());
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
        // Normalize NBSP to regular space and collapse extra whitespace
        // Use Cow to avoid allocating when input is already normalized
        let normalized = if input.contains('\u{a0}') {
            Cow::Owned(input.replace('\u{a0}', " "))
        } else {
            Cow::Borrowed(input)
        };

        let has_multiple_spaces = normalized
            .as_bytes()
            .windows(2)
            .any(|w| w[0] == b' ' && w[1] == b' ');

        let normalized = if has_multiple_spaces {
            Cow::Owned(normalized.split_whitespace().collect::<Vec<_>>().join(" "))
        } else {
            normalized
        };
        let input = &*normalized;

        // Check for optional ingredient format: entire input wrapped in parentheses
        // e.g., "(½ cup chopped walnuts)" indicates an optional ingredient
        if let Some(ingredient) = self.try_parse_optional_ingredient(input) {
            return ingredient;
        }

        // Check for trailing amount format first (common in professional cookbooks)
        // Format: "Ingredient name — AMOUNT" or "Ingredient (info) — AMOUNT"
        if let Some(ingredient) = self.try_parse_trailing_amount_format(input) {
            return ingredient;
        }

        match self.parse_ingredient(input) {
            Ok((_, mut ingredient)) => {
                // Extract secondary amounts from modifier if present
                if let Some(ref modifier) = ingredient.modifier {
                    let (secondary_amounts, cleaned_modifier) =
                        extract_secondary_amounts(modifier, &self.units);

                    // Add any extracted secondary amounts
                    ingredient.amounts.extend(secondary_amounts);

                    // Update modifier (could be empty now)
                    ingredient.modifier = if cleaned_modifier.is_empty() {
                        None
                    } else {
                        Some(cleaned_modifier)
                    };
                }
                ingredient
            }
            Err(_) => {
                // Fallback: create an ingredient with just the name if parsing fails completely
                Ingredient {
                    name: input.trim().to_string(),
                    amounts: vec![],
                    modifier: None,
                    optional: false,
                }
            }
        }
    }

    /// Try to parse an optional ingredient format: "(amount ingredient, modifier)"
    ///
    /// When an entire ingredient line is wrapped in parentheses, it indicates
    /// the ingredient is optional. This is common in cookbooks like Joy of Cooking.
    fn try_parse_optional_ingredient(&self, input: &str) -> Option<Ingredient> {
        let trimmed = input.trim();

        // Must start with '(' and end with ')'
        if !trimmed.starts_with('(') || !trimmed.ends_with(')') {
            return None;
        }

        // Extract content between parentheses
        let inner = &trimmed[1..trimmed.len() - 1];

        // Try to parse the inner content as an ingredient
        match self.parse_ingredient(inner) {
            Ok((_, mut ingredient)) => {
                // Only use this if we successfully parsed something meaningful
                // (not just putting everything in modifier)
                if !ingredient.name.is_empty() || !ingredient.amounts.is_empty() {
                    ingredient.optional = true;
                    Some(ingredient)
                } else {
                    None
                }
            }
            Err(_) => None,
        }
    }

    /// Try to parse ingredient with trailing amount format: "Name — AMOUNT"
    ///
    /// This handles professional/European cookbook formats where the amount
    /// comes at the end after an em-dash, en-dash, or double hyphen.
    fn try_parse_trailing_amount_format(&self, input: &str) -> Option<Ingredient> {
        // Look for em-dash (—), en-dash (–), or double hyphen (--)
        let separators = [" — ", " – ", " -- "];

        for sep in separators {
            if let Some(pos) = input.rfind(sep) {
                let name_part = &input[..pos];
                let amount_part = &input[pos + sep.len()..];

                // Try to parse the amount part
                let mp = MeasurementParser::new(&self.units, self.is_rich_text);
                if let Ok((remaining, amounts)) = mp.parse_measurement_list(amount_part) {
                    // Only use this format if:
                    // 1. We successfully parsed amounts
                    // 2. The remaining part is empty or just whitespace
                    // 3. At least one amount has a non-temperature unit
                    if !amounts.is_empty()
                        && remaining.trim().is_empty()
                        && amounts.iter().any(|m| !is_temperature_unit(m.unit()))
                    {
                        // Clean up the name part - it may contain parenthesized info
                        let name = name_part.trim().to_string();

                        return Some(Ingredient {
                            name,
                            amounts,
                            modifier: None,
                            optional: false,
                        });
                    }
                }
            }
        }

        None
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

        // Normalize NBSP to regular space and collapse extra whitespace
        let normalized = input.replace('\u{a0}', " ");
        let input = normalized.split_whitespace().collect::<Vec<_>>().join(" ");
        let input = input.as_str();

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
        let mp = MeasurementParser::new(&self.units, self.is_rich_text);

        // Define the overall structure of an ingredient line
        let ingredient_format = (
            // Measurements at the beginning (optional)
            opt(|a| mp.parse_measurement_list(a)),
            // Space between measurements and name
            space0,
            // Optional measurements in square brackets (American Sfoglino format: "4 TBSP [56 G] BUTTER")
            opt(|a| mp.parse_bracketed_amounts(a)),
            // Space after bracketed amounts
            space0,
            // Optional adjective with required space after it
            opt((|a| self.adjective(a), space1)),
            // Name component - can be multiple words
            opt(many1(parse_ingredient_text)),
            // Optional measurements in parentheses after name
            opt(|a| mp.parse_parenthesized_amounts(a)),
            // Optional comma before modifier
            opt(tag(", ")),
            // Modifier - everything until end of line
            not_line_ending,
        );

        traced_parser!(
            "parse_ingredient",
            input,
            context("ingredient", ingredient_format).parse(input).map(
                |(
                    next_input,
                    (
                        primary_amounts,
                        _,
                        bracketed_amounts,
                        _,
                        adjective,
                        name_chunks,
                        paren_amounts,
                        _,
                        modifier_text,
                    ),
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
                    // Use case-insensitive matching
                    let name_lower = name.to_lowercase();
                    let mut found_adjectives: Vec<&String> = self
                        .adjectives
                        .iter()
                        .filter(|adj| name_lower.contains(adj.as_str()))
                        .collect();
                    found_adjectives.sort_by_key(|a| std::cmp::Reverse(a.len()));

                    let mut name_lower = name_lower;
                    for adj in found_adjectives {
                        // Only extract if the adjective is still in the name (case-insensitive)
                        // (it may have been removed as part of a longer adjective)
                        if let Some(pos) = name_lower.find(adj.as_str()) {
                            if !modifiers.is_empty() {
                                modifiers.push_str(", ");
                            }
                            modifiers.push_str(adj);
                            // Remove the matched text using the position found
                            // Use pre-allocated String to avoid format! allocation
                            let mut new_name = String::with_capacity(name.len());
                            let before = name[..pos].trim();
                            let after = name[pos + adj.len()..].trim();
                            if !before.is_empty() {
                                new_name.push_str(before);
                                if !after.is_empty() {
                                    new_name.push(' ');
                                }
                            }
                            if !after.is_empty() {
                                new_name.push_str(after);
                            }
                            name = new_name.trim().to_string();
                            name_lower = name.to_lowercase();
                        }
                    }
                    // Clean up multiple spaces
                    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");

                    // Extract alternatives like "or 1 teaspoon dried thyme" to modifier
                    let (name, alternative) = extract_alternative(&name);
                    if let Some(alt) = alternative {
                        if !modifiers.is_empty() {
                            modifiers.push_str(", ");
                        }
                        modifiers.push_str(&alt);
                    }

                    // Combine all measurements (primary, bracketed, and parenthesized)
                    let mut amounts: Vec<Measure> = Vec::new();
                    if let Some(primary) = primary_amounts {
                        amounts.extend(primary);
                    }
                    if let Some(bracketed) = bracketed_amounts {
                        amounts.extend(bracketed);
                    }
                    if let Some(parenthesized) = paren_amounts {
                        amounts.extend(parenthesized);
                    }

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
                            optional: false,
                        },
                    )
                },
            ),
            |i: &Ingredient| i.name.clone(),
            "parse failed"
        )
    }

    /// Parse and validate an adjective string
    fn adjective<'a>(&self, input: &'a str) -> Res<&'a str, String> {
        traced_parser!(
            "adjective",
            input,
            context(
                "adjective",
                verify(parse_unit_text, |s: &str| {
                    self.adjectives.contains(&s.to_lowercase())
                }),
            )
            .parse(input),
            |s: &String| s.clone(),
            "not an adjective"
        )
    }
}

/// Extract alternative ingredients from the name (e.g., "garlic or 1 teaspoon garlic powder")
///
/// Returns (cleaned_name, optional_alternative) where:
/// - cleaned_name: The ingredient name with alternative removed
/// - optional_alternative: The alternative portion to be added to modifier
fn extract_alternative(name: &str) -> (String, Option<String>) {
    use once_cell::sync::Lazy;
    use regex::Regex;

    // Pattern to match " or [number/a/an] ..." at the end of an ingredient name
    // This captures cases like:
    // - "fresh thyme or 1 teaspoon dried thyme"
    // - "butter or a splash of oil"
    static ALTERNATIVE_PATTERN: Lazy<Regex> = Lazy::new(|| {
        #[allow(clippy::expect_used)]
        Regex::new(r"(?i)\s+or\s+(\d+|a\s+|an\s+)").expect("invalid alternative pattern regex")
    });

    if let Some(m) = ALTERNATIVE_PATTERN.find(name) {
        let (ingredient_part, alternative_part) = name.split_at(m.start());
        let alternative = alternative_part.trim();
        if !alternative.is_empty() {
            return (
                ingredient_part.trim().to_string(),
                Some(alternative.to_string()),
            );
        }
    }

    (name.to_string(), None)
}

/// Extract secondary amounts from modifier patterns like "(from about 15 sprigs)"
///
/// Returns (extracted_amounts, cleaned_modifier) where:
/// - extracted_amounts: Vec of Measure parsed from the pattern
/// - cleaned_modifier: The modifier with the pattern removed (or original if no pattern found)
fn extract_secondary_amounts(
    modifier: &str,
    units: &std::collections::HashSet<String>,
) -> (Vec<unit::Measure>, String) {
    use once_cell::sync::Lazy;
    use regex::Regex;

    // Pattern to match "(from about X)", "(about X)", "(approximately X)"
    static SECONDARY_AMOUNT_PATTERN: Lazy<Regex> = Lazy::new(|| {
        // This regex is a compile-time constant, so we can safely use expect
        #[allow(clippy::expect_used)]
        Regex::new(r"\((?:from\s+)?(?:about|approximately|roughly|around)\s+([^)]+)\)")
            .expect("invalid secondary amount regex")
    });

    let Some(caps) = SECONDARY_AMOUNT_PATTERN.captures(modifier) else {
        return (vec![], modifier.to_string());
    };

    // Group 0 (full match) and group 1 are guaranteed to exist when captures succeeds
    let Some(full_match) = caps.get(0) else {
        return (vec![], modifier.to_string());
    };
    let Some(amount_match) = caps.get(1) else {
        return (vec![], modifier.to_string());
    };
    let amount_text = amount_match.as_str().trim();

    // Try to parse the amount text
    let mp = MeasurementParser::new(units, false);
    if let Ok((remaining, measures)) = mp.parse_measurement_list(amount_text) {
        // Accept if we got at least one measure and remaining is either empty
        // or just a single word (the countable item like "sprigs")
        let remaining_trimmed = remaining.trim();
        let is_simple_remaining = remaining_trimmed.is_empty()
            || (remaining_trimmed.split_whitespace().count() == 1
                && remaining_trimmed.chars().all(|c| c.is_alphabetic()));

        if is_simple_remaining && !measures.is_empty() {
            // Remove the matched pattern from modifier
            let cleaned = format!(
                "{}{}",
                &modifier[..full_match.start()],
                &modifier[full_match.end()..]
            )
            .trim()
            .to_string();

            return (measures, cleaned);
        }
    }

    // Couldn't parse - return original
    (vec![], modifier.to_string())
}

/// Check if a unit represents a temperature
fn is_temperature_unit(unit: &unit::Unit) -> bool {
    matches!(unit, unit::Unit::Fahrenheit | unit::Unit::Celcius)
}
