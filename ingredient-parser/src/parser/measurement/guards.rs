use nom::{branch::alt, bytes::complete::tag, combinator::opt, Parser};

use crate::parser::Res;

/// Consume an optional em-dash or en-dash separator between amount and unit.
/// Some cookbooks use formats like "3–4 — tablespoons" where there's an extra
/// dash between the range and the unit.
pub(super) fn optional_dash_separator(input: &str) -> Res<&str, Option<&str>> {
    opt(alt((tag("— "), tag("– "), tag("—"), tag("–")))).parse(input)
}

/// Parse optional trailing period or " of" after units (e.g., "tsp." or "cup of")
/// Also consumes a trailing space after a period (for sentence breaks like "375. Next").
pub(super) fn optional_period_or_of(input: &str) -> Res<&str, Option<&str>> {
    opt(alt((tag(". "), tag("."), tag(" of")))).parse(input)
}

/// Consume an optional indefinite article ("a "/"an ") sitting between the value
/// and the unit, e.g. the "a" in "half a cup". Case-insensitive. Lets "half a
/// cup of milk" reach the `cup` unit instead of leaving "a cup of milk" as the
/// name. ("a cup" with no leading number is already handled because `parse_value`
/// reads a bare "a"/"an" as 1.)
pub(super) fn optional_article(input: &str) -> Res<&str, Option<&str>> {
    opt(alt((
        nom::bytes::complete::tag_no_case("a "),
        nom::bytes::complete::tag_no_case("an "),
    )))
    .parse(input)
}

/// Given a string whose first `(` opens a (possibly nested) group, return the
/// byte index of the matching `)`. Returns `None` if there is no `(` or the
/// parentheses are unbalanced. Used to skip over a parenthesized aside while
/// respecting nesting (e.g. the size in "1 (1½-inch) piece" or a unit after a
/// description).
pub(super) fn find_matching_paren(input: &str) -> Option<usize> {
    let open = input.find('(')?;
    let mut depth = 0usize;
    for (index, character) in input[open..].char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open + index);
                }
            }
            _ => {}
        }
    }
    None
}

/// Check if a bare number looks like a step number in instructions.
///
/// Returns true if the remaining input starts with whitespace followed by
/// a capitalized word (likely an instruction verb like "Bring", "Set", "Add").
pub(super) fn looks_like_step_number(input: &str) -> bool {
    let trimmed = input.trim_start();
    if trimmed.is_empty() {
        return false;
    }

    let first_char = trimmed.chars().next().unwrap_or(' ');
    if !first_char.is_ascii_uppercase() {
        return false;
    }

    let first_word: String = trimmed.chars().take_while(|c| c.is_alphabetic()).collect();
    first_word.len() >= 2
}

/// Check if text starts with a dimension suffix (e.g., "-inch", "-cm", "-inches").
///
/// A dimension suffix is a hyphen followed by a distance unit.
/// For example, "1-inch" in "1-inch piece ginger" should not be parsed as quantity=1.
pub(super) fn starts_with_dimension_suffix(text: &str) -> bool {
    let text = text.to_lowercase();
    if !text.starts_with('-') {
        return false;
    }

    let after_hyphen = &text[1..];
    let unit_part: String = after_hyphen
        .chars()
        .take_while(|c| c.is_alphabetic())
        .collect();

    if unit_part.is_empty() {
        return false;
    }

    is_distance_unit(&unit_part)
}

use crate::parser::vocab::DISTANCE_UNIT_BASES;

/// Check if a string is a distance unit (used for dimension detection).
/// Handles both singular and plural forms automatically.
pub(crate) fn is_distance_unit(s: &str) -> bool {
    let lower = s.to_lowercase();

    for base in DISTANCE_UNIT_BASES {
        if lower == *base {
            return true;
        }
    }

    if lower.ends_with('s') {
        let without_s = &lower[..lower.len() - 1];
        for base in DISTANCE_UNIT_BASES {
            if without_s == *base {
                return true;
            }
        }

        if lower.ends_with("es") && lower.len() > 2 {
            let without_es = &lower[..lower.len() - 2];
            for base in DISTANCE_UNIT_BASES {
                if without_es == *base {
                    return true;
                }
            }
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::simple("(abc) rest", Some(4))]
    #[case::nested("(a (b) c) rest", Some(8))]
    #[case::leading_text("x (a) b", Some(4))]
    #[case::no_open("no parens", None)]
    #[case::unbalanced("(a (b)", None)]
    fn test_find_matching_paren(#[case] input: &str, #[case] expected: Option<usize>) {
        assert_eq!(find_matching_paren(input), expected, "input: {input}");
    }
}
