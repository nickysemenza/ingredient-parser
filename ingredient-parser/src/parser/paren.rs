//! Parenthetical classification — the single "what is this paren?" primitive.
//!
//! Parenthetical handling is spread across the pipeline: `normalize` strips
//! cross-references, note references, and minus-equivalence asides; it lifts
//! descriptive asides and splits crossref+optional; the refine phase hoists
//! measurement parentheticals as secondary amounts and recovers an alias paren
//! back into the name. Each site historically carried its *own* predicate for
//! recognizing the paren shape it cared about.
//!
//! This module centralizes the *classification* question — given the inner text
//! of a top-level parenthetical, what kind is it? — while the *actions* (strip,
//! lift, hoist, recover) stay at their existing sites. Where a normalize rewrite
//! both classifies and strips in one `replace_all`, the regex **definition** now
//! lives here as a shared `pub(crate)` static and normalize calls `replace_all`
//! on it; the definition has one home even though the action stays put.
//!
//! ## Semantic caveats (why some sites keep local guards)
//!
//! The five sites do not all mean exactly the same thing by a given kind, so
//! [`classify`] is a *shared reference ordering*, not a drop-in replacement for
//! every site's control flow:
//!
//! - **`strip_minus_equivalence`** classifies as `MinusEquivalence` but then
//!   applies an *extra* guard (only strip when a quantity remains elsewhere on
//!   the line) — a whole-line-context test [`classify`] can't see from `inner`
//!   alone. That guard stays at the site.
//! - **`Amount`** here means "the inner text parses as a measurement list with a
//!   simple remainder" — the same core test `refine::amounts` runs, minus its
//!   distance-aside rejection and approximation-prefix stripping, which stay at
//!   the site.
//! - **`Descriptive`** mirrors `lift_inline_descriptive_paren`'s *inner* test
//!   (temperature `°` or a number-adjacent distance token); the site keeps its
//!   surrounding "name (aside) name" position guard.
//! - **`Alias`** mirrors `recover_parenthetical_alias_from_modifier`'s inner
//!   guard (no digits, no vulgar fractions); the site keeps its position and
//!   head-noun logic.

// This module is a shared classification primitive. Its narrow predicates
// (`is_descriptive`, `is_alias`) and regex statics are wired into normalize/refine
// sites today; the higher-level `classify`/`spans`/`ParenKind` surface is the
// single-source-of-truth entry point exercised by the unit-test table here and
// intended for the remaining sites as the consolidation proceeds. Allow the
// not-yet-called-from-production arms rather than gating them behind `#[cfg(test)]`
// (which would make the primitive un-callable from production when a site adopts it).
#![allow(dead_code)]

use std::collections::HashSet;

use crate::parser::token::matching_close_paren;
use crate::parser::{MeasurementMode, MeasurementParser};

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
    /// Mirrors `normalize::lift_inline_descriptive_paren`'s inner detection.
    Descriptive,
    /// "(about 2 cups)", "(120g)" — a measurement that can hoist as a secondary
    /// amount. Requires `units` to be `Some`; parses the inner as a measurement
    /// list. `None` units disables this check (yields a later kind).
    Amount,
    /// "(red)" in "purple (red) cabbage" — a bare alias with no digits or vulgar
    /// fractions. Mirrors `refine::recover`'s inner-content guard.
    Alias,
    /// None of the above.
    Other,
}

// --- Shared regex definitions -------------------------------------------------
//
// These are the classify-AND-strip regexes that `normalize` still calls
// `replace_all` on. Their DEFINITIONS live here (one home) even though the
// stripping action stays in normalize.

/// Matches a cross-reference parenthetical whose content is entirely page
/// references and their connectors. See [`ParenKind::CrossReference`].
pub(crate) static CROSS_REF: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    #[allow(clippy::expect_used)]
    regex::Regex::new(
        r"(?i)\s*\(\s*(?:see\s+)?(?:this page|page\s+\d+)(?:[\s,;]*(?:to|or|and)?[\s,;]*(?:see\s+)?(?:this page|page\s+\d+))*\s*\)",
    )
    .expect("invalid cross-ref regex")
});

/// Matches a mixed cross-reference + "optional" parenthetical, reduced to
/// "(optional)" by `split_crossref_optional`.
pub(crate) static CROSS_REF_OPTIONAL: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(
    || {
        #[allow(clippy::expect_used)]
        regex::Regex::new(
            r"(?i)\(\s*(?:see\s+)?(?:this page|page\s+\d+)(?:[\s,;]*(?:to|or|and)?[\s,;]*(?:see\s+)?(?:this page|page\s+\d+))*[\s,;]+optional\s*\)",
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

// --- Inner-content predicates (shared with normalize/refine site guards) ------

/// The inner is *entirely* a cross-reference (page refs + connectors). Anchored
/// so the whole inner must match, mirroring `strip_cross_reference`'s scope: a
/// paren mixing a page ref with real content is not a cross-reference.
pub(crate) fn is_cross_reference(inner: &str) -> bool {
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
}

/// The inner is a *descriptive* aside — a temperature (`°`) or a distance-unit
/// token. Mirrors the inner detection in `normalize::lift_inline_descriptive_paren`
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

/// The inner parses as a measurement list with a simple remainder — the core
/// test `refine::amounts` runs before hoisting a secondary amount. Strips a
/// leading approximation word ("about"/"approximately"/…) as that site does.
/// A distance-only aside ("(about 3-inch)") still classifies as `Amount` here;
/// the refine site separately rejects those, since the whole-parse context is
/// what makes them shape rather than quantity.
pub(crate) fn is_amount(inner: &str, units: &HashSet<String>) -> bool {
    let text = strip_approximation_prefix(inner.trim());
    let mp = MeasurementParser::new(units, MeasurementMode::IngredientList);
    let Ok((remaining, measures)) = mp.parse_measurement_list(text) else {
        return false;
    };
    if measures.is_empty() {
        return false;
    }
    let remaining_trimmed = remaining.trim();
    remaining_trimmed.is_empty()
        || (remaining_trimmed.split_whitespace().count() == 1
            && remaining_trimmed.chars().all(char::is_alphabetic))
}

/// Strip a leading approximation word so "(about 2 cups)" measures as "2 cups".
fn strip_approximation_prefix(text: &str) -> &str {
    for prefix in ["about ", "approximately ", "roughly ", "around ", "from "] {
        if let Some(rest) = text
            .strip_prefix(prefix)
            .or_else(|| text.strip_prefix(&prefix.to_uppercase()))
        {
            return rest.trim_start();
        }
        // Case-insensitive check without allocating for the common lowercase case.
        if text.len() >= prefix.len() && text[..prefix.len()].eq_ignore_ascii_case(prefix) {
            return text[prefix.len()..].trim_start();
        }
    }
    text
}

/// The inner is a bare alias — non-empty, no digits, no vulgar fractions.
/// Mirrors `refine::recover::recover_parenthetical_alias_from_modifier`'s guard.
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
        && is_amount(inner, units)
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
