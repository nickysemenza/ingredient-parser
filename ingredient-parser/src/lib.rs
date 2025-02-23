use std::collections::HashSet;
use std::iter::FromIterator;

pub use crate::ingredient::Ingredient;
use anyhow::Result;
use fraction::fraction_number;
#[allow(deprecated)]
use nom::{
    branch::alt,
    bytes::complete::tag,
    character::complete::{alpha1, char, not_line_ending, satisfy, space0, space1},
    combinator::{opt, verify},
    error::context,
    multi::{many1, separated_list1},
    number::complete::double,
    sequence::{delimited, tuple},
    IResult, Parser,
};
use nom_language::error::VerboseError;
use tracing::info;
use unit::Measure;

extern crate nom;

#[cfg(feature = "serde-derive")]
#[macro_use]
extern crate serde;

mod fraction;
pub mod ingredient;
pub mod rich_text;
pub mod unit;
pub mod util;
pub type Res<T, U> = IResult<T, U, VerboseError<T>>;

/// use [IngredientParser] to customize
pub fn from_str(input: &str) -> Ingredient {
    (IngredientParser::new(false)).from_str(input)
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct IngredientParser {
    pub units: HashSet<String>,
    pub adjectives: HashSet<String>,
    pub is_rich_text: bool,
}
impl IngredientParser {
    pub fn new(is_rich_text: bool) -> Self {
        let units: Vec<String> = vec![
            // non standard units - these aren't really convertible for the most part.
            // default set
            "whole", "packet", "sticks", "stick", "cloves", "clove", "bunch", "head", "large",
            "pinch", "small", "medium", "package", "recipe", "slice", "standard", "can", "leaf",
            "leaves", "strand", "tin",
        ]
        .iter()
        .map(|&s| s.into())
        .collect();
        let adjectives: Vec<String> = [
            "chopped",
            "minced",
            "diced",
            "freshly ground",
            "freshly grated",
            "finely chopped",
            "thinly sliced",
            "sliced",
            "plain",
            "to taste",
        ]
        .iter()
        .map(|&s| s.into())
        .collect();
        IngredientParser {
            units: HashSet::from_iter(units.iter().cloned()),
            adjectives: HashSet::from_iter(adjectives.iter().cloned()),
            is_rich_text,
        }
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
    /// use ingredient::{IngredientParser,unit::Measure};
    /// let ip = IngredientParser::new(false);
    /// assert_eq!(
    ///    ip.must_parse_amount("120 grams"),
    ///    vec![Measure::parse_new("grams",120.0)]
    ///  );
    /// assert_eq!(
    ///    ip.must_parse_amount("120 grams / 1 cup"),
    ///    vec![Measure::parse_new("grams",120.0),Measure::parse_new("cup", 1.0)]
    ///  );
    /// assert_eq!(
    ///    ip.must_parse_amount("120 grams / 1 cup / 1 whole"),
    ///    vec![Measure::parse_new("grams",120.0),Measure::parse_new("cup", 1.0),Measure::parse_new("whole", 1.0)]
    ///  );
    /// ```
    #[tracing::instrument(name = "parse_amount")]
    pub fn parse_amount(&self, input: &str) -> Result<Vec<Measure>> {
        // todo: also can't get this one to fail either
        match self.clone().many_amount(input) {
            Ok((_, res)) => Ok(res),
            Err(e) => Err(anyhow::anyhow!(
                "parse_amount on '{}' failed: {:?}",
                input,
                e
            )),
        }
    }
    pub fn must_parse_amount(&self, input: &str) -> Vec<Measure> {
        self.parse_amount(input).expect("parse failed")
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
    /// use ingredient::{IngredientParser, ingredient::Ingredient, unit::Measure};
    /// let ip = IngredientParser::new(false);
    /// assert_eq!(
    ///     ip.parse_ingredient("1¼  cups / 155.5 grams flour"),
    ///     Ok((
    ///         "",
    ///         Ingredient {
    ///             name: "flour".to_string(),
    ///             amounts: vec![
    ///                 Measure::parse_new("cups", 1.25),
    ///                 Measure::parse_new("grams", 155.5),
    ///             ],
    ///             modifier: None,
    ///         }
    ///     ))
    /// );
    /// ```
    #[tracing::instrument(name = "parse_ingredient")]
    #[allow(clippy::type_complexity)]
    pub fn parse_ingredient(self, input: &str) -> Res<&str, Ingredient> {
        context(
            "ing",
            (
                opt(|a| self.clone().many_amount(a)),
                space0, // space between amount(s) and name
                opt((|a| self.clone().adjective(a), space1)), // optional modifier
                opt(many1(text)), // name, can be multiple words
                opt(|a| self.clone().amt_parens(a)), // can have some more amounts in parens after the name
                opt(tag(", ")),                      // comma seperates the modifier
                not_line_ending, // modifier, can be multiple words and even include numbers, since once we've hit the comma everything is fair game.
            ),
        )
        .parse(input)
        .map(|(next_input, res)| {
            let (
                amounts,
                _maybespace,
                adjective,
                name_chunks,
                amounts2,
                _maybecomma,
                modifier_chunks,
            ): (
                Option<Vec<Measure>>,
                &str,
                Option<(String, &str)>,
                Option<Vec<String>>,
                Option<Vec<Measure>>,
                Option<&str>,
                &str,
            ) = res;
            let mut modifiers: String = modifier_chunks.to_owned();
            if let Some((adjective, _)) = adjective {
                modifiers.push_str(&adjective);
            }
            let mut name: String = name_chunks
                .unwrap_or(vec![])
                .join("")
                .trim_matches(' ')
                .to_string();

            // if the ingredient name still has adjective in it, remove that
            self.adjectives.iter().for_each(|f| {
                if name.contains(f) {
                    modifiers.push_str(f);
                    name = name.replace(f, "").trim_matches(' ').to_string();
                }
            });

            let mut amounts = amounts.unwrap_or_default();
            amounts = match amounts2 {
                Some(a) => amounts.into_iter().chain(a.into_iter()).collect(),
                None => amounts,
            };

            (
                next_input,
                Ingredient {
                    name,
                    amounts,
                    modifier: match modifiers.chars().count() {
                        0 => None,
                        _ => Some(modifiers.to_string()),
                    },
                },
            )
        })
    }
    fn get_value(self, input: &str) -> Res<&str, (f64, Option<f64>)> {
        context(
            "get_value",
            alt((
                |a| self.clone().upper_range_only(a),
                |a| self.clone().num_or_range(a),
            )),
        )
        .parse(input)
    }

    fn num_or_range(self, input: &str) -> Res<&str, (f64, Option<f64>)> {
        context(
            "num_or_range",
            (
                |a| self.clone().num(a),
                opt(|a| self.clone().range_up_num(a)),
            ),
        )
        .parse(input)
        .map(|(next_input, res)| {
            let (val, upper_val) = res;
            let upper = upper_val;
            (next_input, (val, upper))
        })
    }

    fn upper_range_only(self, input: &str) -> Res<&str, (f64, Option<f64>)> {
        context(
            "upper_range_only",
            (
                opt(space0),
                alt((tag("up to"), tag("at most"))),
                space0,
                |a| self.clone().num(a),
            ),
        )
        .parse(input)
        .map(|(next_input, res)| (next_input, (0.0, Some(res.3))))
    }

    fn unit(self, input: &str) -> Res<&str, String> {
        context(
            "unit",
            verify(unitamt, |s: &str| unit::is_valid(self.units.clone(), s)),
        )
        .parse(input)
    }
    fn unit_extra(self, input: &str) -> Res<&str, String> {
        context(
            "unit",
            verify(unitamt, |s: &str| {
                // fix for test_parse_whole_wheat_ambigious
                let mut x = self.units.clone();
                if input.starts_with("whole wheat") {
                    x.remove("whole");
                }
                unit::is_addon_unit(x, s)
            }),
        )
        .parse(input)
    }
    fn adjective(self, input: &str) -> Res<&str, String> {
        context(
            "adjective",
            verify(unitamt, |s: &str| {
                self.adjectives.contains(&s.to_lowercase())
            }),
        )
        .parse(input)
    }

    // parses a single amount
    #[allow(deprecated)]
    fn amount1(self, input: &str) -> Res<&str, Measure> {
        context(
            "amount1",
            tuple(
                (
                    opt(tag("about ")), // todo: add flag for estimates
                    opt(|a| self.clone().mult_prefix_1(a)),
                    |a| self.clone().get_value(a), // value
                    space0,
                    opt(|a| self.clone().unit(a)), // unit
                    opt(alt((tag("."), tag(" of")))),
                ), // 1 gram
            ),
        )
        .parse(input)
        .map(|(next_input, res)| {
            let (_prefix, mult, value, _space, unit, _period) = res;
            let mut v = value.0;
            if let Some(m) = mult {
                v *= m
            }
            return (
                next_input,
                Measure::from_parts(
                    unit.unwrap_or("whole".to_string()).to_lowercase().as_ref(),
                    v,
                    value.1,
                ),
            );
        })
    }
    fn just_extra_unit(self, input: &str) -> Res<&str, Measure> {
        context(
            "just_extra_unit",
            (
                |a| {
                    if self.is_rich_text {
                        space1(a)
                    } else {
                        space0(a)
                    }
                },
                |a| self.clone().unit_extra(a), // unit
                opt(alt((tag("."), tag(" of")))),
                space1,
            ),
        )
        .parse(input)
        .map(|(next_input, res)| {
            let (_, unit, _, _) = res;
            return (
                next_input,
                Measure::from_parts(unit.to_lowercase().as_ref(), 1.0, None),
            );
        })
    }
    // parses an amount like `78g to 104g cornmeal`
    fn amount_with_units_twice(self, input: &str) -> Res<&str, Option<Measure>> {
        context(
            "amount_with_units_twice",
            (
                opt(tag("about ")),            // todo: add flag for estimates
                |a| self.clone().get_value(a), // value
                space0,
                opt(|a| self.clone().unit(a)), // unit
                |a| self.clone().range_up_num(a),
                opt(|a| self.clone().unit(a)),
                opt(alt((tag("."), tag(" of")))),
            ),
        )
        .parse(input)
        .map(|(next_input, res)| {
            let (_prefix, value, _space, unit, upper_val, upper_unit, _period) = res;
            if upper_unit.is_some() && unit != upper_unit {
                info!("unit mismatch: {:?} vs {:?}", unit, upper_unit);
                // panic!("unit mismatch: {:?} vs {:?}", unit, upper_unit)
                return (next_input, None);
            }
            // let upper = match value.1 {
            //     Some(u) => Some(u),
            //     None => upper_val,
            //      match upper_val {
            //         Some(u) => Some(u),
            //         None => None,
            //     },
            // };
            let upper = Some(upper_val);
            return (
                next_input,
                Some(Measure::from_parts(
                    unit.unwrap_or("whole".to_string()).to_lowercase().as_ref(),
                    value.0,
                    upper,
                )),
            );
        })
    }
    // parses 1-n amounts, e.g. `12 grams` or `120 grams / 1 cup`
    #[tracing::instrument(name = "many_amount")]
    fn many_amount(self, input: &str) -> Res<&str, Vec<Measure>> {
        context(
            "many_amount",
            separated_list1(
                alt((tag("; "), tag(" / "), tag(" "), tag(", "), tag("/"))),
                alt((
                    |a| self.clone().plus_amount(a).map(|(a, b)| (a, vec![b])),
                    |a| {
                        self.clone().amount_with_units_twice(a).map(|(a, b)| {
                            (
                                a,
                                match b {
                                    Some(a) => vec![a],
                                    None => vec![],
                                },
                            )
                        })
                    }, // regular amount
                    |a| self.clone().amt_parens(a), // amoiunt with parens
                    |a| self.clone().amount1(a).map(|(a, b)| (a, vec![b])), // regular amount
                    |a| self.clone().just_extra_unit(a).map(|(a, b)| (a, vec![b])), // regular amount
                )),
            ),
        )
        .parse(input)
        .map(|(next_input, res)| {
            // let (a, b) = res;
            (next_input, res.into_iter().flatten().collect())
        })
    }

    fn amt_parens(self, input: &str) -> Res<&str, Vec<Measure>> {
        context(
            "amt_parens",
            delimited(char('('), |a| self.clone().many_amount(a), char(')')),
        )
        .parse(input)
    }
    /// handles vulgar fraction, or just a number
    fn num(self, input: &str) -> Res<&str, f64> {
        if self.is_rich_text {
            context("num", alt((fraction_number, double))).parse(input)
        } else {
            context("num", alt((fraction_number, text_number, double))).parse(input)
        }
    }
    fn mult_prefix_1(self, input: &str) -> Res<&str, f64> {
        context(
            "mult_prefix_1",
            (|a| self.clone().num(a), space1, tag("x"), space1),
        )
        .parse(input)
        .map(|(next_input, res)| {
            let (num, _, _, _) = res;
            (next_input, num)
        })
    }
    fn range_up_num(self, input: &str) -> Res<&str, f64> {
        context(
            "range_up_num",
            alt((
                (
                    space0,
                    alt((tag("-"), tag("–"))), // second dash is an unusual variant
                    space0,
                    |a| self.clone().num(a),
                ),
                (
                    space1,
                    alt((tag("to"), tag("through"), tag("or"))),
                    space1,
                    |a| self.clone().num(a),
                ),
            )),
        )
        .parse(input)
        .map(|(next_input, (_space1, _, _space2, num))| (next_input, num))
    }
    fn plus_amount(self, input: &str) -> Res<&str, Measure> {
        context(
            "plus_num",
            (
                |a| self.clone().amount1(a),
                space1,
                tag("plus"),
                space1,
                |a| self.clone().amount1(a),
            ),
        )
        .parse(input)
        .map(|(next_input, (a, _space1, _, _, b))| {
            let c = a.add(b).unwrap();
            (next_input, c)
        })
    }
}

fn text(input: &str) -> Res<&str, String> {
    (satisfy(|c| match c {
        '-' | '—' | '\'' | '’' | '.' | '\\' => true,
        c => c.is_alphanumeric() || c.is_whitespace(),
    }))
    .parse(input)
    .map(|(next_input, res)| (next_input, res.to_string()))
}
fn unitamt(input: &str) -> Res<&str, String> {
    nom::multi::many0(alt((alpha1, tag("°"), tag("\""))))
        .parse(input)
        .map(|(next_input, res)| (next_input, res.join("")))
}

fn text_number(input: &str) -> Res<&str, f64> {
    context("text_number", alt((tag("one"), tag("a "))))
        .parse(input)
        .map(|(next_input, _)| (next_input, 1.0))
}

#[cfg(test)]
mod tests {
    use std::convert::TryFrom;

    use super::*;
    #[test]
    fn test_amount() {
        assert_eq!(
            (IngredientParser::new(false)).must_parse_amount("350 °"),
            vec![Measure::parse_new("°", 350.0)]
        );
        assert_eq!(
            (IngredientParser::new(false)).must_parse_amount("350 °F"),
            vec![Measure::parse_new("°f", 350.0)]
        );
    }

    #[test]
    fn test_amount_range() {
        assert_eq!(
            (IngredientParser::new(false)).must_parse_amount("2¼-2.5 cups"),
            vec![Measure::parse_new_with_upper("cups", 2.25, 2.5)]
        );

        assert_eq!(
            Ingredient::try_from("1-2 cups flour"),
            Ok(Ingredient {
                name: "flour".to_string(),
                amounts: vec![Measure::parse_new_with_upper("cups", 1.0, 2.0)],
                modifier: None,
            })
        );
        assert_eq!(
            format!(
                "{}",
                (IngredientParser::new(false))
                    .must_parse_amount("2 ¼ - 2.5 cups")
                    .first()
                    .unwrap()
            ),
            "2.25 - 2.5 cups"
        );
        assert_eq!(
            (IngredientParser::new(false)).must_parse_amount("2 to 4 days"),
            vec![Measure::parse_new_with_upper("days", 2.0, 4.0)]
        );

        // #30
        assert_eq!(
            (IngredientParser::new(false)).must_parse_amount("up to 4 days"),
            vec![Measure::parse_new_with_upper("days", 0.0, 4.0)]
        );
    }
    #[test]
    fn test_ingredient_parse() {
        assert_eq!(
            Ingredient::try_from("12 cups flour"),
            Ok(Ingredient {
                name: "flour".to_string(),
                amounts: vec![Measure::parse_new("cups", 12.0)],
                modifier: None,
            })
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
            "1 cup / 125.5 g AP flour, sifted"
        );
    }
}
