//! Classify parenthetical content for structural resolution.
//!
//! The segment resolver decides ownership using this classification and the
//! surrounding ingredient: references are discarded, optional markers set a
//! flag, measurements become amounts, and descriptive asides retain their source.
//! Minus-equivalence asides are discarded only when a primary amount exists.
//! Classification with no unit vocabulary skips measurement parsing, allowing
//! the resolver to retain the parsed payload alongside the classification.

use std::collections::HashSet;

use crate::parser::token::matching_close_paren;
use crate::parser::{MeasurementMode, MeasurementParser};
use crate::unit::Measure;

/// The kind of a single top-level parenthetical, judged from its inner text.
///
/// Ordered most-specific to least in [`classify`]; the first matching kind wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ParenKind {
    /// "(see this page)", "(page 12)", or a chain of page refs — pure navigation
    /// cruft. Classified by [`CROSS_REF`]. Mixed content ("(from Lamb Meat Soup,
    /// this page)") is NOT this — the regex requires the *entire* inner to be
    /// page refs and their connectors.
    CrossReference,
    /// "(see note)", "(notes)" — a pointer to the recipe headnote. Classified by
    /// [`NOTE_REF`]. "(note the color)" is NOT this — real content follows.
    NoteReference,
    /// "(2 sticks minus 1 tablespoon)" — an arithmetic-equivalence aside.
    /// Classified by [`MINUS_PAREN`]. (The site adds a whole-line guard; see the
    /// module docs.)
    MinusEquivalence,
    /// "(optional)" — an optionality marker.
    Optional,
    /// "(70° to 80°F)", "(¼ inch / 6 mm)" — a temperature/distance descriptor.
    Descriptive,
    /// "(about 2 cups)", "(120g)" — a measurement that can hoist as a secondary
    /// amount. Requires `units` to be `Some`; parses the inner as a measurement
    /// list. `None` units disables this check (yields a later kind).
    Amount,
    /// "(red)" in "purple (red) cabbage" — a bare alias with no digits or vulgar
    /// fractions. Position determines whether it qualifies the name or modifier.
    Alias,
    /// None of the above.
    Other,
}

// Parenthetical syntax shared by the classifiers below.

/// Matches a cross-reference parenthetical whose content is entirely page
/// references and their connectors. See [`ParenKind::CrossReference`].
pub(crate) static CROSS_REF: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    #[allow(clippy::expect_used)]
    regex::Regex::new(
        r"(?i)\s*\(\s*(?:(?:see\s+)?(?:this page|page\s+\d+)|see\s+(?:here|above|below))(?:[\s,;]*(?:to|or|and)?[\s,;]*(?:(?:see\s+)?(?:this page|page\s+\d+)|see\s+(?:here|above|below)))*\s*\)",
    )
    .expect("invalid cross-ref regex")
});

/// Matches a mixed cross-reference + "optional" parenthetical, reduced to
/// "(optional)" by `split_crossref_optional`.
pub(crate) static CROSS_REF_OPTIONAL: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(
    || {
        #[allow(clippy::expect_used)]
        regex::Regex::new(
            r"(?i)\(\s*(?:(?:see\s+)?(?:this page|page\s+\d+)|see\s+(?:here|above|below))(?:[\s,;]*(?:to|or|and)?[\s,;]*(?:(?:see\s+)?(?:this page|page\s+\d+)|see\s+(?:here|above|below)))*[\s,;]+optional\s*\)",
        )
        .expect("invalid cross-ref-optional regex")
    },
);

/// Matches a "(see note)" / "(notes)" reference. See [`ParenKind::NoteReference`].
pub(crate) static NOTE_REF: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    #[allow(clippy::expect_used)]
    regex::Regex::new(r"(?i)\s*\(\s*(?:see\s+)?notes?\s*\)").expect("invalid note-ref regex")
});

/// Matches an arithmetic-equivalence parenthetical containing "minus". See
/// [`ParenKind::MinusEquivalence`].
pub(crate) static MINUS_PAREN: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    #[allow(clippy::expect_used)]
    regex::Regex::new(r"\s*\([^)]*\bminus\b[^)]*\)").expect("invalid minus-paren regex")
});

// --- Inner-content predicates ------------------------------------------------

/// The inner is *entirely* a cross-reference (page refs + connectors). Anchored
/// so the whole inner must match, mirroring `strip_cross_reference`'s scope: a
/// paren mixing a page ref with real content is not a cross-reference.
pub(crate) fn is_cross_reference(inner: &str) -> bool {
    if matches!(
        inner.trim().to_ascii_lowercase().as_str(),
        "here" | "see below" | "see above" | "see method" | "see method here"
    ) {
        return true;
    }
    // CROSS_REF matches "(...)"; wrap the inner so the anchored regex sees the
    // parens it expects, and require the whole span to be consumed.
    let wrapped = format!("({inner})");
    CROSS_REF
        .find(&wrapped)
        .is_some_and(|m| m.as_str().trim() == wrapped)
}

/// The inner is exactly a "(see) note(s)" reference.
pub(crate) fn is_note_reference(inner: &str) -> bool {
    let wrapped = format!("({inner})");
    NOTE_REF
        .find(&wrapped)
        .is_some_and(|m| m.as_str().trim() == wrapped)
}

/// The inner contains a "minus" arithmetic-equivalence.
pub(crate) fn is_minus_equivalence(inner: &str) -> bool {
    let wrapped = format!("({inner})");
    MINUS_PAREN.is_match(&wrapped)
}

/// The inner is exactly "optional" (case-insensitive).
pub(crate) fn is_optional(inner: &str) -> bool {
    inner.trim().eq_ignore_ascii_case("optional")
        || CROSS_REF_OPTIONAL.is_match(&format!("({inner})"))
        || optional_note(inner).is_some()
}

/// A semicolon separates an optional marker from an authored note. Unlike a
/// pure reference, the note remains display text and retains its own origin.
pub(crate) fn optional_note(inner: &str) -> Option<&str> {
    let (note, marker) = inner.rsplit_once(';')?;
    (marker.trim().eq_ignore_ascii_case("optional") && !note.trim().is_empty()).then(|| note.trim())
}

/// The inner is a *descriptive* aside — a temperature (`°`) or a distance-unit
/// token.
/// (the ambiguous "in"/"m" bases count only when number-adjacent).
pub(crate) fn is_descriptive(inner: &str) -> bool {
    if inner.contains('°') {
        return true;
    }
    let number_adjacent = |token_start: usize| {
        inner[..token_start]
            .chars()
            .rev()
            .find(|c| !c.is_whitespace() && *c != '-' && *c != '/')
            .is_some_and(|c| c.is_ascii_digit() || crate::fraction::is_vulgar(c))
    };
    let is_distance_token = |token_start: usize, w: &str| {
        crate::parser::is_distance_unit(w)
            && (!matches!(w.to_lowercase().as_str(), "in" | "m") || number_adjacent(token_start))
    };
    let mut token_start = 0usize;
    let mut in_token = false;
    for (i, c) in inner
        .char_indices()
        .chain(std::iter::once((inner.len(), ' ')))
    {
        if c.is_alphabetic() {
            if !in_token {
                token_start = i;
                in_token = true;
            }
        } else if in_token {
            if is_distance_token(token_start, &inner[token_start..i]) {
                return true;
            }
            in_token = false;
        }
    }
    false
}

/// Parse a parenthetical's amount payload once. Explicit quantities and
/// approximation qualifiers are supported; source/yield descriptions are not.
pub(crate) fn amounts(inner: &str, units: &std::collections::HashSet<String>) -> Vec<Measure> {
    let mut text = inner.trim();
    // A per-item weight describes the counted food; it is not an equivalent
    // total amount. Do not mistake "each" for an opaque counted noun.
    if text
        .split_whitespace()
        .next_back()
        .is_some_and(|word| word.eq_ignore_ascii_case("each"))
    {
        return Vec::new();
    }
    for prefix in ["about ", "approximately ", "roughly ", "around "] {
        if text
            .get(..prefix.len())
            .is_some_and(|start| start.eq_ignore_ascii_case(prefix))
        {
            text = text[prefix.len()..].trim_start();
        }
    }
    if !(text
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit() || crate::fraction::is_vulgar(c))
        || crate::parser::text_number(text).is_ok())
    {
        return Vec::new();
    }
    let parser = MeasurementParser::new(units, MeasurementMode::IngredientList);
    let mut amounts = Vec::new();
    // A counted noun can separate equivalent measures: "12 peaches, 4 pounds".
    // Every comma-delimited part must independently be a measure; a descriptive
    // continuation such as "12 peaches, peeled" keeps the entire aside intact.
    let separators = text.match_indices(',').filter_map(|(i, _)| {
        // A thousands separator belongs to the quantity, not the aside list.
        let numeric = i > 0
            && text.as_bytes()[i - 1].is_ascii_digit()
            && text.as_bytes().get(i + 1).is_some_and(u8::is_ascii_digit);
        (!numeric).then_some(i)
    });
    let mut start = 0;
    for end in separators.chain(std::iter::once(text.len())) {
        let part = &text[start..end];
        start = end + 1;
        let Ok((remaining, mut measures)) = parser.parse_measurement_list(part.trim()) else {
            return Vec::new();
        };
        let remaining = remaining.trim();
        let count_size = super::vocab::SIZE_UNIT_WORDS.iter().find(|size| {
            remaining
                .get(..size.len())
                .is_some_and(|s| s.eq_ignore_ascii_case(size))
                && remaining[size.len()..]
                    .chars()
                    .next()
                    .is_none_or(char::is_whitespace)
        });
        let counted_noun = count_size.map_or(remaining, |size| remaining[size.len()..].trim());
        if !(remaining.is_empty()
            || remaining.eq_ignore_ascii_case("in total")
            || counted_noun.is_empty()
            || (counted_noun.split_whitespace().count() == 1
                && counted_noun.chars().all(char::is_alphabetic)))
        {
            return Vec::new();
        }
        if let Some(size) = count_size
            && let Some(measure) = measures.last_mut()
            && matches!(measure.unit(), crate::unit::Unit::Whole)
        {
            *measure = measure.relabel_unit(size);
        }
        amounts.extend(measures);
    }
    amounts
}

/// The inner is a bare alias — non-empty, no digits, no vulgar fractions.
pub(crate) fn is_alias(inner: &str) -> bool {
    let inner = inner.trim();
    !inner.is_empty()
        && !inner
            .chars()
            .any(|c| c.is_ascii_digit() || crate::fraction::is_vulgar(c))
}

/// Classify one parenthetical's inner text, most-specific kind first.
///
/// `units` enables the [`ParenKind::Amount`] check (measurement parsing needs
/// the parser's unit set); pass `None` to skip it. Because `Amount` sits below
/// the specific-shape kinds, a measurement-looking paren that is *also* a
/// cross-reference/note/minus/optional/descriptive is caught by the earlier,
/// tighter kind — matching each site's own ordering.
pub(crate) fn classify(inner: &str, units: Option<&HashSet<String>>) -> ParenKind {
    if is_cross_reference(inner) {
        return ParenKind::CrossReference;
    }
    if is_note_reference(inner) {
        return ParenKind::NoteReference;
    }
    if is_minus_equivalence(inner) {
        return ParenKind::MinusEquivalence;
    }
    if is_optional(inner) {
        return ParenKind::Optional;
    }
    if is_descriptive(inner) {
        return ParenKind::Descriptive;
    }
    if let Some(units) = units
        && !amounts(inner, units).is_empty()
    {
        return ParenKind::Amount;
    }
    if is_alias(inner) {
        return ParenKind::Alias;
    }
    ParenKind::Other
}

/// A single top-level parenthetical span within `s`.
pub(crate) struct ParenSpan<'a> {
    /// Byte range of the whole `(...)` (inclusive of both parens) within `s`.
    pub range: std::ops::Range<usize>,
    /// The inner text between the parens (not trimmed).
    pub inner: &'a str,
}

/// Iterate every top-level parenthetical span in `s` (depth-zero opens only;
/// nested parens are contained within their outer span). Reuses
/// [`matching_close_paren`] for balanced-paren scanning.
pub(crate) fn spans(s: &str) -> impl Iterator<Item = ParenSpan<'_>> {
    let mut cursor = 0usize;
    std::iter::from_fn(move || {
        if cursor >= s.len() {
            return None;
        }
        let rel_open = s[cursor..].find('(')?;
        let open = cursor + rel_open;
        // Unbalanced from `open` on ends iteration.
        let rel_close = matching_close_paren(&s[open..])?;
        let close = open + rel_close;
        let inner = &s[open + 1..close];
        cursor = close + 1;
        Some(ParenSpan {
            range: open..close + 1,
            inner,
        })
    })
}

#[cfg(test)]
mod tests;
