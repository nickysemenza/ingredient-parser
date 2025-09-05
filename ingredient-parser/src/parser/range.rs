//! Range and value parsing functionality
//! 
//! This module contains functions for parsing numeric values and ranges
//! in ingredient measurements (e.g., "2-3 cups", "up to 5 tablespoons").

use nom::{
    branch::alt,
    bytes::complete::tag,
    character::complete::space0,
    character::complete::space1,
    combinator::opt,
    error::context,
    number::complete::double,
    Parser,
};

use crate::fraction::fraction_number;
use super::helpers::{text_number, Res};

/// Parse numeric values including fractions, decimals, and text numbers
pub fn parse_number(input: &str, is_rich_text: bool) -> Res<&str, f64> {
    if is_rich_text {
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

/// Parse the upper end of a range like "-3", "to 5", "through 10", or "or 2"
pub fn parse_range_end(input: &str, is_rich_text: bool) -> Res<&str, f64> {
    // Two possible formats for range syntax:

    // 1. Dash syntax: space + dash + space + number
    let dash_range = (
        space0,                           // Optional space
        alt((tag("-"), tag("–"))),        // Dash (including em-dash)
        space0,                           // Optional space
        |a| parse_number(a, is_rich_text), // Upper bound number
    );

    // 2. Word syntax: space + keyword + space + number
    let word_range = (
        space1,                                      // Required space
        alt((tag("to"), tag("through"), tag("or"))), // Range keywords
        space1,                                      // Required space
        |a| parse_number(a, is_rich_text),            // Upper bound number
    );

    context("range_end", alt((dash_range, word_range)))
        .parse(input)
        .map(|(next_input, (_, _, _, upper_value))| {
            // Return just the upper value
            (next_input, upper_value)
        })
}

/// Parse expressions like "up to 5" or "at most 10"
pub fn parse_upper_bound_only(input: &str, is_rich_text: bool) -> Res<&str, (f64, Option<f64>)> {
    let format = (
        opt(space0),                         // Optional space
        alt((tag("up to"), tag("at most"))), // Upper bound keywords
        space0,                              // Optional space
        |a| parse_number(a, is_rich_text),   // The upper bound value
    );

    context("upper_bound_only", format).parse(input).map(
        |(next_input, (_, _, _, upper_value))| {
            // Return 0.0 as the base value and the parsed number as the upper bound
            (next_input, (0.0, Some(upper_value)))
        },
    )
}

/// Parse a single value possibly followed by a range
pub fn parse_value_with_optional_range(input: &str, is_rich_text: bool) -> Res<&str, (f64, Option<f64>)> {
    let format = (
        |a| parse_number(a, is_rich_text),         // The main value
        opt(|a| parse_range_end(a, is_rich_text)), // Optional range end
    );

    context("value_with_optional_range", format).parse(input)
}

/// Parse a multiplier expression like "2 x" (meaning multiply the following value by 2)
pub fn parse_multiplier(input: &str, is_rich_text: bool) -> Res<&str, f64> {
    let multiplier_format = (
        |a| parse_number(a, is_rich_text), // The multiplier value
        space1,                            // Required whitespace
        tag("x"),                          // The "x" character
        space1,                            // Required whitespace
    );

    context("multiplier", multiplier_format).parse(input).map(
        |(next_input, (multiplier_value, _, _, _))| {
            // Return just the numeric value
            (next_input, multiplier_value)
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_number() {
        // Normal mode (not rich text)
        assert_eq!(parse_number("2", false), Ok(("", 2.0)));
        assert_eq!(parse_number("2.5", false), Ok(("", 2.5)));
        assert_eq!(parse_number("½", false), Ok(("", 0.5)));
        assert_eq!(parse_number("one", false), Ok(("", 1.0)));
        assert_eq!(parse_number("1/2", false), Ok(("", 0.5)));
        
        // Rich text mode
        assert_eq!(parse_number("2", true), Ok(("", 2.0)));
        assert_eq!(parse_number("2.5", true), Ok(("", 2.5)));
        assert_eq!(parse_number("½", true), Ok(("", 0.5)));
        assert_eq!(parse_number("1/2", true), Ok(("", 0.5)));
        // Text numbers not supported in rich text mode
        assert!(parse_number("one", true).is_err());
    }

    #[test]
    fn test_parse_range_end() {
        // Dash syntax
        assert_eq!(parse_range_end("-3", false), Ok(("", 3.0)));
        assert_eq!(parse_range_end(" - 3", false), Ok(("", 3.0)));
        assert_eq!(parse_range_end("–4", false), Ok(("", 4.0)));
        
        // Word syntax
        assert_eq!(parse_range_end(" to 5", false), Ok(("", 5.0)));
        assert_eq!(parse_range_end(" through 10", false), Ok(("", 10.0)));
        assert_eq!(parse_range_end(" or 2", false), Ok(("", 2.0)));
        
        // Invalid input
        assert!(parse_range_end("5", false).is_err());
        assert!(parse_range_end("-", false).is_err());
    }

    #[test]
    fn test_parse_upper_bound_only() {
        assert_eq!(parse_upper_bound_only("up to 5", false), Ok(("", (0.0, Some(5.0)))));
        assert_eq!(parse_upper_bound_only("at most 10", false), Ok(("", (0.0, Some(10.0)))));
        assert_eq!(parse_upper_bound_only(" up to 3", false), Ok(("", (0.0, Some(3.0)))));
        
        // Invalid input
        assert!(parse_upper_bound_only("5", false).is_err());
        assert!(parse_upper_bound_only("up to", false).is_err());
    }

    #[test]
    fn test_parse_value_with_optional_range() {
        // Single value
        assert_eq!(parse_value_with_optional_range("2", false), Ok(("", (2.0, None))));
        
        // Value with range
        assert_eq!(parse_value_with_optional_range("2-3", false), Ok(("", (2.0, Some(3.0)))));
        assert_eq!(parse_value_with_optional_range("2 to 5", false), Ok(("", (2.0, Some(5.0)))));
        assert_eq!(parse_value_with_optional_range("1 or 2", false), Ok(("", (1.0, Some(2.0)))));
    }

    #[test]
    fn test_parse_multiplier() {
        assert_eq!(parse_multiplier("2 x ", false), Ok(("", 2.0)));
        assert_eq!(parse_multiplier("3 x ", false), Ok(("", 3.0)));
        assert_eq!(parse_multiplier("½ x ", false), Ok(("", 0.5)));
        
        // Invalid input
        assert!(parse_multiplier("2 y ", false).is_err());
        assert!(parse_multiplier("2x", false).is_err()); // Missing spaces
        assert!(parse_multiplier("x 2", false).is_err());
    }
}