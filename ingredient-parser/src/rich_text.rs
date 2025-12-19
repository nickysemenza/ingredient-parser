use std::collections::HashSet;

use crate::{parser::MeasurementParser, unit::Measure, IngredientParser, Res};
use itertools::Itertools;
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
    // https://www.reddit.com/r/rust/comments/e3mq41/combining_enum_values_with_itertools_coalesce/
    r.into_iter()
        .coalesce(|previous, current| match (&previous, &current) {
            (Chunk::Text(a), Chunk::Text(b)) => Ok(Chunk::Text(format!("{a}{b}"))),
            _ => Err((previous, current)),
        })
        .collect()
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
    text2(input).map(|(next_input, res)| (next_input, Chunk::Text(res)))
}
/// Parse text characters for rich text (recipe instructions).
///
/// Allows: alphanumeric, whitespace, plus additional punctuation
/// (commas, parentheses, semicolons, colons, slashes, etc.)
///
/// Note: This is more permissive than `parser::helpers::text()` which is
/// designed for ingredient names only.
fn text2(input: &str) -> Res<&str, String> {
    (satisfy(|c| match c {
        '-' | '—' | '\'' | '\u{2019}' | '.' | '\\' => true,
        ',' | '(' | ')' | ';' | '#' | '/' | ':' | '!' => true, // in text2 but not text
        c => c.is_alphanumeric() || c.is_whitespace(),
    }))(input)
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

    #[test]
    fn test_rich_parser_basic() {
        let parser = RichParser::new(vec![]);
        let result = parser.parse("hello 1 cup foo bar").unwrap();

        assert_eq!(result.len(), 3);
        assert!(matches!(result[0], Chunk::Text(_)));
        assert!(matches!(result[1], Chunk::Measure(_)));
        assert!(matches!(result[2], Chunk::Text(_)));
    }

    #[test]
    fn test_rich_parser_with_ingredients() {
        let parser = RichParser::new(vec!["flour".to_string(), "sugar".to_string()]);
        let result = parser.parse("Add 2 cups flour and sugar").unwrap();

        // Should have chunks for text, measure, text, ingredient, text, ingredient
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
    fn test_rich_parser_empty_input() {
        let parser = RichParser::new(vec![]);
        let result = parser.parse("").unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_rich_parser_only_text() {
        let parser = RichParser::new(vec![]);
        let result = parser.parse("just some text").unwrap();

        assert_eq!(result.len(), 1);
        assert!(matches!(&result[0], Chunk::Text(s) if s == "just some text"));
    }

    #[test]
    fn test_rich_parser_multiple_measures() {
        let parser = RichParser::new(vec![]);
        let result = parser.parse("Mix 1 cup flour with 2 tbsp sugar").unwrap();

        let measures: Vec<_> = result
            .iter()
            .filter(|c| matches!(c, Chunk::Measure(_)))
            .collect();
        assert_eq!(measures.len(), 2);
    }

    #[test]
    fn test_condense_text() {
        // Test that adjacent Text chunks are combined
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
        // Test that non-adjacent Text chunks aren't combined
        let chunks = vec![
            Chunk::Text("hello ".to_string()),
            Chunk::Measure(vec![Measure::new("cup", 1.0)]),
            Chunk::Text(" world".to_string()),
        ];
        let condensed = condense_text(chunks);
        assert_eq!(condensed.len(), 3);
    }

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
        // Empty ingredient names should be filtered out
        let chunks = vec![Chunk::Text("some text".to_string())];
        let names = vec!["".to_string(), "flour".to_string()];
        let result = extract_ingredients(chunks, &names);
        // Should not crash with empty name
        assert!(!result.is_empty());
    }

    #[test]
    fn test_rich_parser_default() {
        // Test Default impl
        let parser: RichParser = Default::default();
        let result = parser.parse("1 cup").unwrap();
        assert!(!result.is_empty());
    }

    #[test]
    fn test_text2_special_chars() {
        // Test text2 handles special characters
        let result = text2("hello!");
        assert!(result.is_ok());

        let result = text2("(test)");
        assert!(result.is_ok());

        let result = text2(";");
        assert!(result.is_ok());

        let result = text2(":");
        assert!(result.is_ok());
    }

    #[test]
    fn test_chunk_clone() {
        let chunk = Chunk::Text("test".to_string());
        let cloned = chunk.clone();
        assert_eq!(chunk, cloned);
    }
}
