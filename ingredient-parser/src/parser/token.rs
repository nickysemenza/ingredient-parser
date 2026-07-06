//! Shared token-level helpers for the refine passes.
//!
//! These are small string/word utilities that several refine passes had each
//! copied inline. Centralizing them keeps the exact normalization/predicate
//! behavior in one place (and one place to unit-test), so the passes agree on
//! what a "word" is and when it looks like a preparation token.

use std::collections::HashSet;

/// Normalize a word for vocab lookup: strip leading/trailing punctuation and
/// lowercase. `trim_matches` only strips from the ends, so an *internal* hyphen
/// ("bone-in") survives — the suffix checks in [`is_prep_token`] depend on that.
pub(crate) fn norm(word: &str) -> String {
    word.trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase()
}

/// Iterate whitespace-split words paired with their byte offset within `s`.
/// The words are subslices of `s`, so pointer arithmetic recovers each start.
pub(crate) fn offsets(s: &str) -> impl Iterator<Item = (usize, &str)> {
    let base = s.as_ptr() as usize;
    s.split_whitespace()
        .map(move |w| (w.as_ptr() as usize - base, w))
}

/// Whether an already-lowercased word looks like a participle: it ends in "ed"
/// or is a known preparation adjective. Callers lowercase before calling (both
/// original sites did), so this takes `word_lower` and does not lowercase again.
pub(crate) fn is_participle(word_lower: &str, adjectives: &HashSet<String>) -> bool {
    word_lower.ends_with("ed") || adjectives.contains(word_lower)
}

/// Whether `word` is a "prep" token as used by
/// `refine::recover::recover_head_noun_from_modifier`: a preparation participle
/// ("-ed"), an "-ly" adverb ("roughly"/"finely"), a hyphenless descriptor
/// ("boneless"/"seedless"), a hyphenated meat/prep descriptor
/// ("bone-in"/"skin-on"/"sugar-free"), or a known intensifier adverb.
/// Deliberately NOT the broad adjective set — a descriptive adjective like
/// "fresh" must lead the head noun, not be swallowed as prep.
pub(crate) fn is_prep_token(word: &str) -> bool {
    let wl = norm(word);
    wl.ends_with("ed")
        || wl.ends_with("ly")
        || wl.ends_with("less")
        || wl
            .rsplit_once('-')
            .is_some_and(|(_, suf)| matches!(suf, "in" | "on" | "out" | "off" | "free" | "style"))
        || crate::parser::vocab::INTENSIFIER_ADVERBS.contains(&wl.as_str())
}

/// Byte index of the `)` that closes the `(` opening at the start of `input`.
/// Returns `None` if `input` does not start with `(` at depth zero, or the
/// parentheses are unbalanced. Depth-aware, so nested parens are handled.
pub(crate) fn matching_close_paren(input: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (index, character) in input.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn norm_survives_internal_hyphen() {
        assert_eq!(norm("bone-in"), "bone-in");
        assert_eq!(norm("skin-on"), "skin-on");
    }

    #[test]
    fn norm_strips_surrounding_punctuation() {
        assert_eq!(norm("(red),"), "red");
        assert_eq!(norm("chopped,"), "chopped");
        assert_eq!(norm("\"quoted\""), "quoted");
    }

    #[test]
    fn norm_lowercases() {
        assert_eq!(norm("Fresh"), "fresh");
        assert_eq!(norm("BONE-IN"), "bone-in");
    }

    #[test]
    fn offsets_reports_byte_positions() {
        let s = "  chicken   thighs";
        let got: Vec<(usize, &str)> = offsets(s).collect();
        assert_eq!(got, vec![(2, "chicken"), (12, "thighs")]);
        // Offsets index back into the original string.
        for (off, word) in offsets(s) {
            assert_eq!(&s[off..off + word.len()], word);
        }
    }

    #[test]
    fn is_participle_matches_ed_and_adjectives() {
        let mut adjectives = HashSet::new();
        adjectives.insert("fresh".to_string());
        assert!(is_participle("chopped", &adjectives));
        assert!(is_participle("fresh", &adjectives));
        assert!(!is_participle("onion", &adjectives));
    }

    #[test]
    fn is_prep_token_positive() {
        for w in [
            "deribbed",
            "roughly",
            "boneless",
            "bone-in",
            "skin-on",
            "sugar-free",
        ] {
            assert!(is_prep_token(w), "{w:?} should be a prep token");
        }
    }

    #[test]
    fn is_prep_token_negative() {
        for w in ["chicken", "fresh", "onion"] {
            assert!(!is_prep_token(w), "{w:?} should not be a prep token");
        }
    }

    #[test]
    fn matching_close_paren_simple() {
        assert_eq!(matching_close_paren("(red) cabbage"), Some(4));
    }

    #[test]
    fn matching_close_paren_nested() {
        // The outer close is at the end of "(a (b) c)".
        let s = "(a (b) c) rest";
        assert_eq!(matching_close_paren(s), Some(8));
        assert_eq!(&s[..=8], "(a (b) c)");
    }

    #[test]
    fn matching_close_paren_unbalanced() {
        assert_eq!(matching_close_paren("(no close here"), None);
        // Extra close before any open bottoms out the depth counter.
        assert_eq!(matching_close_paren(") stray"), None);
    }
}
