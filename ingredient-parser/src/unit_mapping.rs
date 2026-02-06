//! Unit Mapping Parser
//!
//! Parses unit mapping strings in multiple formats:
//! - Conversion format: "4 lb = $5"
//! - Price-per format: "$5/4lb"
//! - With source: "4 lb = $5 @ costco"

use crate::parser::parse_amount_string;
use crate::unit::Measure;
use serde::{Deserialize, Serialize};

/// Parsed unit mapping with optional source
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct ParsedUnitMapping {
    pub a: Measure,
    pub b: Measure,
    pub source: Option<String>,
}

/// Parse a unit mapping string in multiple formats:
/// - "4 lb = $5" (conversion format)
/// - "$5/4lb" (price-per format)
/// - "4 lb = $5 @ costco" (with source)
///
/// # Examples
/// ```
/// use ingredient::unit_mapping::parse_unit_mapping;
///
/// let result = parse_unit_mapping("4 lb = $5").unwrap();
/// assert_eq!(result.a.value(), 4.0);
/// assert_eq!(result.b.value(), 5.0);
/// ```
pub fn parse_unit_mapping(input: &str) -> Result<ParsedUnitMapping, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("Empty input".to_string());
    }

    // Extract source if present: "... @ source"
    let (mapping_part, source) = extract_source(input);

    // Try conversion format: "4 lb = $5"
    if let Some((left, right)) = mapping_part.split_once('=') {
        let a = parse_amount_string(left.trim())?;
        let b = parse_amount_string(right.trim())?;
        return Ok(ParsedUnitMapping { a, b, source });
    }

    // Try price-per format: "$5/4lb"
    if let Some(result) = try_parse_price_per(mapping_part) {
        let (price, amount) = result?;
        return Ok(ParsedUnitMapping {
            a: amount,
            b: price,
            source,
        });
    }

    Err(format!(
        "Invalid unit mapping format: '{input}'. Expected format: '4 lb = $5' or '$5/4lb'"
    ))
}

/// Extract source from input if present
/// "4 lb = $5 @ costco" -> ("4 lb = $5", Some("costco"))
fn extract_source(input: &str) -> (&str, Option<String>) {
    // Look for @ from the end to handle cases like "some @ sign = $5 @ store"
    if let Some(at_pos) = input.rfind(" @ ") {
        let mapping_part = input[..at_pos].trim();
        let source = input[at_pos + 3..].trim();
        if !source.is_empty() {
            return (mapping_part, Some(source.to_string()));
        }
    }
    (input, None)
}

/// Try to parse price-per format: "$5/4lb" or "$5/4 lb"
/// Returns None if not in price-per format, Some(Result) if it looks like price-per
fn try_parse_price_per(input: &str) -> Option<Result<(Measure, Measure), String>> {
    // Must start with $ and contain /
    if !input.starts_with('$') || !input.contains('/') {
        return None;
    }

    // Split on / to get price and amount parts
    let slash_pos = input.find('/')?;
    let price_str = &input[..slash_pos];
    let amount_str = &input[slash_pos + 1..];

    Some((|| {
        let price = parse_amount_string(price_str.trim())?;
        let amount = parse_amount_string(amount_str.trim())?;
        Ok((price, amount))
    })())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use rstest::rstest;

    // ============================================================================
    // Conversion Format Tests
    // ============================================================================

    #[rstest]
    #[case::with_spaces("4 lb = $5", 4.0, "lb", 5.0, "$")]
    #[case::no_spaces("4lb=$5", 4.0, "lb", 5.0, "$")]
    #[case::weight_conversion("1 cup = 120g", 1.0, "cup", 120.0, "g")]
    #[case::decimal_values("2.5 cups = $3.50", 2.5, "cups", 3.5, "$")]
    fn test_conversion_format(
        #[case] input: &str,
        #[case] a_val: f64,
        #[case] a_unit: &str,
        #[case] b_val: f64,
        #[case] b_unit: &str,
    ) {
        let result = parse_unit_mapping(input).unwrap();
        assert_eq!(result.a.value(), a_val);
        assert_eq!(result.a.unit_as_string(), a_unit);
        assert_eq!(result.b.value(), b_val);
        assert_eq!(result.b.unit_as_string(), b_unit);
        assert_eq!(result.source, None);
    }

    // ============================================================================
    // Price-per Format Tests
    // ============================================================================

    #[rstest]
    #[case::no_space("$5/4lb", 4.0, "lb", 5.0)]
    #[case::with_space("$5/4 lb", 4.0, "lb", 5.0)]
    fn test_price_per_format(
        #[case] input: &str,
        #[case] a_val: f64,
        #[case] a_unit: &str,
        #[case] b_val: f64,
    ) {
        let result = parse_unit_mapping(input).unwrap();
        assert_eq!(result.a.value(), a_val);
        assert_eq!(result.a.unit_as_string(), a_unit);
        assert_eq!(result.b.value(), b_val);
        assert_eq!(result.b.unit_as_string(), "$");
    }

    // ============================================================================
    // Source Extraction Tests
    // ============================================================================

    #[rstest]
    #[case::conversion_with_source("4 lb = $5 @ costco", 4.0, 5.0, "costco")]
    #[case::price_per_with_source("$5/4lb @ whole foods", 4.0, 5.0, "whole foods")]
    fn test_with_source(
        #[case] input: &str,
        #[case] a_val: f64,
        #[case] b_val: f64,
        #[case] source: &str,
    ) {
        let result = parse_unit_mapping(input).unwrap();
        assert_eq!(result.a.value(), a_val);
        assert_eq!(result.b.value(), b_val);
        assert_eq!(result.source, Some(source.to_string()));
    }

    #[rstest]
    #[case::with_source("4 lb = $5 @ costco", "4 lb = $5", Some("costco".to_string()))]
    #[case::no_source("4 lb = $5", "4 lb = $5", None)]
    #[case::price_per_source("$5/4lb @ whole foods", "$5/4lb", Some("whole foods".to_string()))]
    #[case::empty_source("4 lb = $5 @ ", "4 lb = $5 @ ", None)]
    #[case::whitespace_only_source("4 lb = $5 @   ", "4 lb = $5 @   ", None)]
    fn test_extract_source(
        #[case] input: &str,
        #[case] mapping: &str,
        #[case] source: Option<String>,
    ) {
        assert_eq!(extract_source(input), (mapping, source));
    }

    // ============================================================================
    // Invalid Format Tests
    // ============================================================================

    #[rstest]
    #[case::invalid("invalid")]
    #[case::missing_equals("4 lb")]
    #[case::missing_left("= $5")]
    #[case::empty("")]
    fn test_invalid_format(#[case] input: &str) {
        assert!(parse_unit_mapping(input).is_err());
    }

    // ============================================================================
    // Unit Singularization Test
    // ============================================================================

    #[test]
    fn test_unit_singularization() {
        let result = parse_unit_mapping("2.5 cups = $3.50").unwrap();
        assert_eq!(result.a.unit().to_str(), "cup");
        assert_eq!(result.b.unit().to_str(), "$");
    }
}
