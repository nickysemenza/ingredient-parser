use crate::{unit::Measure, IngredientParser, Res};
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
        .coalesce(
            |previous, current| match (&previous, &current) {
                (Chunk::Text(a), Chunk::Text(b)) => Ok(Chunk::Text(format!("{a}{b}"))),
                _ => Err((previous, current)),
            },
        )
        .collect()
}
// find any text chunks which have an ingredient name as a substring in them.
// if so, split on the ingredient name, giving it it's own `Chunk::Ing`.
fn extract_ingredients(r: Rich, ingredient_names: Vec<String>) -> Rich {
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

fn amounts_chunk(ip: IngredientParser, input: &str) -> Res<&str, Chunk> {
    context("amounts_chunk", |a| ip.clone().parse_measurement_list(a))
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
/// use ingredient::{unit::Measure, IngredientParser, rich_text::{RichParser, Chunk}};
/// assert_eq!(
/// (RichParser {
/// ingredient_names: vec![],
/// ip: IngredientParser::new(true),
/// }).parse("hello 1 cups foo bar").unwrap(),
/// vec![
///     Chunk::Text("hello ".to_string()),
///     Chunk::Measure(vec![Measure::parse_new("cups", 1.0)]),
///     Chunk::Text(" foo bar".to_string())
/// ]
/// );
/// ```
#[derive(Clone, PartialEq, Debug, Default)]
pub struct RichParser {
    pub ingredient_names: Vec<String>,
    pub ip: IngredientParser,
}
impl RichParser {
    #[tracing::instrument]
    pub fn parse(self, input: &str) -> Result<Rich, String> {
        match context(
            "amts",
            many0(alt((|a| amounts_chunk(self.ip.clone(), a), text_chunk))),
        )
        .parse(input)
        {
            Ok((_, res)) => Ok(extract_ingredients(
                condense_text(res),
                self.ingredient_names.clone(),
            )),
            Err(e) => Err(format!("unable to parse '{input}': {e}")),
        }
    }
}
