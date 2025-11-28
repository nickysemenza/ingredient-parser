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
//! let parser = IngredientParser::new(false);
//! let ingredient = parser.from_str("1¼ cups / 155.5g flour");
//! assert_eq!(ingredient.amounts.len(), 2); // Multiple units parsed
//! ```
//!
//! ## Error Handling
//!
//! Use [`IngredientParser::try_from_str`] for explicit error handling:
//!
//! ```
//! use ingredient::IngredientParser;
//!
//! let parser = IngredientParser::new(false);
//! match parser.try_from_str("2 cups flour") {
//!     Ok(ingredient) => println!("Parsed: {}", ingredient.name),
//!     Err(e) => eprintln!("Parse error: {}", e),
//! }
//! ```

use std::collections::HashSet;
use std::iter::FromIterator;

pub use crate::ingredient::Ingredient;
pub use crate::error::{IngredientError, IngredientResult};
use fraction::fraction_number;
#[allow(deprecated)]
use nom::{
    branch::alt,
    bytes::complete::tag,
    character::complete::{char, not_line_ending, space0, space1},
    combinator::{opt, verify},
    error::context,
    multi::{many1, separated_list1},
    number::complete::double,
    sequence::{delimited, tuple},
    Parser,
};
use tracing::info;
use unit::Measure;
use parser::{text, text_number, unitamt, Res};

#[cfg(feature = "serde-derive")]
#[macro_use]
extern crate serde;

mod fraction;
pub mod error;
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
    IngredientParser::new(false).from_str(input)
}

/// Customizable ingredient parser with configurable units and adjectives
/// 
/// This parser allows you to customize which units and adjectives are recognized
/// during parsing. It also supports rich text mode for handling special Unicode
/// characters like fractions (½, ¼, etc.).
/// 
/// # Fields
/// 
/// * `units` - Set of recognized measurement units (e.g. "cups", "grams", "tbsp")
/// * `adjectives` - Set of recognized adjectives that get moved to the modifier field
/// * `is_rich_text` - Whether to parse rich text characters (Unicode fractions, etc.)
/// 
/// # Examples
/// 
/// ```
/// use ingredient::IngredientParser;
/// use std::collections::HashSet;
/// 
/// // Create parser with custom units
/// let mut parser = IngredientParser::new(false);
/// parser.units.insert("handfuls".to_string());
/// 
/// let ingredient = parser.from_str("2 handfuls of nuts");
/// assert_eq!(ingredient.name, "nuts");
/// 
/// // Rich text mode handles Unicode fractions
/// let rich_parser = IngredientParser::new(true);
/// let ingredient = rich_parser.from_str("½ cup sugar");
/// // Parser handles the ½ character directly
/// ```
#[derive(Clone, PartialEq, Debug, Default)]
pub struct IngredientParser {
    /// Set of recognized measurement units
    pub units: HashSet<String>,
    /// Set of recognized adjectives that get moved to modifier field
    pub adjectives: HashSet<String>,
    /// Whether to parse rich text characters (Unicode fractions, etc.)
    pub is_rich_text: bool,
}
impl IngredientParser {
    /// Create a new ingredient parser with default units and adjectives
    /// 
    /// # Arguments
    /// 
    /// * `is_rich_text` - Whether to enable rich text parsing for Unicode characters
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
    /// let parser = IngredientParser::new(false);
    /// 
    /// // Rich text parser (handles ½, ¼, etc.)
    /// let rich_parser = IngredientParser::new(true);
    /// ```
    pub fn new(is_rich_text: bool) -> Self {
        let units: Vec<String> = vec![
            // non standard units - these aren't really convertible for the most part.
            // default set
            "whole", "packet", "sticks", "stick", "cloves", "clove", "bunch", "head", "large",
            "pinch", "small", "medium", "package", "recipe", "slice", "standard", "can", "leaf",
            "leaves", "strand", "tin",
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
            is_rich_text,
        }
    }
    /// wrapper for [self.parse_ingredient]
    /// ```
    /// use ingredient::{from_str};
    /// assert_eq!(from_str("one whole egg").to_string(),"1 whole egg");
    /// ```
    /// Parse an ingredient string into an Ingredient object
    ///
    /// This method never panics and provides fallback behavior for unparseable input
    pub fn from_str(self, input: &str) -> Ingredient {
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

    /// Parse an ingredient string with explicit error handling
    /// 
    /// This method returns a `Result` that allows you to handle parsing errors
    /// explicitly, unlike `from_str` which provides fallback behavior.
    /// 
    /// # Arguments
    /// 
    /// * `input` - The ingredient string to parse
    /// 
    /// # Returns
    /// 
    /// * `Ok(Ingredient)` - Successfully parsed ingredient
    /// * `Err(IngredientError)` - Detailed error information about parsing failure
    /// 
    /// # Examples
    /// 
    /// ```
    /// use ingredient::IngredientParser;
    /// 
    /// let parser = IngredientParser::new(false);
    /// 
    /// match parser.try_from_str("2 cups flour") {
    ///     Ok(ingredient) => println!("Parsed: {}", ingredient.name),
    ///     Err(e) => eprintln!("Parse error: {}", e),
    /// }
    /// ```
    pub fn try_from_str(self, input: &str) -> IngredientResult<Ingredient> {
        match self.parse_ingredient(input) {
            Ok((_, ingredient)) => Ok(ingredient),
            Err(e) => Err(IngredientError::ParseError {
                input: input.to_string(),
                context: format!("Failed to parse ingredient: {e:?}"),
            }),
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
    /// let parser = IngredientParser::new(false);
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
    pub fn parse_with_trace(self, input: &str) -> trace::ParseWithTrace<Ingredient> {
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
    /// let ip = IngredientParser::new(false);
    /// assert_eq!(
    ///    ip.must_parse_amount("120 grams"),
    ///    vec![Measure::parse_new("grams",120.0)]
    ///  );
    /// assert_eq!(
    ///    ip.must_parse_amount("120 grams / 1 cup"),
    ///    vec![Measure::parse_new("grams",120.0),Measure::parse_new("cup", 1.0)]
    ///  );
    /// assert_eq!(
    ///    ip.must_parse_amount("120 grams / 1 cup / 1 whole"),
    ///    vec![Measure::parse_new("grams",120.0),Measure::parse_new("cup", 1.0),Measure::parse_new("whole", 1.0)]
    ///  );
    /// ```
    /// Parse a string containing one or more measurements
    ///
    /// Returns a Result with a Vec of Measures, or an error if parsing fails
    #[tracing::instrument(name = "parse_amount")]
    pub fn parse_amount(&self, input: &str) -> IngredientResult<Vec<Measure>> {
        match self.clone().parse_measurement_list(input) {
            Ok((_, measurements)) => Ok(measurements),
            Err(e) => Err(IngredientError::AmountParseError {
                input: input.to_string(),
                reason: format!("{e:?}"),
            }),
        }
    }

    /// Parse measurements with no error handling (will panic on failure)
    /// 
    /// # Panics
    /// This method will panic if parsing fails. Consider using `parse_amount` for error handling.
    pub fn must_parse_amount(&self, input: &str) -> Vec<Measure> {
        match self.parse_amount(input) {
            Ok(measures) => measures,
            Err(e) => panic!("Measurement parsing failed for '{input}': {e}"),
        }
    }

    /// Parse measurements with safer error handling that returns empty vec on failure
    pub fn try_parse_amount(&self, input: &str) -> Vec<Measure> {
        self.parse_amount(input).unwrap_or_else(|_| vec![])
    }

    /// Parse an ingredient line item, such as `120 grams / 1 cup whole wheat flour, sifted lightly`.
    ///
    /// returns an [Ingredient], Can be used as a wrapper to return verbose errors.
    ///
    /// supported formats include:
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
    /// ```
    /// use ingredient::{IngredientParser, ingredient::Ingredient, unit::Measure};
    /// let ip = IngredientParser::new(false);
    /// assert_eq!(
    ///     ip.parse_ingredient("1¼  cups / 155.5 grams flour"),
    ///     Ok((
    ///         "",
    ///         Ingredient {
    ///             name: "flour".to_string(),
    ///             amounts: vec![
    ///                 Measure::parse_new("cups", 1.25),
    ///                 Measure::parse_new("grams", 155.5),
    ///             ],
    ///             modifier: None,
    ///         }
    ///     ))
    /// );
    /// ```
    /// Parse a complete ingredient line including amounts, name, and modifiers
    ///
    /// This handles formats like:
    /// - "2 cups flour"
    /// - "1-2 tbsp sugar, sifted"
    /// - "3 large eggs"
    /// - "1 cup (240ml) milk, room temperature"
    #[tracing::instrument(name = "parse_ingredient")]
    #[allow(clippy::type_complexity)]
    pub fn parse_ingredient(self, input: &str) -> Res<&str, Ingredient> {
        use trace::{trace_enter, trace_exit_failure, trace_exit_success};

        trace_enter("parse_ingredient", input);

        // Define the overall structure of an ingredient line
        let ingredient_format = (
            // Measurements at the beginning (optional)
            opt(|a| self.clone().parse_measurement_list(a)),
            // Space between measurements and name
            space0,
            // Optional adjective with required space after it
            opt((|a| self.clone().adjective(a), space1)),
            // Name component - can be multiple words
            opt(many1(text)),
            // Optional measurements in parentheses after name
            opt(|a| self.clone().parse_parenthesized_amounts(a)),
            // Optional comma before modifier
            opt(tag(", ")),
            // Modifier - everything until end of line
            not_line_ending,
        );

        let result = context("ingredient", ingredient_format)
            .parse(input)
            .map(|(next_input, res)| {
                let (
                    primary_amounts,       // Measurements at start of line
                    _,                     // Space
                    adjective,             // Optional adjective
                    name_chunks,           // Name components
                    parenthesized_amounts, // Measurements in parentheses
                    _,                     // Comma
                    modifier_text,         // Modifier text
                ): (
                    Option<Vec<Measure>>,
                    &str,
                    Option<(String, &str)>,
                    Option<Vec<String>>,
                    Option<Vec<Measure>>,
                    Option<&str>,
                    &str,
                ) = res;

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
                self.adjectives.iter().for_each(|adj| {
                    if name.contains(adj) {
                        modifiers.push_str(adj);
                        name = name.replace(adj, "").trim_matches(' ').to_string();
                    }
                });

                // Combine all measurements
                let amounts = match (primary_amounts, parenthesized_amounts) {
                    (Some(primary), Some(parenthesized)) => {
                        // Combine both sets of measurements
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
            });

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

    /// Parse a value that may have a range, returning (value, optional_upper_range)
    fn get_value(self, input: &str) -> Res<&str, (f64, Option<f64>)> {
        use trace::{trace_enter, trace_exit_failure, trace_exit_success};
        trace_enter("get_value", input);

        let result = context(
            "value_with_range",
            alt((
                |a| self.clone().parse_upper_bound_only(a), // "up to X" or "at most X"
                |a| self.clone().parse_value_with_optional_range(a), // A value possibly with a range
            )),
        )
        .parse(input);

        match &result {
            Ok((remaining, (val, upper))) => {
                let consumed = input.len() - remaining.len();
                let preview = match upper {
                    Some(u) => format!("{val}-{u}"),
                    None => format!("{val}"),
                };
                trace_exit_success(consumed, &preview);
            }
            Err(_) => trace_exit_failure("no value"),
        }
        result
    }

    /// Parse a single value possibly followed by a range
    fn parse_value_with_optional_range(self, input: &str) -> Res<&str, (f64, Option<f64>)> {
        use trace::{trace_enter, trace_exit_failure, trace_exit_success};
        trace_enter("parse_value_with_optional_range", input);

        // Format: numeric value + optional range
        let format = (
            |a| self.clone().parse_number(a),         // The main value
            opt(|a| self.clone().parse_range_end(a)), // Optional range end
        );

        let result = context("value_with_optional_range", format).parse(input);

        match &result {
            Ok((remaining, (val, upper))) => {
                let consumed = input.len() - remaining.len();
                let preview = match upper {
                    Some(u) => format!("{val}-{u}"),
                    None => format!("{val}"),
                };
                trace_exit_success(consumed, &preview);
            }
            Err(_) => trace_exit_failure("no value"),
        }
        result
    }

    /// Parse expressions like "up to 5" or "at most 10"
    fn parse_upper_bound_only(self, input: &str) -> Res<&str, (f64, Option<f64>)> {
        use trace::{trace_enter, trace_exit_failure, trace_exit_success};
        trace_enter("parse_upper_bound_only", input);

        // Format: prefix + number
        let format = (
            opt(space0),                         // Optional space
            alt((tag("up to"), tag("at most"))), // Upper bound keywords
            space0,                              // Optional space
            |a| self.clone().parse_number(a),    // The upper bound value
        );

        let result = context("upper_bound_only", format).parse(input).map(
            |(next_input, (_, _, _, upper_value))| {
                // Return 0.0 as the base value and the parsed number as the upper bound
                (next_input, (0.0, Some(upper_value)))
            },
        );

        match &result {
            Ok((remaining, (_, Some(u)))) => {
                let consumed = input.len() - remaining.len();
                trace_exit_success(consumed, &format!("up to {u}"));
            }
            Ok(_) => trace_exit_success(0, ""),
            Err(_) => trace_exit_failure("no upper bound"),
        }
        result
    }

    fn unit(self, input: &str) -> Res<&str, String> {
        use trace::{trace_enter, trace_exit_failure, trace_exit_success};
        trace_enter("unit", input);

        let result = context(
            "unit",
            verify(unitamt, |s: &str| unit::is_valid(self.units.clone(), s)),
        )
        .parse(input);

        match &result {
            Ok((remaining, unit_str)) => {
                let consumed = input.len() - remaining.len();
                trace_exit_success(consumed, unit_str);
            }
            Err(_) => trace_exit_failure("not a valid unit"),
        }
        result
    }
    fn unit_extra(self, input: &str) -> Res<&str, String> {
        use trace::{trace_enter, trace_exit_failure, trace_exit_success};
        trace_enter("unit_extra", input);

        let result = context(
            "unit",
            verify(unitamt, |s: &str| {
                // fix for test_parse_whole_wheat_ambigious
                let mut x = self.units.clone();
                if input.starts_with("whole wheat") {
                    x.remove("whole");
                }
                unit::is_addon_unit(x, s)
            }),
        )
        .parse(input);

        match &result {
            Ok((remaining, unit_str)) => {
                let consumed = input.len() - remaining.len();
                trace_exit_success(consumed, unit_str);
            }
            Err(_) => trace_exit_failure("not an addon unit"),
        }
        result
    }
    fn adjective(self, input: &str) -> Res<&str, String> {
        use trace::{trace_enter, trace_exit_failure, trace_exit_success};
        trace_enter("adjective", input);

        let result = context(
            "adjective",
            verify(unitamt, |s: &str| {
                self.adjectives.contains(&s.to_lowercase())
            }),
        )
        .parse(input);

        match &result {
            Ok((remaining, adj)) => {
                let consumed = input.len() - remaining.len();
                trace_exit_success(consumed, adj);
            }
            Err(_) => trace_exit_failure("not an adjective"),
        }
        result
    }

    /// Parse a single measurement like "2 cups" or "about 3 tablespoons"
    #[allow(deprecated)]
    fn parse_single_measurement(self, input: &str) -> Res<&str, Measure> {
        use trace::{trace_enter, trace_exit_failure, trace_exit_success};
        trace_enter("parse_single_measurement", input);

        // Define the structure of a basic measurement
        let measurement_parser = (
            opt(tag("about ")),                        // Optional "about" prefix for estimates
            opt(|a| self.clone().parse_multiplier(a)), // Optional multiplier (e.g., "2 x")
            |a| self.clone().get_value(a),             // The numeric value
            space0,                                    // Optional whitespace
            opt(|a| self.clone().unit(a)),             // Optional unit of measure
            opt(alt((tag("."), tag(" of")))),          // Optional trailing period or "of"
        );

        let result = context("single_measurement", tuple(measurement_parser))
            .parse(input)
            .map(|(next_input, res)| {
                let (_estimate_prefix, multiplier, value, _, unit, _) = res;

                // Apply multiplier if present
                let final_value = match multiplier {
                    Some(m) => value.0 * m,
                    None => value.0,
                };

                // Default to "whole" unit if none specified
                let final_unit = unit.unwrap_or_else(|| "whole".to_string()).to_lowercase();

                // Create the measurement
                (
                    next_input,
                    Measure::from_parts(
                        final_unit.as_ref(),
                        final_value,
                        value.1, // Pass along any upper range value
                    ),
                )
            });

        match &result {
            Ok((remaining, measure)) => {
                let consumed = input.len() - remaining.len();
                trace_exit_success(consumed, &measure.to_string());
            }
            Err(_) => trace_exit_failure("no measurement"),
        }
        result
    }
    /// Parse a standalone unit with implicit quantity of 1, like "cup" or "tablespoons"
    fn parse_unit_only(self, input: &str) -> Res<&str, Measure> {
        use trace::{trace_enter, trace_exit_failure, trace_exit_success};
        trace_enter("parse_unit_only", input);

        // Format: optional space + unit + optional period/of + required space
        let unit_only_format = (
            // Space requirement depends on text mode
            |a| {
                if self.is_rich_text {
                    space1(a) // Rich text mode requires space
                } else {
                    space0(a) // Normal mode allows optional space
                }
            },
            |a| self.clone().unit_extra(a),   // Parse the unit
            opt(alt((tag("."), tag(" of")))), // Optional period or "of"
            space1,                           // Required space after unit
        );

        let result = context("unit_only", unit_only_format)
            .parse(input)
            .map(|(next_input, (_, unit, _, _))| {
                // Create a measure with value 1.0 and the parsed unit
                (
                    next_input,
                    Measure::from_parts(unit.to_lowercase().as_ref(), 1.0, None),
                )
            });

        match &result {
            Ok((remaining, measure)) => {
                let consumed = input.len() - remaining.len();
                trace_exit_success(consumed, &measure.to_string());
            }
            Err(_) => trace_exit_failure("no unit-only"),
        }
        result
    }
    /// Parse a range with units, like "78g to 104g" or "2-3 cups"
    fn parse_range_with_units(self, input: &str) -> Res<&str, Option<Measure>> {
        use trace::{trace_enter, trace_exit_failure, trace_exit_success};
        trace_enter("parse_range_with_units", input);

        // Format for a measurement with a range
        let range_format = (
            opt(tag("about ")),                  // Optional "about" for estimates
            |a| self.clone().get_value(a),       // The lower value
            space0,                              // Optional whitespace
            opt(|a| self.clone().unit(a)),       // Optional unit for lower value
            |a| self.clone().parse_range_end(a), // The upper range value
            opt(|a| self.clone().unit(a)),       // Optional unit for upper value
            opt(alt((tag("."), tag(" of")))),    // Optional period or "of"
        );

        let result = context("range_with_units", range_format)
            .parse(input)
            .map(|(next_input, res)| {
                let (_, lower_value, _, lower_unit, upper_val, upper_unit, _) = res;

                // Check for unit mismatch - both units must be the same if both are specified
                if upper_unit.is_some() && lower_unit != upper_unit {
                    info!(
                        "unit mismatch between range values: {:?} vs {:?}",
                        lower_unit, upper_unit
                    );
                    return (next_input, None);
                }

                // Create the measurement with range
                (
                    next_input,
                    Some(Measure::from_parts(
                        // Use the lower unit, or default to "whole" if not specified
                        lower_unit
                            .unwrap_or_else(|| "whole".to_string())
                            .to_lowercase()
                            .as_ref(),
                        lower_value.0,
                        Some(upper_val),
                    )),
                )
            });

        match &result {
            Ok((remaining, opt_measure)) => {
                let consumed = input.len() - remaining.len();
                let preview = opt_measure
                    .as_ref()
                    .map(|m| m.to_string())
                    .unwrap_or_else(|| "unit mismatch".to_string());
                trace_exit_success(consumed, &preview);
            }
            Err(_) => trace_exit_failure("no range"),
        }
        result
    }
    // parses 1-n amounts, e.g. `12 grams` or `120 grams / 1 cup`
    #[tracing::instrument(name = "many_amount")]
    /// Parse a list of measurements with different separators
    ///
    /// This handles formats like:
    /// - "2 cups; 1 tbsp"
    /// - "120 grams / 1 cup"
    /// - "1 tsp, 2 tbsp"
    #[tracing::instrument(name = "many_amount")]
    fn parse_measurement_list(self, input: &str) -> Res<&str, Vec<Measure>> {
        use trace::{trace_enter, trace_exit_failure, trace_exit_success};

        trace_enter("measurement_list", input);

        // Define the separators between measurements
        let amount_separators = alt((
            tag("; "),  // semicolon with space
            tag(" / "), // slash with spaces
            tag("/"),   // bare slash
            tag(", "),  // comma with space
            tag(" "),   // just a space
        ));

        // Define the different types of measurements we can parse
        let amount_parsers = alt((
            // "1 cup plus 2 tbsp" -> combines measurements
            |input| {
                self.clone()
                    .parse_plus_expression(input)
                    .map(|(next, measure)| (next, vec![measure]))
            },
            // Range with units on both sides: "2-3 cups" or "1 to 2 tbsp"
            |input| {
                self.clone()
                    .parse_range_with_units(input)
                    .map(|(next, opt_measure)| {
                        (next, opt_measure.map_or_else(Vec::new, |m| vec![m]))
                    })
            },
            // Parenthesized amounts like "(1 cup)"
            |input| self.clone().parse_parenthesized_amounts(input),
            // Basic measurement like "2 cups"
            |input| {
                self.clone()
                    .parse_single_measurement(input)
                    .map(|(next, measure)| (next, vec![measure]))
            },
            // Just a unit with implicit quantity of 1, like "cup"
            |input| {
                self.clone()
                    .parse_unit_only(input)
                    .map(|(next, measure)| (next, vec![measure]))
            },
        ));

        // Parse a list of measurements separated by the defined separators
        let result = context(
            "measurement_list",
            separated_list1(amount_separators, amount_parsers),
        )
        .parse(input)
        .map(|(next_input, measures_list)| {
            // Flatten nested Vec<Vec<Measure>> into Vec<Measure>
            (next_input, measures_list.into_iter().flatten().collect::<Vec<Measure>>())
        });

        // Record trace outcome
        match &result {
            Ok((remaining, measures)) => {
                let consumed = input.len() - remaining.len();
                let preview = measures
                    .iter()
                    .map(|m| m.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                trace_exit_success(consumed, &preview);
            }
            Err(_) => {
                trace_exit_failure("no measurements found");
            }
        }

        result
    }

    /// Parse measurements enclosed in parentheses: (1 cup)
    fn parse_parenthesized_amounts(self, input: &str) -> Res<&str, Vec<Measure>> {
        use trace::{trace_enter, trace_exit_failure, trace_exit_success};
        trace_enter("parse_parenthesized_amounts", input);

        let result = context(
            "parenthesized_amounts",
            delimited(
                char('('),                                  // Opening parenthesis
                |a| self.clone().parse_measurement_list(a), // Parse measurements inside parentheses
                char(')'),                                  // Closing parenthesis
            ),
        )
        .parse(input);

        match &result {
            Ok((remaining, measures)) => {
                let consumed = input.len() - remaining.len();
                let preview = measures
                    .iter()
                    .map(|m| m.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                trace_exit_success(consumed, &preview);
            }
            Err(_) => trace_exit_failure("no parenthesized amounts"),
        }
        result
    }
    /// Parse numeric values including fractions, decimals, and text numbers like "one"
    fn parse_number(self, input: &str) -> Res<&str, f64> {
        use trace::{trace_enter, trace_exit_failure, trace_exit_success};
        trace_enter("parse_number", input);

        // Choose parsers based on whether we're in rich text mode
        let result = if self.is_rich_text {
            // Rich text mode: try fraction or decimal number
            context(
                "number",
                alt((
                    fraction_number, // Parse fractions like "½" or "1/2"
                    double,          // Parse decimal numbers like "2.5"
                )),
            )
            .parse(input)
        } else {
            // Normal mode: try fraction, text number, or decimal
            context(
                "number",
                alt((
                    fraction_number, // Parse fractions like "½" or "1/2"
                    text_number,     // Parse text numbers like "one" or "a"
                    double,          // Parse decimal numbers like "2.5"
                )),
            )
            .parse(input)
        };

        match &result {
            Ok((remaining, value)) => {
                let consumed = input.len() - remaining.len();
                trace_exit_success(consumed, &format!("{value}"));
            }
            Err(_) => trace_exit_failure("no number"),
        }
        result
    }
    /// Parse a multiplier expression like "2 x" (meaning multiply the following value by 2)
    fn parse_multiplier(self, input: &str) -> Res<&str, f64> {
        use trace::{trace_enter, trace_exit_failure, trace_exit_success};
        trace_enter("parse_multiplier", input);

        // Define the format of a multiplier: number + space + "x" + space
        let multiplier_format = (
            |a| self.clone().parse_number(a), // The multiplier value
            space1,                           // Required whitespace
            tag("x"),                         // The "x" character
            space1,                           // Required whitespace
        );

        let result = context("multiplier", multiplier_format).parse(input).map(
            |(next_input, (multiplier_value, _, _, _))| {
                // Return just the numeric value
                (next_input, multiplier_value)
            },
        );

        match &result {
            Ok((remaining, value)) => {
                let consumed = input.len() - remaining.len();
                trace_exit_success(consumed, &format!("{value}x"));
            }
            Err(_) => trace_exit_failure("no multiplier"),
        }
        result
    }
    /// Parse the upper end of a range like "-3", "to 5", "through 10", or "or 2"
    fn parse_range_end(self, input: &str) -> Res<&str, f64> {
        use trace::{trace_enter, trace_exit_failure, trace_exit_success};
        trace_enter("parse_range_end", input);

        // Two possible formats for range syntax:

        // 1. Dash syntax: space + dash + space + number
        let dash_range = (
            space0,                           // Optional space
            alt((tag("-"), tag("–"))),        // Dash (including em-dash)
            space0,                           // Optional space
            |a| self.clone().parse_number(a), // Upper bound number
        );

        // 2. Word syntax: space + keyword + space + number
        let word_range = (
            space1,                                      // Required space
            alt((tag("to"), tag("through"), tag("or"))), // Range keywords
            space1,                                      // Required space
            |a| self.clone().parse_number(a),            // Upper bound number
        );

        let result = context("range_end", alt((dash_range, word_range)))
            .parse(input)
            .map(|(next_input, (_, _, _, upper_value))| {
                // Return just the upper value
                (next_input, upper_value)
            });

        match &result {
            Ok((remaining, value)) => {
                let consumed = input.len() - remaining.len();
                trace_exit_success(consumed, &format!("{value}"));
            }
            Err(_) => trace_exit_failure("no range end"),
        }
        result
    }
    /// Parse expressions with "plus" that combine two measurements
    ///
    /// For example: "1 cup plus 2 tablespoons"
    fn parse_plus_expression(self, input: &str) -> Res<&str, Measure> {
        use trace::{trace_enter, trace_exit_failure, trace_exit_success};
        trace_enter("parse_plus_expression", input);

        // Define the structure of a plus expression
        let plus_parser = (
            |a| self.clone().parse_single_measurement(a), // First measurement
            space1,                                       // Required whitespace
            tag("plus"),                                  // The "plus" keyword
            space1,                                       // Required whitespace
            |a| self.clone().parse_single_measurement(a), // Second measurement
        );

        let result = context("plus_expression", plus_parser).parse(input).map(
            |(next_input, (first_measure, _, _, _, second_measure))| {
                // Add the two measurements together
                match first_measure.add(second_measure) {
                    Ok(combined) => (next_input, combined),
                    Err(_) => {
                        // If addition fails, just return the first measure as fallback
                        (next_input, first_measure)
                    }
                }
            },
        );

        match &result {
            Ok((remaining, measure)) => {
                let consumed = input.len() - remaining.len();
                trace_exit_success(consumed, &measure.to_string());
            }
            Err(_) => trace_exit_failure("no plus expression"),
        }
        result
    }
}


#[cfg(test)]
mod tests {
    use std::convert::TryFrom;

    use super::*;
    #[test]
    fn test_amount() {
        assert_eq!(
            (IngredientParser::new(false)).must_parse_amount("350 °"),
            vec![Measure::parse_new("°", 350.0)]
        );
        assert_eq!(
            (IngredientParser::new(false)).must_parse_amount("350 °F"),
            vec![Measure::parse_new("°f", 350.0)]
        );
    }

    #[test]
    fn test_amount_range() {
        assert_eq!(
            (IngredientParser::new(false)).must_parse_amount("2¼-2.5 cups"),
            vec![Measure::parse_new_with_upper("cups", 2.25, 2.5)]
        );

        assert_eq!(
            Ingredient::try_from("1-2 cups flour"),
            Ok(Ingredient {
                name: "flour".to_string(),
                amounts: vec![Measure::parse_new_with_upper("cups", 1.0, 2.0)],
                modifier: None,
            })
        );
        let amounts = (IngredientParser::new(false))
            .must_parse_amount("2 ¼ - 2.5 cups");
        assert!(!amounts.is_empty(), "Expected at least one measure");
        assert_eq!(format!("{}", amounts[0]), "2.25 - 2.5 cups");
        assert_eq!(
            (IngredientParser::new(false)).must_parse_amount("2 to 4 days"),
            vec![Measure::parse_new_with_upper("days", 2.0, 4.0)]
        );

        // #30
        assert_eq!(
            (IngredientParser::new(false)).must_parse_amount("up to 4 days"),
            vec![Measure::parse_new_with_upper("days", 0.0, 4.0)]
        );
    }
    #[test]
    fn test_ingredient_parse() {
        assert_eq!(
            Ingredient::try_from("12 cups flour"),
            Ok(Ingredient {
                name: "flour".to_string(),
                amounts: vec![Measure::parse_new("cups", 12.0)],
                modifier: None,
            })
        );
    }

    #[test]
    fn test_stringy() {
        assert_eq!(
            format!("res: {}", from_str("12 cups flour")),
            "res: 12 cups flour"
        );
        assert_eq!(from_str("one whole egg").to_string(), "1 whole egg");
        assert_eq!(from_str("a tsp flour").to_string(), "1 tsp flour");
    }
    #[test]
    fn test_with_parens() {
        assert_eq!(
            from_str("1 cup (125.5 grams) AP flour, sifted").to_string(),
            "1 cup / 125.5 g AP flour, sifted"
        );
    }
}
