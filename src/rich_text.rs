use crate::{many_amount, text, Amount, Res};
use itertools::Itertools;
use nom::{branch::alt, error::context, multi::many1};

#[derive(Debug, Clone, PartialEq)]
pub enum Chunk {
    Amount(Vec<Amount>),
    Text(String),
}
pub type Rich = Vec<Chunk>;
fn condense(r: Rich) -> Rich {
    r.into_iter()
        .coalesce(
            |previous, current| match (previous.clone(), current.clone()) {
                (Chunk::Text(a), Chunk::Text(b)) => Ok(Chunk::Text(format!("{}{}", a, b))),
                _ => Err((previous, current)),
            },
        )
        .collect()
}

fn amounts_chunk(input: &str) -> Res<&str, Chunk> {
    context("amounts_chunk", many_amount)(input)
        .map(|(next_input, res)| (next_input, Chunk::Amount(res)))
}
fn text_chunk(input: &str) -> Res<&str, Chunk> {
    context("text_chunk", text)(input)
        .map(|(next_input, res)| (next_input, Chunk::Text(res.to_string())))
}

/// Parse some rich text that has some parsable [Amount] scattered around in it. Useful for displaying text with fancy formatting.
/// returns [Rich]
/// ```
/// use ingredient::{Amount, rich_text::{parse, Chunk}};
/// assert_eq!(
/// parse("hello 1 cups foo bar"),
/// vec![
/// 	Chunk::Text("hello ".to_string()),
/// 	Chunk::Amount(vec![Amount::new("cups", 1.0)]),
/// 	Chunk::Text(" foo bar".to_string())
/// ]
/// );
/// ```
pub fn parse(input: &str) -> Rich {
    condense(
        context("amts", many1(alt((text_chunk, amounts_chunk))))(input)
            .unwrap()
            .1,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rich_text() {
        assert_eq!(
            parse("hello 1 cups foo bar"),
            vec![
                Chunk::Text("hello ".to_string()),
                Chunk::Amount(vec![Amount::new("cups", 1.0)]),
                Chunk::Text(" foo bar".to_string())
            ]
        );
        assert_eq!(
            parse("2-2 1/2 cups foo' bar"),
            vec![
                Chunk::Amount(vec![Amount::new_with_upper("cups", 2.0, 2.5)]),
                Chunk::Text(" foo' bar".to_string())
            ]
        );
    }
}
