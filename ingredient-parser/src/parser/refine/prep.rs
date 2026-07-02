use super::*;

/// An adjective after a word-boundary " or " belongs to the ALTERNATIVE, not
/// the primary: leave it for the alternative passes ("basil or chopped parsley"
/// must keep "chopped" with parsley, not read as prep for basil). `pos` is the
/// adjective's byte offset in `name_lower`.
fn adjective_belongs_to_alternative(name_lower: &str, pos: usize) -> bool {
    name_lower.find(" or ").is_some_and(|or_pos| pos > or_pos)
}

/// An adjective after a word-boundary " and " usually belongs to the second
/// conjunct of an "X and Y" line ("Kosher salt and freshly ground black pepper"
/// — "freshly ground" modifies the pepper, not the whole line), so leave it in
/// the name. The `end < len` clause keeps a *trailing* phrase like "to taste"
/// ("Salt and pepper to taste") extractable: only a mid-seam adjective with a
/// head noun still after it is skipped. (Multi-ingredient lines with "and"
/// conjunctions are out of scope for this pass.) `pos`/`end` bound the adjective
/// in `name_lower`.
fn adjective_belongs_to_second_conjunct(name_lower: &str, pos: usize, end: usize) -> bool {
    name_lower
        .find(" and ")
        .is_some_and(|and_pos| pos > and_pos && end < name_lower.len())
}

/// "fresh" immediately before " or " is a genuine contrast ("fresh or frozen …"),
/// not the implied default — leave it in the name for the alternative pass to
/// reconstruct ("fresh blueberries"). `end` is the byte offset just past the
/// adjective in `name_lower`.
fn fresh_is_contrastive(adjective: &str, name_lower: &str, end: usize) -> bool {
    adjective == "fresh" && name_lower[end..].starts_with(" or ")
}

/// Require a whitespace/string-edge boundary on both sides of the adjective, so
/// an adjective embedded in a larger token is left alone (e.g. "chopped" inside
/// "well-chopped" must not corrupt the name into "well-"). `pos`/`end` bound the
/// adjective in `name`.
fn on_word_boundaries(name: &str, pos: usize, end: usize) -> bool {
    let before_boundary = name[..pos]
        .chars()
        .next_back()
        .is_none_or(char::is_whitespace);
    let after_boundary = name[end..].chars().next().is_none_or(char::is_whitespace);
    before_boundary && after_boundary
}

/// Fold a stranded intensifier ("very") or manner adverb ("diagonally") sitting
/// immediately before the adjective into the modifier too, extending the cut so
/// it doesn't get left behind in the name ("very thinly sliced chives" -> name
/// "chives"; "diagonally sliced scallions" -> name "scallions", modifier
/// "diagonally sliced"). Only the word directly abutting the adjective is
/// consumed. Returns `(prep_phrase, cut)`: the modifier text and the byte offset
/// in `name`/`name_lower` where the removed span starts. When no adverb folds in,
/// returns the bare adjective and `pos` unchanged.
fn extend_cut_over_adverb(
    adjective: &str,
    name: &str,
    name_lower: &str,
    pos: usize,
) -> (String, usize) {
    if let Some(prev) = name_lower[..pos].split_whitespace().next_back()
        && (crate::parser::vocab::INTENSIFIER_ADVERBS.contains(&prev)
            || crate::parser::vocab::MANNER_ADVERBS.contains(&prev))
        && let Some(wstart) = name_lower[..pos].rfind(prev)
    {
        let boundary_ok = name.is_char_boundary(wstart)
            && name[..wstart]
                .chars()
                .next_back()
                .is_none_or(char::is_whitespace);
        if boundary_ok {
            return (format!("{prev} {adjective}"), wstart);
        }
    }
    (adjective.to_string(), pos)
}

/// Remove the `[pos, end)` span from `s`, rejoining the trimmed before/after
/// slices with a single space. `pos`/`end` must be char boundaries in `s`.
///
/// Used to rebuild both `name` and its lowercase view from the same span, so
/// `name_lower` stays in sync without re-lowercasing the whole string each
/// iteration. `before`/`after` are pre-trimmed and the caller's guards never
/// emit a leading/trailing space, so the result needs no final trim.
fn join_around_span(s: &str, pos: usize, end: usize) -> String {
    let before = s[..pos].trim();
    let after = s[end..].trim();
    let mut out = String::with_capacity(s.len());
    if !before.is_empty() {
        out.push_str(before);
        if !after.is_empty() {
            out.push(' ');
        }
    }
    if !after.is_empty() {
        out.push_str(after);
    }
    out
}

impl IngredientParser {
    pub(super) fn extract_adjectives_from_name(&self, parsed: &mut ParsedIngredient) {
        let Some(mut name_lower) = crate::parser::byte_aligned_lowercase(&parsed.name) else {
            return;
        };
        let mut found_adjectives: Vec<&String> = self
            .adjectives
            .iter()
            .filter(|adj| name_lower.contains(adj.as_str()))
            .collect();
        // Common case: a clean name carries no known adjective. Bail before the
        // `parsed.name.clone()` (and the per-adjective work) rather than cloning,
        // looping zero times, and writing the name back unchanged.
        if found_adjectives.is_empty() {
            return;
        }
        // Longest-match-first, so a two-word adjective is claimed before either of
        // its component words. The driver below takes the *first* match per pass.
        found_adjectives.sort_by_key(|adj| Reverse(adj.len()));
        let mut name = parsed.name.clone();

        // Count of leading prep adjectives already moved to the front, so several
        // ("chopped minced onion") keep their source order instead of reversing.
        let mut leading_count = 0usize;
        for adjective in found_adjectives {
            let Some(pos) = name_lower.find(adjective.as_str()) else {
                continue;
            };
            let end = pos + adjective.len();

            if adjective_belongs_to_alternative(&name_lower, pos)
                || adjective_belongs_to_second_conjunct(&name_lower, pos, end)
                || fresh_is_contrastive(adjective, &name_lower, end)
                || !on_word_boundaries(&name, pos, end)
            {
                continue;
            }

            let (prep, cut) = extend_cut_over_adverb(adjective, &name, &name_lower, pos);

            // A prep adjective at the *start* of the name leads the modifier
            // ("minced lamb (not too lean)" -> "minced (not too lean)"), matching
            // what the grammar's old leading-adjective branch produced before prep
            // extraction was unified here. A mid/trailing one is appended. Several
            // leading ones keep source order via the running insert index.
            if name[..cut].trim().is_empty() {
                parsed
                    .modifier
                    .insert(leading_count, ModifierPart::Prep(prep));
                leading_count += 1;
            } else {
                parsed.push_modifier(ModifierPart::Prep(prep));
            }

            name = join_around_span(&name, cut, end);
            name_lower = join_around_span(&name_lower, cut, end);
        }

        parsed.name = name;
    }

    /// Move a trailing participial preparation clause out of the name into the
    /// modifier: "anchovy fillets mashed with the flat side of a knife into a
    /// paste" -> name "anchovy fillets", modifier "mashed with the flat side of a
    /// knife into a paste". `extract_adjectives_from_name` only relocates the
    /// adjective *word*, never its trailing prepositional tail, so this handles
    /// the "<head noun> <participle> <preposition> …" shape as a whole and runs
    /// *before* it so the full span moves intact.
    ///
    /// Tightly guarded: the trigger token must look like a participle (ends in
    /// "ed", or is a known adjective) AND be immediately followed by a cooking
    /// preposition ("with"/"into"), AND have at least one preceding word (the head
    /// noun). That last guard keeps a *leading* participle in the name
    /// ("mashed potatoes" — participle is the first word, no split), and the
    /// preposition requirement leaves plain "<noun> with <noun>" ("chicken with
    /// skin") alone since the noun isn't a participle.
    pub(super) fn extract_trailing_prep_clause(&self, parsed: &mut ParsedIngredient) {
        // Find the byte offset to cut at while only borrowing the name; the borrow
        // ends with this block so the owned-string rewrite below can reassign it.
        let cut = {
            let name = parsed.name.as_str();
            // Byte offset of each whitespace-split token within `name`.
            let tokens: Vec<(usize, &str)> = crate::parser::token::offsets(name).collect();
            let mut found = None;
            // Start at 1: index 0 is the head noun and can never be the trigger.
            for i in 1..tokens.len() {
                let Some(&(_, next)) = tokens.get(i + 1) else {
                    break;
                };
                let (start, word) = tokens[i];
                let word_lower = word.to_lowercase();
                let is_participle =
                    crate::parser::token::is_participle(&word_lower, &self.adjectives);
                let next_lower = next.to_lowercase();
                let is_cooking_prep = next_lower == "with" || next_lower == "into";
                if is_participle && is_cooking_prep && name.is_char_boundary(start) {
                    found = Some(start);
                    break;
                }
            }
            found
        };
        let Some(start) = cut else {
            return;
        };
        let clause = parsed.name[start..].trim().to_string();
        let new_name = parsed.name[..start].trim().to_string();
        if new_name.is_empty() || clause.is_empty() {
            return;
        }
        parsed.name = new_name;
        parsed.push_modifier(ModifierPart::Prep(clause));
    }

    /// Move a trailing "for …" purpose clause out of the name into the modifier.
    /// Two shapes qualify:
    /// - "for `<gerund>` …" ("Extra-virgin olive oil for brushing the bread" ->
    ///   name "Extra-virgin olive oil", modifier "for brushing the bread"), and
    /// - "for the `<noun>` …" ("Butter for the pans" -> name "Butter", modifier
    ///   "for the pans"). The definite article is the signal here; the singular
    ///   "for the pan" is a fixed vocab phrase handled by
    ///   `extract_adjectives_from_name`, but plurals/other nouns leak past it.
    ///
    /// Runs AFTER `extract_adjectives_from_name`, so fixed purpose phrases already
    /// in the vocab ("for dusting", "for garnish") are gone and aren't
    /// double-handled. The guards (next word is an "ing" gerund ≥5 chars, or the
    /// article "the") keep a plain "<name> for <noun>" like "flour for bread"
    /// intact.
    pub(super) fn extract_purpose_gerund(&self, parsed: &mut ParsedIngredient) {
        // Match the first word-boundary " for " on the original string so the
        // byte offsets stay valid for slicing.
        crate::lazy_regex!(FOR_PATTERN, r"(?i)\s+for\s+");

        // Borrow `parsed.name` for the match/guards; only the two owned result
        // strings are built before the name is reassigned, so no upfront clone.
        let Some(m) = FOR_PATTERN.find(&parsed.name) else {
            return;
        };
        let next_word = parsed.name[m.end()..]
            .split_whitespace()
            .next()
            .unwrap_or("");
        let is_gerund = next_word.len() >= 5
            && next_word.ends_with("ing")
            && next_word.chars().all(char::is_alphabetic);
        let is_for_the = next_word.eq_ignore_ascii_case("the");
        if !is_gerund && !is_for_the {
            return;
        }
        let clause = parsed.name[m.start()..].trim().to_string();
        let new_name = parsed.name[..m.start()].trim().to_string();
        parsed.name = new_name;
        parsed.push_modifier(ModifierPart::Prep(clause));
    }
}

#[cfg(test)]
mod adjective_guard_tests {
    //! Direct coverage for the guards extracted from `extract_adjectives_from_name`.
    //! End-to-end accuracy is pinned by the corpus and `refine/tests.rs`; these
    //! rows exercise each predicate in isolation. Byte offsets are computed with
    //! `.find(..)` so the rows read as intent, not hand-counted positions.
    #![allow(clippy::unwrap_used)]
    use super::*;
    use rstest::rstest;

    /// Fires only when the adjective sits *after* a word-boundary " or ".
    #[rstest]
    #[case::after_or("basil or chopped parsley", "chopped", true)]
    #[case::before_or("chopped basil or parsley", "chopped", false)]
    #[case::no_or("chopped parsley", "chopped", false)]
    fn test_adjective_belongs_to_alternative(
        #[case] name_lower: &str,
        #[case] adj: &str,
        #[case] expected: bool,
    ) {
        let pos = name_lower.find(adj).unwrap();
        assert_eq!(adjective_belongs_to_alternative(name_lower, pos), expected);
    }

    /// Fires on a mid-seam adjective after " and " that still has a head noun
    /// after it; a trailing phrase (adjective ending the string) stays extractable.
    #[rstest]
    #[case::mid_seam("salt and freshly ground pepper", "freshly", true)]
    #[case::before_and("freshly ground salt and pepper", "freshly", false)]
    #[case::trailing("salt and pepper minced", "minced", false)]
    #[case::no_and("freshly ground pepper", "freshly", false)]
    fn test_adjective_belongs_to_second_conjunct(
        #[case] name_lower: &str,
        #[case] adj: &str,
        #[case] expected: bool,
    ) {
        let pos = name_lower.find(adj).unwrap();
        let end = pos + adj.len();
        assert_eq!(
            adjective_belongs_to_second_conjunct(name_lower, pos, end),
            expected
        );
    }

    /// "fresh" is kept only when immediately followed by " or " (a contrast).
    #[rstest]
    #[case::contrast("fresh or frozen berries", "fresh", true)]
    #[case::not_contrast("fresh berries", "fresh", false)]
    #[case::other_adj("chopped or minced garlic", "chopped", false)]
    fn test_fresh_is_contrastive(
        #[case] name_lower: &str,
        #[case] adj: &str,
        #[case] expected: bool,
    ) {
        let end = name_lower.find(adj).unwrap() + adj.len();
        assert_eq!(fresh_is_contrastive(adj, name_lower, end), expected);
    }

    /// Both sides of the span must fall on whitespace or a string edge.
    #[rstest]
    #[case::clean("very chopped onion", "chopped", true)]
    #[case::at_start("chopped onion", "chopped", true)]
    #[case::embedded_before("well-chopped onion", "chopped", false)]
    #[case::embedded_after("choppedonion", "chopped", false)]
    fn test_on_word_boundaries(#[case] name: &str, #[case] adj: &str, #[case] expected: bool) {
        let pos = name.find(adj).unwrap();
        let end = pos + adj.len();
        assert_eq!(on_word_boundaries(name, pos, end), expected);
    }

    /// Folds a preceding intensifier/manner adverb into the cut; leaves a plain
    /// preceding noun (or nothing) alone.
    #[rstest]
    #[case::intensifier("very sliced chives", "sliced", "very sliced", 0)]
    #[case::manner("diagonally sliced scallions", "sliced", "diagonally sliced", 0)]
    #[case::no_adverb("baby sliced carrots", "sliced", "sliced", 5)]
    #[case::at_start("sliced onion", "sliced", "sliced", 0)]
    fn test_extend_cut_over_adverb(
        #[case] name: &str,
        #[case] adj: &str,
        #[case] expected_prep: &str,
        #[case] expected_cut: usize,
    ) {
        let name_lower = name.to_lowercase();
        let pos = name_lower.find(adj).unwrap();
        let (prep, cut) = extend_cut_over_adverb(adj, name, &name_lower, pos);
        assert_eq!((prep.as_str(), cut), (expected_prep, expected_cut));
    }

    /// Removes the span and rejoins the trimmed halves with one space; a
    /// leading/trailing removal leaves no stray whitespace.
    #[rstest]
    #[case::middle("very chopped onion", 5, 12, "very onion")]
    #[case::leading("chopped onion", 0, 8, "onion")]
    #[case::trailing("onion chopped", 6, 13, "onion")]
    fn test_join_around_span(
        #[case] s: &str,
        #[case] pos: usize,
        #[case] end: usize,
        #[case] expected: &str,
    ) {
        assert_eq!(join_around_span(s, pos, end), expected);
    }
}
