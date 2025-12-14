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
// text2 is like text, but allows for more ambiguous characters when parsing text but not caring about ingredient names
fn text2(input: &str) -> Res<&str, String> {
    (satisfy(|c| match c {
        '-' | '—' | '\'' | '’' | '.' | '\\' => true,
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
