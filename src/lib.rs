use std::{convert::TryFrom, fmt};

use nom::{
    branch::alt,
    bytes::complete::tag,
    character::complete::{alpha1, char, not_line_ending, satisfy, space0, space1},
    combinator::{opt, verify},
    error::{context, VerboseError},
    multi::{many1, separated_list1},
    number::complete::double,
    sequence::{delimited, tuple},
    IResult,
};

use crate::util::num_without_zeroes;

extern crate nom;

#[cfg(feature = "serde-derive")]
#[macro_use]
extern crate serde;

pub mod rich_text;
pub mod unit;
pub mod util;

type Res<T, U> = IResult<T, U, VerboseError<T>>;

#[cfg_attr(feature = "serde-derive", derive(Serialize, Deserialize))]
#[derive(Clone, PartialEq, PartialOrd, Debug, Default)]
/// Holds a unit and value pair for an ingredient.
pub struct Amount {
    pub unit: String,
    pub value: f64,
    pub upper_value: Option<f64>,
}

impl fmt::Display for Amount {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", num_without_zeroes(self.value)).unwrap();
        if let Some(u) = self.upper_value {
            if u != 0.0 {
                write!(f, " - {}", num_without_zeroes(u)).unwrap();
            }
        }
        write!(f, " {}", self.unit)
    }
}

impl Amount {
    pub fn new(unit: &str, value: f64) -> Amount {
        Amount {
            unit: unit.to_string(),
            value,
            upper_value: None,
        }
    }
    pub fn new_with_upper(unit: &str, value: f64, upper: f64) -> Amount {
        Amount {
            unit: unit.to_string(),
            value,
            upper_value: Some(upper),
        }
    }
}

#[cfg_attr(feature = "serde-derive", derive(Serialize, Deserialize))]
#[derive(Clone, PartialEq, PartialOrd, Debug, Default)]
/// Holds a name, list of [Amount], and optional modifier string
pub struct Ingredient {
    pub name: String,
    pub amounts: Vec<Amount>,
    pub modifier: Option<String>,
}

impl TryFrom<&str> for Ingredient {
    type Error = String;
    fn try_from(value: &str) -> Result<Ingredient, Self::Error> {
        Ok(from_str(value))
    }
}

impl fmt::Display for Ingredient {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let amounts: Vec<String> = self.amounts.iter().map(|id| id.to_string()).collect();
        let modifier = match self.modifier.clone() {
            Some(m) => {
                format!(", {}", m)
            }
            None => "".to_string(),
        };
        let amount_list = match amounts.len() {
            0 => "n/a ".to_string(),
            _ => format!("{} ", amounts.join(" / ")),
        };
        let name = self.name.clone();
        return write!(f, "{}{}{}", amount_list, name, modifier);
    }
}
/// use [IngredientParser] to customize
pub fn from_str(input: &str) -> Ingredient {
    (IngredientParser::new()).from_str(input)
}

#[derive(Clone, PartialEq, PartialOrd, Debug, Default)]
pub struct IngredientParser {
    pub units: Vec<String>,
}
impl IngredientParser {
    pub fn new() -> Self {
        let units: Vec<String> = vec![
            // non standard units - these aren't really convertible for the most part.
            // default set
            "whole", "packet", "sticks", "stick", "cloves", "clove", "bunch", "head", "large",
            "medium", "package", "recipe", "slice", "standard", "can", "leaf", "leaves",
        ]
        .iter()
        .map(|&s| s.into())
        .collect();
        IngredientParser { units }
    }
    /// wrapper for [self.parse_ingredient]
    /// ```
    /// use ingredient::{from_str};
    /// assert_eq!(from_str("one whole egg").to_string(),"1 whole egg");
    /// ```
    pub fn from_str(self, input: &str) -> Ingredient {
        //todo: add back error handling? can't get this to ever fail since parser is pretty flexible
        self.parse_ingredient(input).unwrap().1
    }

    /// Parses one or two amounts, e.g. `12 grams` or `120 grams / 1 cup`. Used by [self.parse_ingredient].
    /// ```
    /// use ingredient::{IngredientParser,Amount};
    /// let ip = IngredientParser::new();
    /// assert_eq!(
    ///    ip.parse_amount("120 grams"),
    ///    vec![Amount::new("grams",120.0)]
    ///  );
    /// assert_eq!(
    ///    ip.parse_amount("120 grams / 1 cup"),
    ///    vec![Amount::new("grams",120.0),Amount::new("cup", 1.0)]
    ///  );
    /// assert_eq!(
    ///    ip.parse_amount("120 grams / 1 cup / 1 whole"),
    ///    vec![Amount::new("grams",120.0),Amount::new("cup", 1.0),Amount::new("whole", 1.0)]
    ///  );
    /// ```
    pub fn parse_amount(&self, input: &str) -> Vec<Amount> {
        // todo: also can't get this one to fail either
        self.clone().many_amount(input).unwrap().1
    }

    /// Parse an ingredient line item, such as `120 grams / 1 cup whole wheat flour, sifted lightly`.
    ///
    /// returns an [Ingredient], Can be used as a wrapper to return verbose errors.
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
    /// use ingredient::{IngredientParser, Ingredient, Amount};
    /// let ip = IngredientParser::new();
    /// assert_eq!(
    ///     ip.parse_ingredient("1¼  cups / 155.5 grams flour"),
    ///     Ok((
    ///         "",
    ///         Ingredient {
    ///             name: "flour".to_string(),
    ///             amounts: vec![
    ///                 Amount::new("cups", 1.25),
    ///                 Amount::new("grams", 155.5),
    ///             ],
    ///             modifier: None,
    ///         }
    ///     ))
    /// );
    /// ```
    pub fn parse_ingredient(self, input: &str) -> Res<&str, Ingredient> {
        context(
            "ing",
            tuple((
                opt(|a| self.clone().many_amount(a)),
                space0,                              // space between amount(s) and name
                opt(many1(text)),                    // name, can be multiple words
                opt(|a| self.clone().amt_parens(a)), // can have some more amounts in parens after the name
                opt(tag(", ")),                      // comma seperates the modifier
                not_line_ending, // modifier, can be multiple words and even include numbers, since once we've hit the comma everything is fair game.
            )),
        )(input)
        .map(|(next_input, res)| {
            let (amounts, _, name_chunks, amounts2, _, modifier_chunks) = res;
            let m = modifier_chunks;

            let name = name_chunks
                .unwrap_or(vec![])
                .join("")
                .trim_matches(' ')
                .to_string();

            let mut amounts = match amounts {
                Some(a) => a,
                None => vec![],
            };
            amounts = match amounts2 {
                Some(a) => amounts.into_iter().chain(a.into_iter()).collect(),
                None => amounts,
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

    fn num_or_range(self, input: &str) -> Res<&str, (f64, Option<f64>)> {
        context(
            "num_or_range",
            tuple((
                num,
                opt(tuple(
                    (
                        space0,
                        alt((tag("-"), tag("–"), tag("to"), tag("through"))), // second dash is an unusual variant
                        space0,
                        num,
                    ), // care about u.3
                )),
            )),
        )(input)
        .map(|(next_input, res)| {
            let (val, upper_val) = res;
            let upper = match upper_val {
                Some(u) => Some(u.3),
                None => None,
            };
            (next_input, (val, upper))
        })
    }

    fn unit(self, input: &str) -> Res<&str, String> {
        context(
            "unit",
            verify(unitamt, |s: &str| unit::is_valid(self.units.clone(), s)),
        )(input)
    }
    // parses a single amount
    fn amount1(self, input: &str) -> Res<&str, Vec<Amount>> {
        let res = context(
            "amount1",
            tuple(
                (
                    opt(tag("about ")),               // todo: add flag for estimates
                    |a| self.clone().num_or_range(a), // value
                    space0,
                    opt(|a| self.clone().unit(a)), // unit
                    opt(alt((tag("."), tag(" of")))),
                ), // 1 gram
            ),
        )(input)
        .map(|(next_input, res)| {
            let (_prefix, value, _space, unit, _period) = res;
            (
                next_input,
                vec![Amount {
                    unit: unit
                        .unwrap_or("whole".to_string())
                        .to_string()
                        .to_lowercase(),
                    value: value.0,
                    upper_value: value.1,
                }],
            )
        });
        res
    }
    // parses 1-n amounts, e.g. `12 grams` or `120 grams / 1 cup`
    fn many_amount(self, input: &str) -> Res<&str, Vec<Amount>> {
        context(
            "amount_multi",
            separated_list1(
                alt((tag("; "), tag(" / "), tag(" "), tag(", "), tag("/"))),
                alt((|a| self.clone().amt_parens(a), |a| self.clone().amount1(a))),
            ),
        )(input)
        .map(|(next_input, res)| {
            // let (a, b) = res;
            (next_input, res.into_iter().flatten().collect())
        })
    }

    fn amt_parens(self, input: &str) -> Res<&str, Vec<Amount>> {
        context(
            "amt_parens",
            delimited(char('('), |a| self.clone().many_amount(a), char(')')),
        )(input)
    }
}
fn text(input: &str) -> Res<&str, &str> {
    alt((
        alpha1,
        space1,
        tag("-"),
        tag("—"),
        tag("-"),
        tag("'"),
        tag("’"),
        tag("."),
    ))(input)
}
fn unitamt(input: &str) -> Res<&str, String> {
    nom::multi::many0(alt((alpha1, tag("°"))))(input)
        .map(|(next_input, res)| (next_input, res.join("")))
}

fn v_frac_to_num(input: char) -> Result<f64, String> {
    // two ranges for unicode fractions
    // https://www.compart.com/en/unicode/search?q=vulgar+fraction#characters
    let (n, d): (i32, i32) = match input {
        '¾' => (3, 4),
        '⅛' => (1, 8),
        '¼' => (1, 4),
        '⅓' => (1, 3),
        '½' => (1, 2),
        _ => return Err(format!("unkown fraction: {}", input)),
    };
    return Ok(n as f64 / d as f64);
}

fn is_frac_char(c: char) -> bool {
    v_frac_to_num(c).is_ok()
}
/// parses unicode vulgar fractions
fn v_fraction(input: &str) -> Res<&str, f64> {
    context("v_fraction", satisfy(is_frac_char))(input)
        .map(|(next_input, res)| (next_input, v_frac_to_num(res).unwrap()))
}

fn n_fraction(input: &str) -> Res<&str, f64> {
    context("n_fraction", tuple((double, tag("/"), double)))(input)
        .map(|(next_input, res)| (next_input, res.0 / res.2))
}
fn text_number(input: &str) -> Res<&str, f64> {
    context("text_number", alt((tag("one"), tag("a "))))(input)
        .map(|(next_input, _)| (next_input, 1.0))
}

/// handles vulgar fraction, or just a number
fn num(input: &str) -> Res<&str, f64> {
    context("num", alt((fraction_number, text_number, double)))(input)
}
/// parses `1 ⅛` or `1 1/8` into `1.125`
fn fraction_number(input: &str) -> Res<&str, f64> {
    context(
        "fraction_number",
        alt((
            tuple((
                opt(tuple((double, space0))), // optional number (and if number, optional space) before
                v_fraction,                   // vulgar frac
            )),
            tuple((
                opt(tuple((double, space1))), // optional number (and if number, required space space) before
                n_fraction,                   // regular frac
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
    use nom::Err as NomErr;

    use super::*;
    #[test]
    fn test_fraction() {
        assert_eq!(fraction_number("1 ⅛"), Ok(("", 1.125)));
        assert_eq!(fraction_number("1 1/8"), Ok(("", 1.125)));
        assert_eq!(fraction_number("1⅓"), Ok(("", 1.3333333333333333)));
        assert_eq!(fraction_number("¼"), Ok(("", 0.25)));
        assert_eq!(fraction_number("1/4"), Ok(("", 0.25)));
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
        assert_eq!(v_frac_to_num('⅛'), Ok(0.125));
        assert_eq!(v_frac_to_num('¼'), Ok(0.25));
        assert_eq!(v_frac_to_num('½'), Ok(0.5));
    }
    #[test]
    fn test_amount() {
        assert_eq!(
            (IngredientParser::new()).parse_amount("350 °"),
            vec![Amount::new("°", 350.0)]
        );
        assert_eq!(
            (IngredientParser::new()).parse_amount("350 °F"),
            vec![Amount::new("°f", 350.0)]
        );
    }

    #[test]
    fn test_amount_range() {
        assert_eq!(
            (IngredientParser::new()).parse_amount("2¼-2.5 cups"),
            vec![Amount::new_with_upper("cups", 2.25, 2.5)]
        );
        assert_eq!(
            (IngredientParser::new()).parse_amount("2¼-2.5 cups"),
            (IngredientParser::new()).parse_amount("2 ¼ - 2.5 cups")
        );
        assert_eq!(
            Ingredient::try_from("1-2 cups flour"),
            Ok(Ingredient {
                name: "flour".to_string(),
                amounts: vec![Amount::new_with_upper("cups", 1.0, 2.0)],
                modifier: None,
            })
        );
        assert_eq!(
            format!(
                "{}",
                (IngredientParser::new())
                    .parse_amount("2 ¼ - 2.5 cups")
                    .first()
                    .unwrap()
            ),
            "2.25 - 2.5 cups"
        );
    }
    #[test]
    fn test_ingredient_parse() {
        assert_eq!(
            Ingredient::try_from("12 cups flour"),
            Ok(Ingredient {
                name: "flour".to_string(),
                amounts: vec![Amount::new("cups", 12.0)],
                modifier: None,
            })
        );
    }

    #[test]
    fn test_ingredient_parse_no_amounts() {
        assert_eq!(
            (IngredientParser::new()).parse_ingredient("egg"),
            Ok((
                "",
                Ingredient {
                    name: "egg".to_string(),
                    amounts: vec![],
                    modifier: None,
                }
            ))
        );
    }
    #[test]
    fn test_ingredient_parse_no_unit() {
        assert_eq!(
            (IngredientParser::new()).parse_ingredient("1 egg"),
            Ok((
                "",
                Ingredient {
                    name: "egg".to_string(),
                    amounts: vec![Amount::new("whole", 1.0)],
                    modifier: None,
                }
            ))
        );
    }
    #[test]
    fn test_ingredient_parse_no_unit_multi_name() {
        assert_eq!(
            (IngredientParser::new()).parse_ingredient("1 cinnamon stick"),
            Ok((
                "",
                Ingredient {
                    name: "cinnamon stick".to_string(),
                    amounts: vec![Amount::new("whole", 1.0)],
                    modifier: None,
                }
            ))
        );
        assert_eq!(
            (IngredientParser::new()).parse_ingredient("1 cinnamon stick"),
            (IngredientParser::new()).parse_ingredient("1 whole cinnamon stick"),
        );
    }
    #[test]
    fn test_ingredient_parse_no_unit_multi_name_adj() {
        assert_eq!(
            (IngredientParser::new()).parse_ingredient("1 cinnamon stick, crushed"),
            Ok((
                "",
                Ingredient {
                    name: "cinnamon stick".to_string(),
                    amounts: vec![Amount::new("whole", 1.0)],
                    modifier: Some("crushed".to_string()),
                }
            ))
        );
    }
    #[test]
    fn test_stringy() {
        assert_eq!(
            format!("res: {}", from_str("12 cups flour")),
            "res: 12 cups flour"
        );
        assert_eq!(from_str("one whole egg").to_string(), "1 whole egg");
        assert_eq!(from_str("a tsp flour").to_string(), "1 tsp flour");
    }
    #[test]
    fn test_with_parens() {
        assert_eq!(
            from_str("1 cup (125.5 grams) AP flour, sifted").to_string(),
            "1 cup / 125.5 grams AP flour, sifted"
        );
    }
    #[test]
    fn test_no_ingredient_amounts() {
        assert_eq!(
            Ingredient {
                name: "apples".to_string(),
                amounts: vec![],
                modifier: None,
            }
            .to_string(),
            "n/a apples"
        );
    }
    #[test]
    fn test_ingredient_parse_multi() {
        assert_eq!(
            (IngredientParser::new()).parse_ingredient("12 cups all purpose flour, lightly sifted"),
            Ok((
                "",
                Ingredient {
                    name: "all purpose flour".to_string(),
                    amounts: vec![Amount::new("cups", 12.0)],
                    modifier: Some("lightly sifted".to_string()),
                }
            ))
        );

        assert_eq!(
            (IngredientParser::new()).parse_ingredient("1¼  cups / 155.5 grams flour"),
            Ok((
                "",
                Ingredient {
                    name: "flour".to_string(),
                    amounts: vec![Amount::new("cups", 1.25), Amount::new("grams", 155.5),],
                    modifier: None,
                }
            ))
        );

        assert_eq!(
            (IngredientParser::new()).parse_ingredient(
                "0.25 ounces (1 packet, about 2 teaspoons) instant or rapid rise yeast"
            ),
            Ok((
                "",
                Ingredient {
                    name: "instant or rapid rise yeast".to_string(),
                    amounts: vec![
                        Amount::new("ounces", 0.25),
                        Amount::new("packet", 1.0),
                        Amount::new("teaspoons", 2.0),
                    ],
                    modifier: None
                }
            ))
        );
        assert_eq!(
            (IngredientParser::new())
                .parse_ingredient("6 ounces unsalted butter (1½ sticks; 168.75g)"),
            Ok((
                "",
                Ingredient {
                    name: "unsalted butter".to_string(),
                    amounts: vec![
                        Amount::new("ounces", 6.0),
                        Amount::new("sticks", 1.5),
                        Amount::new("g", 168.75),
                    ],
                    modifier: None
                }
            ))
        );
        assert_eq!(
            (IngredientParser::new())
                .parse_ingredient("½ pound 2 sticks; 227 g unsalted butter, room temperature"),
            Ok((
                "",
                Ingredient {
                    name: "unsalted butter".to_string(),
                    amounts: vec![
                        Amount::new("pound", 0.5),
                        Amount::new("sticks", 2.0),
                        Amount::new("g", 227.0),
                    ],
                    modifier: Some("room temperature".to_string())
                }
            ))
        );
        assert_eq!(
            (IngredientParser::new()).parse_ingredient("1 ½ cups/192 grams all-purpose flour"),
            (IngredientParser::new()).parse_ingredient("1 1/2 cups / 192 grams all-purpose flour")
        );
    }
    #[test]
    fn test_weird_chars() {
        assert_eq!(
            (IngredientParser::new())
                .parse_ingredient("2 cups/240 grams confectioners’ sugar, sifted"),
            Ok((
                "",
                Ingredient {
                    name: "confectioners’ sugar".to_string(),
                    amounts: vec![Amount::new("cups", 2.0), Amount::new("grams", 240.0)],
                    modifier: Some("sifted".to_string())
                }
            ))
        );
        assert_eq!(
            (IngredientParser::new()).parse_ingredient("2 cups/240 grams confectioners' sugar"),
            Ok((
                "",
                Ingredient {
                    name: "confectioners' sugar".to_string(),
                    amounts: vec![Amount::new("cups", 2.0), Amount::new("grams", 240.0)],
                    modifier: None
                }
            ))
        );
    }
    #[test]
    fn test_unit_period_mixed_case() {
        assert_eq!(
            (IngredientParser::new()).parse_ingredient("1 Tbsp. flour"),
            (IngredientParser::new()).parse_ingredient("1 tbsp flour"),
        );
        assert_eq!(
            (IngredientParser::new()).parse_ingredient("12 cloves of garlic, peeled"),
            Ok((
                "",
                Ingredient {
                    name: "garlic".to_string(),
                    amounts: vec![Amount::new("cloves", 12.0),],
                    modifier: Some("peeled".to_string())
                }
            ))
        );
    }
    #[test]
    fn test_parse_ingredient_cloves() {
        assert_eq!(
            (IngredientParser::new()).parse_ingredient("1 clove garlic, grated"),
            Ok((
                "",
                Ingredient {
                    name: "garlic".to_string(),
                    amounts: vec![Amount::new("clove", 1.0),],
                    modifier: Some("grated".to_string())
                }
            ))
        );
        //todo: doesn't work
        // assert_eq!(
        //     (IngredientParser::new()).parse_ingredient("1 clove, grated"),
        //     Ok((
        //         "",
        //         Ingredient {
        //             name: "clove".to_string(),
        //             amounts: vec![Amount::new("whole", 1.0),],
        //             modifier: Some("grated".to_string())
        //         }
        //     ))
        // );
    }
}
