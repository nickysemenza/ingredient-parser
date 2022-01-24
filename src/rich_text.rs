use crate::{text, Amount, IngredientParser, Res};
use itertools::Itertools;
use nom::{branch::alt, bytes::complete::tag, error::context, multi::many0};

#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "serde-derive", derive(Serialize, Deserialize))]
#[serde(tag = "kind", content = "value")]
pub enum Chunk {
    Amount(Vec<Amount>),
    Text(String),
    Ing(String),
}
pub type Rich = Vec<Chunk>;
fn condense_text(r: Rich) -> Rich {
    // https://www.reddit.com/r/rust/comments/e3mq41/combining_enum_values_with_itertools_coalesce/
    r.into_iter()
        .coalesce(
            |previous, current| match (previous.clone(), current.clone()) {
                (Chunk::Text(a), Chunk::Text(b)) => Ok(Chunk::Text(format!("{}{}", a, b))),
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
            Chunk::Text(mut a) => {
                // let mut a = s;
                let mut r = vec![];

                for i in ingredient_names.iter().filter(|x| x.len() > 0) {
                    match a.split_once(i) {
                        Some((prefix, suffix)) => {
                            r.push(Chunk::Text(prefix.to_string()));
                            r.push(Chunk::Ing(i.to_string()));
                            a = suffix.to_string();
                        }
                        None => {}
                    }
                }
                if a.len() > 0 {
                    // ignore empty
                    r.push(Chunk::Text(a));
                }

                r
            }
            _ => vec![s.clone()],
        })
        .collect()
}

fn amounts_chunk(ip: IngredientParser, input: &str) -> Res<&str, Chunk> {
    let res = context("amounts_chunk", |a| ip.clone().many_amount(a))(input)
        .map(|(next_input, res)| (next_input, Chunk::Amount(res)));
    return res;
}
fn text_chunk(input: &str) -> Res<&str, Chunk> {
    context("text_chunk", text2)(input)
        .map(|(next_input, res)| (next_input, Chunk::Text(res.to_string())))
}
// text2 is like text, but allows for more ambiguous characters when parsing text but not caring about ingredient names
fn text2(input: &str) -> Res<&str, &str> {
    alt((
        text,
        tag(","),
        tag("("),
        tag(")"),
        tag(";"),
        tag("#"),
        tag("’"),
        tag("ó"),
        tag("/"),
        tag(":"),
        tag("!"),
    ))(input)
}
/// Parse some rich text that has some parsable [Amount] scattered around in it. Useful for displaying text with fancy formatting.
/// returns [Rich]
/// ```
/// use ingredient::{Amount, IngredientParser, rich_text::{RichParser, Chunk}};
/// assert_eq!(
/// (RichParser {
/// ingredient_names: vec![],
/// ip: IngredientParser::new(),
/// }).parse("hello 1 cups foo bar").unwrap(),
/// vec![
/// 	Chunk::Text("hello ".to_string()),
/// 	Chunk::Amount(vec![Amount::new("cups", 1.0)]),
/// 	Chunk::Text(" foo bar".to_string())
/// ]
/// );
/// ```
#[derive(Clone, PartialEq, PartialOrd, Debug, Default)]
pub struct RichParser {
    pub ingredient_names: Vec<String>,
    pub ip: IngredientParser,
}
impl RichParser {
    pub fn parse(self, input: &str) -> Result<Rich, String> {
        match context(
            "amts",
            many0(alt((text_chunk, |a| amounts_chunk(self.ip.clone(), a)))),
        )(input)
        {
            Ok((_, res)) => Ok(extract_ingredients(
                condense_text(res),
                self.ingredient_names.clone(),
            )),
            Err(e) => Err(format!("unable to parse '{}': {}", input, e)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rich_text() {
        assert_eq!(
            (RichParser {
                ingredient_names: vec![],
                ip: IngredientParser::new(),
            })
            .parse("hello 1 cups foo bar")
            .unwrap(),
            vec![
                Chunk::Text("hello ".to_string()),
                Chunk::Amount(vec![Amount::new("cups", 1.0)]),
                Chunk::Text(" foo bar".to_string())
            ]
        );
        assert_eq!(
            (RichParser {
                ingredient_names: vec!["bar".to_string()],
                ip: IngredientParser::new(),
            })
            .parse("hello 1 cups foo bar")
            .unwrap(),
            vec![
                Chunk::Text("hello ".to_string()),
                Chunk::Amount(vec![Amount::new("cups", 1.0)]),
                // Chunk::Text(" foo bar".to_string()),
                Chunk::Text(" foo ".to_string()),
                Chunk::Ing("bar".to_string())
            ]
        );
        assert_eq!(
            (RichParser {
                ingredient_names: vec![],
                ip: IngredientParser::new(),
            })
            .parse("2-2 1/2 cups foo' bar")
            .unwrap(),
            vec![
                Chunk::Amount(vec![Amount::new_with_upper("cups", 2.0, 2.5)]),
                Chunk::Text(" foo' bar".to_string())
            ]
        );
    }
    #[test]
    fn test_rich_text_space() {
        assert_eq!(
            (RichParser {
                ingredient_names: vec!["foo bar".to_string()],
                ip: IngredientParser::new(),
            })
            .parse("hello 1 cups foo bar")
            .unwrap(),
            vec![
                Chunk::Text("hello ".to_string()),
                Chunk::Amount(vec![Amount::new("cups", 1.0)]),
                Chunk::Text(" ".to_string()),
                Chunk::Ing("foo bar".to_string()),
            ]
        );
    }
}
