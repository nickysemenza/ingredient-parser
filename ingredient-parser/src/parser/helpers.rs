//! Helper functions for parsing ingredients
//! 
//! This module contains low-level parsing helpers used by the main ingredient parser.

use nom::{
    branch::alt,
    bytes::complete::tag,
    character::complete::{alpha1, satisfy},
    error::context,
    multi::many0,
    IResult, Parser,
};
use nom_language::error::VerboseError;

pub(crate) type Res<T, U> = IResult<T, U, VerboseError<T>>;

/// Parse text that can contain various characters common in ingredient names
pub(crate) fn text(input: &str) -> Res<&str, String> {
    satisfy(|c| match c {
        '-' | '—' | '\'' | '\u{2019}' | '.' | '\\' => true,
        c => c.is_alphanumeric() || c.is_whitespace(),
    })
    .parse(input)
    .map(|(next_input, res)| (next_input, res.to_string()))
}

/// Parse unit/amount text including degrees and quotes
pub(crate) fn unitamt(input: &str) -> Res<&str, String> {
    many0(alt((alpha1, tag("°"), tag("\""))))
        .parse(input)
        .map(|(next_input, res)| (next_input, res.join("")))
}

/// Parse text numbers like "one" or "a"
pub(crate) fn text_number(input: &str) -> Res<&str, f64> {
    context("text_number", alt((tag("one"), tag("a "))))
        .parse(input)
        .map(|(next_input, _)| (next_input, 1.0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text() {
        // text() parses a single character, so it returns one char and the remaining input
        assert_eq!(text("a"), Ok(("", "a".to_string())));
        assert_eq!(text("flour"), Ok(("lour", "f".to_string())));
        assert_eq!(text("-"), Ok(("", "-".to_string())));
        assert_eq!(text("—"), Ok(("", "—".to_string())));
        assert_eq!(text("'"), Ok(("", "'".to_string())));
        assert_eq!(text("\u{2019}"), Ok(("", "\u{2019}".to_string())));
        assert_eq!(text("."), Ok(("", ".".to_string())));
        assert_eq!(text("\\"), Ok(("", "\\".to_string())));
        assert_eq!(text(" "), Ok(("", " ".to_string())));
    }

    #[test]
    fn test_unitamt() {
        assert_eq!(unitamt("cups"), Ok(("", "cups".to_string())));
        assert_eq!(unitamt("°F"), Ok(("", "°F".to_string())));
        assert_eq!(unitamt("\""), Ok(("", "\"".to_string())));
        assert_eq!(unitamt("oz"), Ok(("", "oz".to_string())));
        assert_eq!(unitamt(""), Ok(("", "".to_string())));
    }

    #[test]
    fn test_text_number() {
        assert_eq!(text_number("one"), Ok(("", 1.0)));
        assert_eq!(text_number("a "), Ok(("", 1.0)));
        assert!(text_number("two").is_err());
        assert!(text_number("1").is_err());
    }
}