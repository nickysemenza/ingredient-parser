use std::collections::HashSet;

use crate::{parser::MeasurementParser, unit::Measure, IngredientParser, Res};
use nom::{branch::alt, character::complete::satisfy, error::context, multi::many0, Parser};

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde-derive", derive(Serialize, Deserialize))]
#[serde(tag = "kind", content = "value")]
pub enum Chunk {
    Measure(Vec<Measure>),
    Text(String),
    Ing(String),
}
pub type Rich = Vec<Chunk>;
fn condense_text(r: Rich) -> Rich {
    let mut result = Vec::with_capacity(r.len());
    for chunk in r {
        match (&mut result.last_mut(), &chunk) {
            (Some(Chunk::Text(prev)), Chunk::Text(new)) => {
                prev.push_str(new);
            }
            _ => result.push(chunk),
        }
    }
    result
}
// find any text chunks which have an ingredient name as a substring in them.
// if so, split on the ingredient name, giving it it's own `Chunk::Ing`.
fn extract_ingredients(r: Rich, ingredient_names: &[String]) -> Rich {
    r.into_iter()
        .flat_map(|s| match s {
            Chunk::Text(mut text) => {
                // let mut a = s;
                let mut text_or_ing_res = vec![];

                for candidate in ingredient_names.iter().filter(|x| !x.is_empty()) {
                    if let Some((prefix, suffix)) = text.split_once(candidate) {
                        text_or_ing_res.push(Chunk::Text(prefix.to_string()));
                        text_or_ing_res.push(Chunk::Ing(candidate.to_string()));
                        text = suffix.to_string();
                    }
                }
                if !text.is_empty() {
                    // ignore empty
                    text_or_ing_res.push(Chunk::Text(text));
                }

                text_or_ing_res
            }
            _ => vec![s.clone()],
        })
        .collect()
}

fn amounts_chunk<'a>(units: &HashSet<String>, input: &'a str) -> Res<&'a str, Chunk> {
    // Always use rich text mode (true) for instruction parsing
    let mp = MeasurementParser::new(units, true);
    context("amounts_chunk", |a| mp.parse_measurement_list(a))
        .parse(input)
        .map(|(next_input, res)| (next_input, Chunk::Measure(res)))
}
fn text_chunk(input: &str) -> Res<&str, Chunk> {
    parse_rich_char(input).map(|(next_input, res)| (next_input, Chunk::Text(res)))
}
/// Parse a single text character for rich text (recipe instructions).
///
/// Allows: alphanumeric, whitespace, plus additional punctuation
/// (commas, parentheses, semicolons, colons, slashes, etc.)
///
/// Note: This is more permissive than `parser::helpers::parse_ingredient_text()` which is
/// designed for ingredient names only.
fn parse_rich_char(input: &str) -> Res<&str, String> {
    satisfy(|c| match c {
        '-' | '\u{2014}' | '\'' | '\u{2019}' | '.' | '\\' => true,
        ',' | '(' | ')' | ';' | '#' | '/' | ':' | '!' => true,
        c => c.is_alphanumeric() || c.is_whitespace(),
    })
    .parse(input)
    .map(|(next_input, res)| (next_input, res.to_string()))
}
/// Parse some rich text that has some parsable [Measure] scattered around in it. Useful for displaying text with fancy formatting.
/// returns [Rich]
/// ```
/// use ingredient::{unit::Measure, rich_text::{RichParser, Chunk}};
/// assert_eq!(
/// RichParser::new(vec![]).parse("hello 1 cups foo bar").unwrap(),
/// vec![
///     Chunk::Text("hello ".to_string()),
///     Chunk::Measure(vec![Measure::new("cups", 1.0)]),
///     Chunk::Text(" foo bar".to_string())
/// ]
/// );
/// ```
#[derive(Clone, PartialEq, Debug, Default)]
pub struct RichParser {
    ingredient_names: Vec<String>,
    ip: IngredientParser,
}
impl RichParser {
    /// Create a new RichParser for parsing recipe instructions
    ///
    /// # Arguments
    ///
    /// * `ingredient_names` - List of ingredient names to highlight in the text
    ///
    /// # Example
    /// ```
    /// use ingredient::rich_text::RichParser;
    ///
    /// let parser = RichParser::new(vec!["flour".to_string(), "sugar".to_string()]);
    /// let chunks = parser.parse("Add 2 cups flour").unwrap();
    /// ```
    pub fn new(ingredient_names: Vec<String>) -> Self {
        Self {
            ingredient_names,
            ip: IngredientParser::new(),
        }
    }

    #[tracing::instrument]
    pub fn parse(&self, input: &str) -> Result<Rich, String> {
        let units = self.ip.units();
        match context(
            "amts",
            many0(alt((|a| amounts_chunk(units, a), text_chunk))),
        )
        .parse(input)
        {
            Ok((_, res)) => Ok(extract_ingredients(
                condense_text(res),
                &self.ingredient_names,
            )),
            Err(e) => Err(format!("unable to parse '{input}': {e}")),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use rstest::{fixture, rstest};

    #[fixture]
    fn parser() -> RichParser {
        RichParser::new(vec![])
    }

    #[fixture]
    fn parser_with_ingredients() -> RichParser {
        RichParser::new(vec!["flour".to_string(), "sugar".to_string()])
    }

    // ============================================================================
    // RichParser Basic Tests
    // ============================================================================

    #[rstest]
    fn test_rich_parser_basic(parser: RichParser) {
        let result = parser.parse("hello 1 cup foo bar").unwrap();
        assert_eq!(result.len(), 3);
        assert!(matches!(result[0], Chunk::Text(_)));
        assert!(matches!(result[1], Chunk::Measure(_)));
        assert!(matches!(result[2], Chunk::Text(_)));
    }

    #[rstest]
    fn test_rich_parser_empty_input(parser: RichParser) {
        let result = parser.parse("").unwrap();
        assert!(result.is_empty());
    }

    #[rstest]
    fn test_rich_parser_only_text(parser: RichParser) {
        let result = parser.parse("just some text").unwrap();
        assert_eq!(result.len(), 1);
        assert!(matches!(&result[0], Chunk::Text(s) if s == "just some text"));
    }

    #[rstest]
    fn test_rich_parser_bullet_proof_text(parser: RichParser) {
        // "bullet-proof" should NOT be parsed as containing a measurement
        let result = parser.parse("This bullet-proof recipe translates").unwrap();
        // Should be a single text chunk with no measurements extracted
        let has_measure = result.iter().any(|c| matches!(c, Chunk::Measure(_)));
        assert!(
            !has_measure,
            "bullet-proof should not extract measurements: {result:?}"
        );
    }

    #[rstest]
    fn test_rich_parser_multiple_measures(parser: RichParser) {
        let result = parser.parse("Mix 1 cup flour with 2 tbsp sugar").unwrap();
        let measures: Vec<_> = result
            .iter()
            .filter(|c| matches!(c, Chunk::Measure(_)))
            .collect();
        assert_eq!(measures.len(), 2);
    }

    #[rstest]
    fn test_rich_parser_with_ingredients(parser_with_ingredients: RichParser) {
        let result = parser_with_ingredients
            .parse("Add 2 cups flour and sugar")
            .unwrap();

        let has_flour = result
            .iter()
            .any(|c| matches!(c, Chunk::Ing(s) if s == "flour"));
        let has_sugar = result
            .iter()
            .any(|c| matches!(c, Chunk::Ing(s) if s == "sugar"));
        assert!(has_flour);
        assert!(has_sugar);
    }

    #[test]
    fn test_rich_parser_default() {
        let parser: RichParser = Default::default();
        let result = parser.parse("1 cup").unwrap();
        assert!(!result.is_empty());
    }

    // ============================================================================
    // Condense Text Tests
    // ============================================================================

    #[test]
    fn test_condense_text_adjacent() {
        let chunks = vec![
            Chunk::Text("hello ".to_string()),
            Chunk::Text("world".to_string()),
        ];
        let condensed = condense_text(chunks);
        assert_eq!(condensed.len(), 1);
        assert!(matches!(&condensed[0], Chunk::Text(s) if s == "hello world"));
    }

    #[test]
    fn test_condense_text_mixed() {
        let chunks = vec![
            Chunk::Text("hello ".to_string()),
            Chunk::Measure(vec![Measure::new("cup", 1.0)]),
            Chunk::Text(" world".to_string()),
        ];
        let condensed = condense_text(chunks);
        assert_eq!(condensed.len(), 3);
    }

    // ============================================================================
    // Extract Ingredients Tests
    // ============================================================================

    #[test]
    fn test_extract_ingredients() {
        let chunks = vec![Chunk::Text("Add flour and sugar".to_string())];
        let names = vec!["flour".to_string(), "sugar".to_string()];
        let result = extract_ingredients(chunks, &names);

        let has_flour = result
            .iter()
            .any(|c| matches!(c, Chunk::Ing(s) if s == "flour"));
        let has_sugar = result
            .iter()
            .any(|c| matches!(c, Chunk::Ing(s) if s == "sugar"));
        assert!(has_flour);
        assert!(has_sugar);
    }

    #[test]
    fn test_extract_ingredients_empty_name() {
        let chunks = vec![Chunk::Text("some text".to_string())];
        let names = vec!["".to_string(), "flour".to_string()];
        let result = extract_ingredients(chunks, &names);
        assert!(!result.is_empty());
    }

    // ============================================================================
    // parse_rich_char() Parser Tests
    // ============================================================================

    #[rstest]
    #[case::exclamation("hello!")]
    #[case::parenthesis("(test)")]
    #[case::semicolon(";")]
    #[case::colon(":")]
    #[case::comma(",")]
    #[case::slash("/")]
    #[case::hash("#")]
    fn test_parse_rich_char_special_chars(#[case] input: &str) {
        assert!(parse_rich_char(input).is_ok());
    }

    // ============================================================================
    // Cooking Terms Should Not Be Measurements
    // ============================================================================

    #[rstest]
    #[case::medium_speed("on medium speed until done")]
    #[case::high_heat("over high heat for 5 minutes")]
    #[case::low_heat("simmer on low heat")]
    #[case::medium_high_heat("cook over medium heat")]
    #[case::small_sheet_tray("on a small sheet tray")]
    #[case::large_pot("in a large pot")]
    #[case::medium_bowl("in a medium bowl")]
    #[case::small_saucepan("in a small saucepan")]
    fn test_cooking_terms_not_measurements(parser: RichParser, #[case] input: &str) {
        let result = parser.parse(input).unwrap();
        // "medium speed", "high heat", etc. should NOT produce a Measure chunk
        // with units like "medium" or "high"
        let has_size_measure = result.iter().any(|c| {
            matches!(c, Chunk::Measure(measures) if measures.iter().any(|m| {
                let unit_str = m.unit().to_string().to_lowercase();
                matches!(unit_str.as_str(), "small" | "medium" | "large" | "high" | "low")
            }))
        });
        assert!(
            !has_size_measure,
            "Should not parse cooking terms as measurements: {input:?} -> {result:?}"
        );
    }

    // ============================================================================
    // Chunk Tests
    // ============================================================================

    #[test]
    fn test_chunk_clone() {
        let chunk = Chunk::Text("test".to_string());
        let cloned = chunk.clone();
        assert_eq!(chunk, cloned);
    }

    // Regression test: numbers followed by periods and capitalized words (like oven temps)
    // should be parsed as measurements, not rejected as step numbers
    #[test]
    fn test_oven_temperature_parsing() {
        let parser = RichParser::new(vec![]);
        let result = parser.parse("Heat oven to 375. Combine flour").unwrap();
        // "375" should be parsed as a Measure with Whole unit
        let has_375 = result.iter().any(|c| match c {
            Chunk::Measure(measures) => measures.iter().any(|m| (m.value() - 375.0).abs() < 0.01),
            _ => false,
        });
        assert!(has_375, "Should parse 375 as a measurement: {result:?}");
    }
}
