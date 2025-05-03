use std::collections::HashSet;
use std::iter::FromIterator;

pub use crate::ingredient::Ingredient;
use anyhow::Result;
use fraction::fraction_number;
#[allow(deprecated)]
use nom::{
    branch::alt,
    bytes::complete::tag,
    character::complete::{alpha1, char, not_line_ending, satisfy, space0, space1},
    combinator::{opt, verify},
    error::context,
    multi::{many1, separated_list1},
    number::complete::double,
    sequence::{delimited, tuple},
    IResult, Parser,
};
use nom_language::error::VerboseError;
use tracing::info;
use unit::Measure;

extern crate nom;

#[cfg(feature = "serde-derive")]
#[macro_use]
extern crate serde;

mod fraction;
pub mod ingredient;
pub mod rich_text;
pub mod unit;
pub mod util;
pub type Res<T, U> = IResult<T, U, VerboseError<T>>;

/// Parse an ingredient string using default settings
///
/// Use [IngredientParser] directly to customize the parsing behavior
pub fn from_str(input: &str) -> Ingredient {
    IngredientParser::new(false).from_str(input)
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct IngredientParser {
    pub units: HashSet<String>,
    pub adjectives: HashSet<String>,
    pub is_rich_text: bool,
}
impl IngredientParser {
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
    /// This is a wrapper around parse_ingredient that handles the Result
    pub fn from_str(self, input: &str) -> Ingredient {
        // The parser is flexible enough that it rarely fails
        self.parse_ingredient(input).unwrap().1
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
    pub fn parse_amount(&self, input: &str) -> Result<Vec<Measure>> {
        match self.clone().parse_measurement_list(input) {
            Ok((_, measurements)) => Ok(measurements),
            Err(e) => Err(anyhow::anyhow!(
                "Failed to parse amount from '{}': {:?}",
                input,
                e
            )),
        }
    }

    /// Parse measurements with no error handling (will panic on failure)
    pub fn must_parse_amount(&self, input: &str) -> Vec<Measure> {
        self.parse_amount(input)
            .expect("Measurement parsing failed")
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

        context("ingredient", ingredient_format)
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
            })
    }

    /// Parse a value that may have a range, returning (value, optional_upper_range)
    fn get_value(self, input: &str) -> Res<&str, (f64, Option<f64>)> {
        context(
            "value_with_range",
            alt((
                |a| self.clone().parse_upper_bound_only(a), // "up to X" or "at most X"
                |a| self.clone().parse_value_with_optional_range(a), // A value possibly with a range
            )),
        )
        .parse(input)
    }

    /// Parse a single value possibly followed by a range
    fn parse_value_with_optional_range(self, input: &str) -> Res<&str, (f64, Option<f64>)> {
        // Format: numeric value + optional range
        let format = (
            |a| self.clone().parse_number(a),         // The main value
            opt(|a| self.clone().parse_range_end(a)), // Optional range end
        );

        context("value_with_optional_range", format).parse(input)
    }

    /// Parse expressions like "up to 5" or "at most 10"
    fn parse_upper_bound_only(self, input: &str) -> Res<&str, (f64, Option<f64>)> {
        // Format: prefix + number
        let format = (
            opt(space0),                         // Optional space
            alt((tag("up to"), tag("at most"))), // Upper bound keywords
            space0,                              // Optional space
            |a| self.clone().parse_number(a),    // The upper bound value
        );

        context("upper_bound_only", format).parse(input).map(
            |(next_input, (_, _, _, upper_value))| {
                // Return 0.0 as the base value and the parsed number as the upper bound
                (next_input, (0.0, Some(upper_value)))
            },
        )
    }

    fn unit(self, input: &str) -> Res<&str, String> {
        context(
            "unit",
            verify(unitamt, |s: &str| unit::is_valid(self.units.clone(), s)),
        )
        .parse(input)
    }
    fn unit_extra(self, input: &str) -> Res<&str, String> {
        context(
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
        .parse(input)
    }
    fn adjective(self, input: &str) -> Res<&str, String> {
        context(
            "adjective",
            verify(unitamt, |s: &str| {
                self.adjectives.contains(&s.to_lowercase())
            }),
        )
        .parse(input)
    }

    /// Parse a single measurement like "2 cups" or "about 3 tablespoons"
    #[allow(deprecated)]
    fn parse_single_measurement(self, input: &str) -> Res<&str, Measure> {
        // Define the structure of a basic measurement
        let measurement_parser = (
            opt(tag("about ")),                        // Optional "about" prefix for estimates
            opt(|a| self.clone().parse_multiplier(a)), // Optional multiplier (e.g., "2 x")
            |a| self.clone().get_value(a),             // The numeric value
            space0,                                    // Optional whitespace
            opt(|a| self.clone().unit(a)),             // Optional unit of measure
            opt(alt((tag("."), tag(" of")))),          // Optional trailing period or "of"
        );

        context("single_measurement", tuple(measurement_parser))
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
            })
    }
    /// Parse a standalone unit with implicit quantity of 1, like "cup" or "tablespoons"
    fn parse_unit_only(self, input: &str) -> Res<&str, Measure> {
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

        context("unit_only", unit_only_format)
            .parse(input)
            .map(|(next_input, (_, unit, _, _))| {
                // Create a measure with value 1.0 and the parsed unit
                (
                    next_input,
                    Measure::from_parts(unit.to_lowercase().as_ref(), 1.0, None),
                )
            })
    }
    /// Parse a range with units, like "78g to 104g" or "2-3 cups"
    fn parse_range_with_units(self, input: &str) -> Res<&str, Option<Measure>> {
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

        context("range_with_units", range_format)
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
            })
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
        context(
            "measurement_list",
            separated_list1(amount_separators, amount_parsers),
        )
        .parse(input)
        .map(|(next_input, measures_list)| {
            // Flatten nested Vec<Vec<Measure>> into Vec<Measure>
            (next_input, measures_list.into_iter().flatten().collect())
        })
    }

    /// Parse measurements enclosed in parentheses: (1 cup)
    fn parse_parenthesized_amounts(self, input: &str) -> Res<&str, Vec<Measure>> {
        context(
            "parenthesized_amounts",
            delimited(
                char('('),                                  // Opening parenthesis
                |a| self.clone().parse_measurement_list(a), // Parse measurements inside parentheses
                char(')'),                                  // Closing parenthesis
            ),
        )
        .parse(input)
    }
    /// Parse numeric values including fractions, decimals, and text numbers like "one"
    fn parse_number(self, input: &str) -> Res<&str, f64> {
        // Choose parsers based on whether we're in rich text mode
        if self.is_rich_text {
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
        }
    }
    /// Parse a multiplier expression like "2 x" (meaning multiply the following value by 2)
    fn parse_multiplier(self, input: &str) -> Res<&str, f64> {
        // Define the format of a multiplier: number + space + "x" + space
        let multiplier_format = (
            |a| self.clone().parse_number(a), // The multiplier value
            space1,                           // Required whitespace
            tag("x"),                         // The "x" character
            space1,                           // Required whitespace
        );

        context("multiplier", multiplier_format).parse(input).map(
            |(next_input, (multiplier_value, _, _, _))| {
                // Return just the numeric value
                (next_input, multiplier_value)
            },
        )
    }
    /// Parse the upper end of a range like "-3", "to 5", "through 10", or "or 2"
    fn parse_range_end(self, input: &str) -> Res<&str, f64> {
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

        context("range_end", alt((dash_range, word_range)))
            .parse(input)
            .map(|(next_input, (_, _, _, upper_value))| {
                // Return just the upper value
                (next_input, upper_value)
            })
    }
    /// Parse expressions with "plus" that combine two measurements
    ///
    /// For example: "1 cup plus 2 tablespoons"
    fn parse_plus_expression(self, input: &str) -> Res<&str, Measure> {
        // Define the structure of a plus expression
        let plus_parser = (
            |a| self.clone().parse_single_measurement(a), // First measurement
            space1,                                       // Required whitespace
            tag("plus"),                                  // The "plus" keyword
            space1,                                       // Required whitespace
            |a| self.clone().parse_single_measurement(a), // Second measurement
        );

        context("plus_expression", plus_parser).parse(input).map(
            |(next_input, (first_measure, _, _, _, second_measure))| {
                // Add the two measurements together
                let combined = first_measure.add(second_measure).unwrap();
                (next_input, combined)
            },
        )
    }
}

fn text(input: &str) -> Res<&str, String> {
    (satisfy(|c| match c {
        '-' | '—' | '\'' | '’' | '.' | '\\' => true,
        c => c.is_alphanumeric() || c.is_whitespace(),
    }))
    .parse(input)
    .map(|(next_input, res)| (next_input, res.to_string()))
}
fn unitamt(input: &str) -> Res<&str, String> {
    nom::multi::many0(alt((alpha1, tag("°"), tag("\""))))
        .parse(input)
        .map(|(next_input, res)| (next_input, res.join("")))
}

fn text_number(input: &str) -> Res<&str, f64> {
    context("text_number", alt((tag("one"), tag("a "))))
        .parse(input)
        .map(|(next_input, _)| (next_input, 1.0))
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
        assert_eq!(
            format!(
                "{}",
                (IngredientParser::new(false))
                    .must_parse_amount("2 ¼ - 2.5 cups")
                    .first()
                    .unwrap()
            ),
            "2.25 - 2.5 cups"
        );
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
