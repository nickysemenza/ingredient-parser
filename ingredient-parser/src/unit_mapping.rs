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
/// assert_eq!(result.a.values().0, 4.0);
/// assert_eq!(result.b.values().0, 5.0);
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

    #[test]
    fn test_conversion_format() {
        let result = parse_unit_mapping("4 lb = $5").unwrap();
        assert_eq!(result.a.values().0, 4.0);
        assert_eq!(result.a.values().2, "lb");
        assert_eq!(result.b.values().0, 5.0);
        assert_eq!(result.b.values().2, "$"); // Dollar's canonical form is "$"
        assert_eq!(result.source, None);
    }

    #[test]
    fn test_conversion_format_no_spaces() {
        let result = parse_unit_mapping("4lb=$5").unwrap();
        assert_eq!(result.a.values().0, 4.0);
        assert_eq!(result.a.values().2, "lb");
        assert_eq!(result.b.values().0, 5.0);
    }

    #[test]
    fn test_price_per_format() {
        let result = parse_unit_mapping("$5/4lb").unwrap();
        // Note: a is the amount, b is the price (normalized order)
        assert_eq!(result.a.values().0, 4.0);
        assert_eq!(result.a.values().2, "lb");
        assert_eq!(result.b.values().0, 5.0);
        assert_eq!(result.b.values().2, "$"); // Dollar's canonical form is "$"
    }

    #[test]
    fn test_price_per_format_with_space() {
        let result = parse_unit_mapping("$5/4 lb").unwrap();
        assert_eq!(result.a.values().0, 4.0);
        assert_eq!(result.a.values().2, "lb");
    }

    #[test]
    fn test_with_source() {
        let result = parse_unit_mapping("4 lb = $5 @ costco").unwrap();
        assert_eq!(result.a.values().0, 4.0);
        assert_eq!(result.b.values().0, 5.0);
        assert_eq!(result.source, Some("costco".to_string()));
    }

    #[test]
    fn test_price_per_with_source() {
        let result = parse_unit_mapping("$5/4lb @ whole foods").unwrap();
        assert_eq!(result.a.values().0, 4.0);
        assert_eq!(result.b.values().0, 5.0);
        assert_eq!(result.source, Some("whole foods".to_string()));
    }

    #[test]
    fn test_weight_conversion() {
        let result = parse_unit_mapping("1 cup = 120g").unwrap();
        assert_eq!(result.a.values().0, 1.0);
        assert_eq!(result.a.values().2, "cup");
        assert_eq!(result.b.values().0, 120.0);
        assert_eq!(result.b.values().2, "g");
    }

    #[test]
    fn test_decimal_values() {
        let result = parse_unit_mapping("2.5 cups = $3.50").unwrap();
        assert_eq!(result.a.values().0, 2.5);
        assert_eq!(result.b.values().0, 3.5);
    }

    #[test]
    fn test_invalid_format() {
        assert!(parse_unit_mapping("invalid").is_err());
        assert!(parse_unit_mapping("4 lb").is_err());
        assert!(parse_unit_mapping("= $5").is_err());
        assert!(parse_unit_mapping("").is_err());
    }

    #[test]
    fn test_extract_source() {
        assert_eq!(
            extract_source("4 lb = $5 @ costco"),
            ("4 lb = $5", Some("costco".to_string()))
        );
        assert_eq!(extract_source("4 lb = $5"), ("4 lb = $5", None));
        assert_eq!(
            extract_source("$5/4lb @ whole foods"),
            ("$5/4lb", Some("whole foods".to_string()))
        );
    }

    #[test]
    fn test_unit_singularization() {
        let result = parse_unit_mapping("2.5 cups = $3.50").unwrap();
        // Use unit().to_str() which returns canonical singular form
        assert_eq!(result.a.unit().to_str(), "cup");
        assert_eq!(result.b.unit().to_str(), "$");
    }
}
