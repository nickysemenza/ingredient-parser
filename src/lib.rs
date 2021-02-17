use std::fmt;

use nom::{
    branch::alt,
    bytes::complete::tag,
    character::complete::{alpha1, char, not_line_ending, satisfy, space0, space1},
    combinator::opt,
    error::{context, convert_error, VerboseError},
    multi::many1,
    number::complete::float,
    sequence::{delimited, tuple},
    Err as NomErr, IResult,
};

extern crate nom;

#[cfg(feature = "serde-derive")]
#[macro_use]
extern crate serde;

type Res<T, U> = IResult<T, U, VerboseError<T>>;

#[cfg_attr(feature = "serde-derive", derive(Serialize, Deserialize))]
#[derive(Debug, PartialEq)]
/// Holds a unit and value pair for an ingredient.
pub struct Amount {
    pub unit: String,
    pub value: f32,
}

impl fmt::Display for Amount {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} {}", self.value, self.unit)
    }
}
#[cfg_attr(feature = "serde-derive", derive(Serialize, Deserialize))]
#[derive(Debug, PartialEq)]
/// Holds a name, list of [Amount], and optional modifier string
pub struct Ingredient {
    pub name: String,
    pub amounts: Vec<Amount>,
    pub modifier: Option<String>,
}

impl fmt::Display for Ingredient {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let amounts: Vec<String> = self.amounts.iter().map(|id| id.to_string()).collect();
        let modifier = match self.modifier.clone() {
            Some(m) => format!(", {}", m),
            None => format!(""),
        };
        let amount_list = match amounts.len() {
            0 => format!(""),
            _ => format!("{} ", amounts.join(" / ")),
        };
        write!(f, "{}{}{}", amount_list, self.name, modifier)
    }
}
/// wrapper for [parse_ingredient]
/// ```
/// use ingredient::{ingredient};
/// assert_eq!(ingredient("one whole egg", true).unwrap().to_string(),"1 whole egg");
/// ```
pub fn ingredient(input: &str, verbose_error: bool) -> Result<Ingredient, String> {
    return match parse_ingredient(input) {
        Ok(r) => Ok(r.1),
        Err(e) => {
            let msg = match e {
                NomErr::Error(e) => {
                    if verbose_error {
                        convert_error(input, e)
                    } else {
                        format!("{}", e)
                    }
                }
                _ => format!("{}", e),
            };
            return Err(format!("failed to parse '{}': {}", input, msg));
        }
    };
}

/// Parse an ingredient line item, such as `120 grams / 1 cup whole wheat flour, sifted lightly`
/// into a [Ingredient]. [ingredient] can be used as a wrapper to return verbose errors.
///
/// supported formats include:
/// * 1 g name
/// * 1 g / 1g name, modifier
/// * 1 g; 1 g name
/// * ¼ g name
/// * 1/4 g name
/// * 1 ¼ g name
/// * 1 1/4 g name
/// * 1 g (1 g) name
/// * 1 g name (about 1 g; 1 g)
/// * name
/// * 1 name
/// ```
/// use ingredient::{parse_ingredient, Ingredient, Amount};
/// assert_eq!(
///     parse_ingredient("1¼  cups / 155.5 grams flour"),
///     Ok((
///         "",
///         Ingredient {
///             name: "flour".to_string(),
///             amounts: vec![
///                 Amount {
///                     unit: "cups".to_string(),
///                     value: 1.25
///                 },
///                 Amount {
///                     unit: "grams".to_string(),
///                     value: 155.5
///                 }
///             ],
///             modifier: None,
///         }
///     ))
/// );
/// ```
pub fn parse_ingredient(input: &str) -> Res<&str, Ingredient> {
    context(
        "ing",
        tuple((
            opt(alt((
                // amounts might be totally optional
                amount2, // 1g / 1 g
                // OR
                amount1, // 1g
            ))),
            space0,           // space between amount(s) and name
            opt(many1(text)), // name, can be multiple words
            opt(amt_parens),  // can have some more amounts in parens after the name
            opt(tag(", ")),   // comma seperates the modifier
            not_line_ending, // modifier, can be multiple words and even include numbers, since once we've hit the comma everything is fair game.
        )),
    )(input)
    .map(|(next_input, res)| {
        let (amounts, _, name_chunks, amounts2, _, modifier_chunks) = res;
        let m = modifier_chunks;

        let mut name = match name_chunks {
            Some(n) => n.join("").trim_matches(' ').to_string(),
            None => "".to_string(),
        };
        let mut amounts = match amounts {
            Some(a) => a,
            None => vec![],
        };
        amounts = match amounts2 {
            Some(a) => amounts.into_iter().chain(a.into_iter()).collect(),
            None => amounts,
        };

        // if we have a unit but no name, e.g. `1 egg`,
        // the unit is the name, so normalize it to `1 whole egg`,
        if name.len() == 0 && amounts.len() == 1 {
            name = amounts[0].unit.clone();
            amounts[0].unit = "whole".to_string();
        };
        (
            next_input,
            Ingredient {
                name,
                amounts,
                modifier: match m.chars().count() {
                    0 => None,
                    _ => Some(m.to_string()),
                },
            },
        )
    })
}

fn text(input: &str) -> Res<&str, &str> {
    alt((alpha1, space1, tag("-")))(input)
}

// parses 2 amounts, seperated by ; or /
fn amount2(input: &str) -> Res<&str, Vec<Amount>> {
    context(
        "amount2",
        nom::sequence::separated_pair(
            amount1,
            alt((tag("; "), tag(" / "), tag(" "), tag(", "))),
            alt((amt_parens, amount1)),
        ),
    )(input)
    .map(|(next_input, res)| {
        let (a, b) = res;
        (next_input, a.into_iter().chain(b.into_iter()).collect())
    })
}

// parses a single amount
fn amount1(input: &str) -> Res<&str, Vec<Amount>> {
    context(
        "amount1",
        tuple(
            (opt(tag("about ")), num, space0, alpha1), // 1 gram
        ),
    )(input)
    .map(|(next_input, res)| {
        let (_, value, _, unit) = res;
        (
            next_input,
            vec![Amount {
                unit: unit.to_string(),
                value,
            }],
        )
    })
}
fn amt_parens(input: &str) -> Res<&str, Vec<Amount>> {
    context(
        "amt_parens",
        delimited(char('('), alt((amount2, amount1)), char(')')),
    )(input)
}

fn v_frac_to_num(input: &char) -> Result<f32, String> {
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
// two ranges for unicode fractions
// https://www.compart.com/en/unicode/search?q=vulgar+fraction#characters
fn is_frac_char(c: char) -> bool {
    match c {
        '¼'..='¾' => true,
        '⅐'..='⅞' => true,
        _ => false,
    }
}
/// parses unicode vulgar fractions
fn v_fraction(input: &str) -> Res<&str, f32> {
    context("v_fraction", satisfy(is_frac_char))(input).map(|(next_input, res)| {
        let num = match v_frac_to_num(&res) {
            Ok(x) => x,
            _ => 0.0,
        };

        (next_input, num)
    })
}

fn n_fraction(input: &str) -> Res<&str, f32> {
    context("n_fraction", tuple((float, tag("/"), float)))(input)
        .map(|(next_input, res)| (next_input, res.0 / res.2))
}
fn text_number(input: &str) -> Res<&str, f32> {
    context("text_number", tag("one"))(input).map(|(next_input, _)| (next_input, 1.0))
}

/// handles vulgar fraction, or just a number
fn num(input: &str) -> Res<&str, f32> {
    context("num", alt((fraction_number, text_number, float)))(input)
}
/// parses `1 ⅛` or `1 1/8` into `1.125`
fn fraction_number(input: &str) -> Res<&str, f32> {
    context(
        "fraction_number",
        alt((
            tuple((
                opt(tuple((float, space0))), // optional number (and if number, optional space) before
                v_fraction,                  // vulgar frac
            )),
            tuple((
                opt(tuple((float, space1))), // optional number (and if number, required space space) before
                n_fraction,                  // regular frac
            )),
        )),
    )(input)
    .map(|(next_input, res)| {
        let (num, frac) = res;
        let num1 = match num {
            Some(x) => x.0,
            None => 0.0,
        };
        (next_input, num1 + frac)
    })
}

#[cfg(test)]
mod tests {
    use nom::error::{ErrorKind, VerboseErrorKind};

    use super::*;
    #[test]
    fn test_fraction() {
        assert_eq!(fraction_number("1 ⅛"), Ok(("", 1.125)));
        assert_eq!(fraction_number("1 1/8"), Ok(("", 1.125)));
        assert_eq!(fraction_number("1⅓"), Ok(("", 1.3333334)));
        assert_eq!(fraction_number("¼"), Ok(("", 0.25)));
        assert_eq!(fraction_number("1/4"), Ok(("", 0.25)));
        assert_eq!(fraction_number("⅐"), Ok(("", 0.0))); // unkown are dropped
        assert_eq!(
            fraction_number("1"),
            Err(NomErr::Error(VerboseError {
                errors: vec![
                    ("", VerboseErrorKind::Nom(ErrorKind::Tag)),
                    ("1", VerboseErrorKind::Context("n_fraction")),
                    ("1", VerboseErrorKind::Nom(ErrorKind::Alt)),
                    ("1", VerboseErrorKind::Context("fraction_number")),
                ]
            }))
        );
    }
    #[test]
    fn test_v_fraction() {
        assert_eq!(v_frac_to_num(&'⅛'), Ok(0.125));
        assert_eq!(v_frac_to_num(&'¼'), Ok(0.25));
        assert_eq!(v_frac_to_num(&'½'), Ok(0.5));
    }

    #[test]
    fn test_ingredient_parse() {
        assert_eq!(
            ingredient("12 cups flour", false),
            Ok(Ingredient {
                name: "flour".to_string(),
                amounts: vec![Amount {
                    unit: "cups".to_string(),
                    value: 12.0
                }],
                modifier: None,
            })
        );
        assert_eq!(
            parse_ingredient("egg"),
            Ok((
                "",
                Ingredient {
                    name: "egg".to_string(),
                    amounts: vec![],
                    modifier: None,
                }
            ))
        );
        assert_eq!(
            parse_ingredient("1 egg"),
            Ok((
                "",
                Ingredient {
                    name: "egg".to_string(),
                    amounts: vec![Amount {
                        unit: "whole".to_string(),
                        value: 1.0
                    }],
                    modifier: None,
                }
            ))
        );
        assert_eq!(
            format!("res: {}", ingredient("12 cups flour", false).unwrap()),
            "res: 12 cups flour"
        );
        assert_eq!(
            ingredient("one whole egg", true).unwrap().to_string(),
            "1 whole egg"
        );
        assert_eq!(
            ingredient("1 cup (125.5 grams) AP flour, sifted", false)
                .unwrap()
                .to_string(),
            "1 cup / 125.5 grams AP flour, sifted"
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

        assert_eq!(
            parse_ingredient(
                "0.25 ounces (1 packet, about 2 teaspoons) instant or rapid rise yeast"
            ),
            Ok((
                "",
                Ingredient {
                    name: "instant or rapid rise yeast".to_string(),
                    amounts: vec![
                        Amount {
                            unit: "ounces".to_string(),
                            value: 0.25
                        },
                        Amount {
                            unit: "packet".to_string(),
                            value: 1.0
                        },
                        Amount {
                            unit: "teaspoons".to_string(),
                            value: 2.0
                        }
                    ],
                    modifier: None
                }
            ))
        );
        assert_eq!(
            parse_ingredient("6 ounces unsalted butter (1½ sticks; 168.75g)"),
            Ok((
                "",
                Ingredient {
                    name: "unsalted butter".to_string(),
                    amounts: vec![
                        Amount {
                            unit: "ounces".to_string(),
                            value: 6.0
                        },
                        Amount {
                            unit: "sticks".to_string(),
                            value: 1.5
                        },
                        Amount {
                            unit: "g".to_string(),
                            value: 168.75
                        }
                    ],
                    modifier: None
                }
            ))
        );
    }
}
