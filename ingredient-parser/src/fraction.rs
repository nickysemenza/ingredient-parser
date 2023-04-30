use nom::{
    branch::alt,
    bytes::complete::tag,
    character::complete::{satisfy, space0, space1},
    combinator::opt,
    error::context,
    number::complete::double,
    sequence::tuple,
};

use crate::Res;

fn v_frac_to_num(input: char) -> Result<f64, String> {
    // two ranges for unicode fractions
    // https://www.compart.com/en/unicode/search?q=vulgar+fraction#characters
    let (n, d): (i32, i32) = match input {
        '¾' => (3, 4),
        '⅛' => (1, 8),
        '¼' => (1, 4),
        '⅓' => (1, 3),
        '½' => (1, 2),
        '⅔' => (2, 3),
        _ => return Err(format!("unkown fraction: {input}")),
    };
    Ok(n as f64 / d as f64)
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

/// parses `1 ⅛` or `1 1/8` into `1.125`
pub fn fraction_number(input: &str) -> Res<&str, f64> {
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

    use nom::error::{ErrorKind, VerboseError, VerboseErrorKind};
    use nom::Err as NomErr;

    use crate::fraction::{fraction_number, v_frac_to_num};

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
}
