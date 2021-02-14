use std::fmt;

use nom::{
    branch::alt,
    bytes::complete::tag,
    character::complete::{alpha1, satisfy, space0, space1},
    combinator::opt,
    error::context,
    multi::{many0, many1},
    number::complete::float,
    sequence::tuple,
};

extern crate nom;

#[derive(Debug, PartialEq)]
pub struct Amount {
    unit: String,
    value: f32,
}

impl fmt::Display for Amount {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {}", self.value, self.unit)
    }
}

#[derive(Debug, PartialEq)]
pub struct Ingredient {
    name: String,
    amounts: Vec<Amount>,
    modifier: Option<String>,
}

impl fmt::Display for Ingredient {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let amounts: Vec<String> = self.amounts.iter().map(|id| id.to_string()).collect();
        write!(f, "{} {}", amounts.join(" / "), self.name)
    }
}

pub fn ingredient(input: &str) -> Result<Ingredient, String> {
    return match parse_ingredient(input) {
        Ok(r) => Ok(r.1),
        Err(e) => Err(format!("failed to parse '{}': {}", input, e)),
    };
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
fn parse_ingredient(input: &str) -> nom::IResult<&str, Ingredient> {
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
                amounts,
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
    context(
        "amount1",
        tuple(
            (num, space0, alpha1), // 1 gram
        ),
    )(input)
    .map(|(next_input, res)| {
        let (value, _, unit) = res;
        (
            next_input,
            vec![Amount {
                unit: unit.to_string(),
                value,
            }],
        )
    })
}

pub fn v_frac_to_num(input: &char) -> Result<f32, String> {
    let (n, d): (i32, i32) = match input {
        '¾' => (3, 4),
        '⅛' => (1, 8),
        '¼' => (1, 4),
        '⅓' => (1, 3),
        '½' => (1, 2),
        _ => return Err(format!("unkown fraction: {}", input)),
    };
    return Ok(n as f32 / d as f32);
}

/// parses unicode vulgar fractions
pub fn v_fraction(input: &str) -> nom::IResult<&str, char> {
    context(
        "v_fraction",
        satisfy(|c|
            // two ranges for unicode fractions
            // https://www.compart.com/en/unicode/search?q=vulgar+fraction#characters
            (c <= '¾' && c >= '¼') || (c >= '⅐' && c <= '⅞')),
    )(input)
}

/// handles vulgar fraction, or just a number
pub fn num(input: &str) -> nom::IResult<&str, f32> {
    context("num", alt((fraction_number, float)))(input)
}
/// parses `1 ⅛` into 1.125
pub fn fraction_number(input: &str) -> nom::IResult<&str, f32> {
    context(
        "fraction_number",
        tuple((
            opt(tuple((float, space0))), // optional number (and if number, optional space) before
            v_fraction,                  // vulgar frac
        )),
    )(input)
    .map(|(next_input, res)| {
        let (num, frac) = res;
        let num1 = match num {
            Some(x) => x.0,
            None => 0.0,
        };
        let num2 = match v_frac_to_num(&frac) {
            Ok(x) => x,
            _ => 0.0,
        };
        (next_input, num1 + num2)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fraction() {
        assert_eq!(fraction_number("1 ⅛"), Ok(("", 1.125)));
        assert_eq!(fraction_number("1⅓"), Ok(("", 1.3333334)));
        assert_eq!(fraction_number("¼"), Ok(("", 0.25)));
        assert_eq!(fraction_number("⅐"), Ok(("", 0.0))); // unkown are dropped
        assert_eq!(
            fraction_number("1"),
            Err(nom::Err::Error(nom::error::Error::new(
                "",
                nom::error::ErrorKind::Satisfy
            )))
        );
        // assert_eq!(strip_frac("1 ⅛"), "a");
    }
    #[test]
    fn test_v_fraction() {
        assert_eq!(v_frac_to_num(&'⅛'), Ok(0.125));
        assert_eq!(v_frac_to_num(&'¼'), Ok(0.25));
        assert_eq!(v_frac_to_num(&'½'), Ok(0.5));
        // assert_eq!(strip_frac("1 ⅛"), "a");
    }

    #[test]
    fn test_ingredient_parse() {
        assert_eq!(
            parse_ingredient("12 cups flour"),
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
            parse_ingredient("12 cups all purpose flour, lightly sifted"),
            Ok((
                "",
                Ingredient {
                    name: "all purpose flour".to_string(),
                    amounts: vec![Amount {
                        unit: "cups".to_string(),
                        value: 12.0
                    }],
                    modifier: Some("lightly sifted".to_string()),
                }
            ))
        );

        assert_eq!(
            parse_ingredient("1¼  cups / 155.5 grams flour"),
            Ok((
                "",
                Ingredient {
                    name: "flour".to_string(),
                    amounts: vec![
                        Amount {
                            unit: "cups".to_string(),
                            value: 1.25
                        },
                        Amount {
                            unit: "grams".to_string(),
                            value: 155.5
                        }
                    ],
                    modifier: None,
                }
            ))
        );
    }
}
