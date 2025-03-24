use nom::{
    branch::alt,
    bytes::complete::tag,
    character::complete::{satisfy, space0, space1},
    combinator::opt,
    error::context,
    number::complete::double,
    Parser,
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
        // Adding more common unicode fractions
        '⅕' => (1, 5),
        '⅖' => (2, 5),
        '⅗' => (3, 5),
        '⅘' => (4, 5),
        '⅙' => (1, 6),
        '⅚' => (5, 6),
        '⅐' => (1, 7),
        '⅑' => (1, 9),
        '⅒' => (1, 10),
        _ => return Err(format!("unknown fraction: {input}")),
    };
    Ok(n as f64 / d as f64)
}

fn is_frac_char(c: char) -> bool {
    v_frac_to_num(c).is_ok()
}

/// parses unicode vulgar fractions
fn v_fraction(input: &str) -> Res<&str, f64> {
    context("v_fraction", satisfy(is_frac_char))
        .parse(input)
        .map(|(next_input, res)| (next_input, v_frac_to_num(res).unwrap()))
}
fn n_fraction(input: &str) -> Res<&str, f64> {
    context("n_fraction", (double, tag("/"), double))
        .parse(input)
        .map(|(next_input, res)| (next_input, res.0 / res.2))
}

/// Parses mixed number formats like `1 ⅛` or `1 1/8` into `1.125`
/// 
/// This parser handles both unicode vulgar fractions and standard slash-notation fractions,
/// either alone or with a whole number component.
pub fn fraction_number(input: &str) -> Res<&str, f64> {
    // Define parser for unicode vulgar fractions with optional whole number
    let vulgar_fraction_parser = (
        opt((double, space0)),  // Optional whole number with optional whitespace
        v_fraction,             // Unicode vulgar fraction like ½, ¼, etc.
    );
    
    // Define parser for slash-notation fractions with optional whole number
    let slash_fraction_parser = (
        opt((double, space1)),  // Optional whole number with required whitespace
        n_fraction,             // Standard fraction notation like 1/4, 3/8, etc.
    );
    
    // Try both parsers
    context(
        "fraction_number",
        alt((vulgar_fraction_parser, slash_fraction_parser)),
    )
    .parse(input)
    .map(|(next_input, res)| {
        let (whole_number, fractional_part) = res;
        
        // Extract whole number or default to 0.0
        let whole_value = whole_number.map_or(0.0, |(num, _)| num);
        
        // Sum whole and fractional parts
        (next_input, whole_value + fractional_part)
    })
}

#[cfg(test)]
mod tests {

    use nom::error::ErrorKind;
    use nom::Err as NomErr;
    use nom_language::error::{VerboseError, VerboseErrorKind};

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
