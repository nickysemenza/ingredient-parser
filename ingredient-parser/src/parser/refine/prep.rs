use super::*;

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
        found_adjectives.sort_by_key(|adj| Reverse(adj.len()));
        let mut name = parsed.name.clone();

        // Count of leading prep adjectives already moved to the front, so several
        // ("chopped minced onion") keep their source order instead of reversing.
        let mut leading_count = 0usize;
        for adjective in found_adjectives {
            let Some(pos) = name_lower.find(adjective.as_str()) else {
                continue;
            };

            // An adjective after a word-boundary " or " belongs to the
            // ALTERNATIVE, not the primary: leave it for the alternative passes
            // ("basil or chopped parsley" must keep "chopped" with parsley, not
            // read as prep for basil).
            if let Some(or_pos) = name_lower.find(" or ")
                && pos > or_pos
            {
                continue;
            }

            let end = pos + adjective.len();

            // An adjective after a word-boundary " and " usually belongs to the
            // second conjunct of an "X and Y" line ("Kosher salt and freshly
            // ground black pepper" — "freshly ground" modifies the pepper, not
            // the whole line), so leave it in the name. The `end < len` clause
            // keeps a *trailing* phrase like "to taste" ("Salt and pepper to
            // taste") extractable: only a mid-seam adjective with a head noun
            // still after it is skipped. (Multi-ingredient lines with "and"
            // conjunctions are out of scope for this pass.)
            if let Some(and_pos) = name_lower.find(" and ")
                && pos > and_pos
                && end < name_lower.len()
            {
                continue;
            }

            // "fresh" immediately before " or " is a genuine contrast
            // ("fresh or frozen …"), not the implied default — leave it in the
            // name for the alternative pass to reconstruct ("fresh blueberries").
            if adjective.as_str() == "fresh" && name_lower[end..].starts_with(" or ") {
                continue;
            }

            // Require a whitespace/string-edge boundary on both sides, so an
            // adjective embedded in a larger token is left alone (e.g. "chopped"
            // inside "well-chopped" must not corrupt the name into "well-").
            let before_boundary = name[..pos]
                .chars()
                .next_back()
                .is_none_or(char::is_whitespace);
            let after_boundary = name[end..].chars().next().is_none_or(char::is_whitespace);
            if !before_boundary || !after_boundary {
                continue;
            }

            // Fold a stranded intensifier ("very") or manner adverb ("diagonally")
            // sitting immediately before the adjective into the modifier too, and
            // extend the cut so it doesn't get left behind in the name ("very thinly
            // sliced chives" -> name "chives"; "diagonally sliced scallions" -> name
            // "scallions", modifier "diagonally sliced"). Only the word directly
            // abutting the adjective is consumed.
            let mut prep = adjective.clone();
            let mut cut = pos;
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
                    prep = format!("{prev} {adjective}");
                    cut = wstart;
                }
            }
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

            // Rebuild both `name` and its lowercase view from the same before/after
            // slices, so `name_lower` is kept in sync without re-lowercasing the
            // whole string each iteration. `pos`/`end` are char boundaries in both
            // strings (verified for `name`; for `name_lower` they came from `find`).
            let join = |s: &str, pos: usize, end: usize| -> String {
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
                // `before`/`after` are pre-trimmed and the guards above never
                // emit a leading/trailing space, so `out` needs no final trim.
                out
            };

            name = join(&name, cut, end);
            name_lower = join(&name_lower, cut, end);
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
            // Byte offset of each whitespace-split token within `name` (the tokens
            // are subslices of `name`, so pointer arithmetic gives their start).
            let tokens: Vec<(usize, &str)> = name
                .split_whitespace()
                .map(|w| (w.as_ptr() as usize - name.as_ptr() as usize, w))
                .collect();
            let mut found = None;
            // Start at 1: index 0 is the head noun and can never be the trigger.
            for i in 1..tokens.len() {
                let Some(&(_, next)) = tokens.get(i + 1) else {
                    break;
                };
                let (start, word) = tokens[i];
                let word_lower = word.to_lowercase();
                let is_participle =
                    word_lower.ends_with("ed") || self.adjectives.contains(word_lower.as_str());
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
