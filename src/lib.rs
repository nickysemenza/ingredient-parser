use nom::{
    branch::alt,
    bytes::complete::{ tag},
    character::complete::{alpha1,  space0, space1},
    combinator::opt,
    error::context,
    multi::{many0, many1},
    number::complete::float,
    sequence::{ tuple},
};

extern crate nom;

#[derive(Debug, PartialEq)]
pub struct Amount {
    unit: String,
    value: f32,
}
#[derive(Debug, PartialEq)]
pub struct Ingredient {
    name: String,
    amounts: Vec<Amount>,
    modifier: Option<String>,
}

/// Parse an ingredient line item, such as `120 grams / 1 cup whole wheat flour, sifted lightly`
/// into a `Ingredient`
///
/// supported formats:
/// 1 g name
/// 1 g / 1g name, modifier
/// 1 g; 1 g name
///
/// TODO (formats):
/// 1 g name (about 1 g; 1 g)
/// 1 g (1 g) name
///
/// TODO (other):
/// preparse: convert fractions to floats
/// preparse: convert vulgar fractions to floats

// full ingredient parser
pub fn ingredient(input: &str) -> nom::IResult<&str, Ingredient> {
    context(
        "ing",
        tuple((
            alt((
                amount2, // 1g / 1 g
                // OR
                amount1, // 1g
            )),
            space0,                       // space between amt and name
            many1(alt((alpha1, space1))), // name, can be multiple words
            opt(tag(", ")),               // comma seperates the modifier
            many0(alt((alpha1, space1))), // modifier, can be multiple words
        )),
    )(input)
    .map(|(next_input, res)| {
        let (amounts, _, name_chunks, _, modifier_chunks) = res;
        let m = modifier_chunks.join("");
        (
            next_input,
            Ingredient {
                name: name_chunks.join(""),
                amounts: amounts,
                modifier: match m.chars().count() {
                    0 => None,
                    _ => Some(m),
                },
            },
        )
    })
}

// parses 2 amounts, seperated by ; or /  
fn amount2(input: &str) -> nom::IResult<&str, Vec<Amount>> {
    context(
        "amount2",
        nom::sequence::separated_pair(amount1, alt((tag("; "), tag(" / "))), amount1),
    )(input)
    .map(|(next_input, res)| {
        let (a, b) = res;
        (next_input, a.into_iter().chain(b.into_iter()).collect())
    })
}

// parses a single amount
fn amount1(input: &str) -> nom::IResult<&str, Vec<Amount>> {
    context("amount1", tuple((float, space0, alpha1)))(input).map(|(next_input, res)| {
        let (value, _, unit) = res;
        println!("foo{:?}", res);
        (
            next_input,
            vec![Amount {
                unit: unit.to_string(),
                value,
            }],
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ingredient_parse() {
        assert_eq!(
            ingredient("12 cups flour"),
            Ok((
                "",
                Ingredient {
                    name: "flour".to_string(),
                    amounts: vec![Amount {
                        unit: "cups".to_string(),
                        value: 12.0
                    }],
                    modifier: None,
                }
            ))
        );
        assert_eq!(
            ingredient("12 cups flour, lightly sifted"),
            Ok((
                "",
                Ingredient {
                    name: "flour".to_string(),
                    amounts: vec![Amount {
                        unit: "cups".to_string(),
                        value: 12.0
                    }],
                    modifier: Some("lightly sifted".to_string()),
                }
            ))
        );
      
        assert_eq!(
            ingredient("12 cups / 2.3 grams flour"),
            Ok((
                "",
                Ingredient {
                    name: "flour".to_string(),
                    amounts: vec![
                        Amount {
                            unit: "cups".to_string(),
                            value: 12.0
                        },
                        Amount {
                            unit: "grams".to_string(),
                            value: 2.3
                        }
                    ],
                    modifier: None,
                }
            ))
        );
    }
}
