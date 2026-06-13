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

use crate::parser::vocab::DISTANCE_UNIT_BASES;

/// Check if a string is a distance unit (used for dimension detection).
/// Handles both singular and plural forms automatically. The bases are all
/// lowercase ASCII, so `eq_ignore_ascii_case` matches without allocating.
pub(crate) fn is_distance_unit(s: &str) -> bool {
    let hit = |candidate: &str| {
        DISTANCE_UNIT_BASES
            .iter()
            .any(|base| candidate.eq_ignore_ascii_case(base))
    };
    let bytes = s.as_bytes();
    let ends_with_ci = |suffix: &[u8]| {
        bytes
            .len()
            .checked_sub(suffix.len())
            .is_some_and(|i| bytes[i..].eq_ignore_ascii_case(suffix))
    };

    // Exact match, then the singular recovered by stripping a 1- or 2-char plural.
    // Suffix bytes are ASCII letters, so the byte offsets are valid char boundaries.
    hit(s)
        || (ends_with_ci(b"s") && hit(&s[..s.len() - 1]))
        || (ends_with_ci(b"es") && hit(&s[..s.len() - 2]))
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

    #[rstest]
    #[case::period(".")]
    #[case::of(" of")]
    #[case::something("something")]
    fn test_optional_period_or_of(#[case] input: &str) {
        let result = optional_period_or_of(input);
        assert!(result.is_ok());
    }

    #[rstest]
    #[case::inch("inch", true)]
    #[case::inches("inches", true)]
    #[case::cm("cm", true)]
    #[case::mm("mm", true)]
    #[case::foot("foot", true)]
    #[case::ft("ft", true)]
    #[case::meter("meter", true)]
    #[case::meters("meters", true)]
    #[case::cup("cup", false)]
    #[case::tsp("tsp", false)]
    // Case-insensitive + plural-strip edge cases (the alloc-free rewrite's paths).
    #[case::uppercase("INCH", true)] // eq_ignore_ascii_case path
    #[case::mixed_case_plural("Inches", true)] // case-insensitive "es" strip
    #[case::irregular_feet("feet", false)] // irregular plural, not handled
    #[case::empty("", false)]
    #[case::lone_s("s", false)] // strips to "", which is not a base
    fn test_is_distance_unit(#[case] input: &str, #[case] expected: bool) {
        assert_eq!(is_distance_unit(input), expected, "Failed for: {input}");
    }
}
