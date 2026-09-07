//! Assembly repairs owned by the segment stage.
//!
//! These repairs operate on clause structure after assembly and before the
//! name-internal refine pipeline. Keeping them beside segmentation makes the
//! ordering contract and the text they move one local concern.

use super::*;

impl IngredientParser {
    /// Recover a head noun stranded at the tail of an alternatives list in the
    /// modifier. The grammar splits "canola, vegetable, or melted coconut oil" on
    /// the first comma, leaving name="canola" and modifier="vegetable, or melted
    /// coconut oil" — the shared head "oil" dropped off the name entirely. When
    /// the modifier is a comma+or list ending in a curated shared-head noun and
    /// the name is a single bare token, graft the head onto the name ("canola" →
    /// "canola oil") and keep the whole list as an "or …" alternative modifier.
    ///
    /// Gated narrowly (requires a comma *and* an "or", plus a final word in
    /// [`vocab::SHARED_HEAD_NOUNS`]) so lists of complete ingredients —
    /// "salt, pepper, or paprika", "flour, sugar, or baking soda" — never get a
    /// nonsense head grafted on.
    pub(in crate::parser) fn recover_shared_head_from_alternatives(
        &self,
        parsed: &mut ParsedIngredient,
    ) {
        let mut name_words = parsed.name.split_whitespace();
        let (Some(name_word), None) = (name_words.next(), name_words.next()) else {
            return;
        };
        if crate::parser::vocab::SHARED_HEAD_NOUNS.contains(&name_word.to_lowercase().as_str()) {
            return;
        }
        let Some(modifier) = parsed.modifier_string() else {
            return;
        };
        let Some(head) = comma_or_shared_head(&modifier) else {
            return;
        };
        parsed.name = format!("{name_word} {head}");
        parsed.modifier = vec![ModifierPart::Alternative(format!("or {modifier}"))];
    }

    /// Hoist a secondary amount parenthetical out of the assembled modifier.
    pub(in crate::parser) fn extract_secondary_amounts_from_modifier(
        &self,
        parsed: &mut ParsedIngredient,
    ) {
        let Some(modifier) = parsed.modifier_string() else {
            return;
        };

        let (secondary_amounts, cleaned_modifier) =
            extract_secondary_amounts(&modifier, &self.units);
        if secondary_amounts.is_empty() {
            return;
        }
        parsed.amounts.extend(secondary_amounts);
        parsed.modifier = if cleaned_modifier.trim().is_empty() {
            Vec::new()
        } else {
            vec![ModifierPart::Raw(cleaned_modifier)]
        };
    }

    /// Recover from a leading prep phrase that displaced the ingredient name.
    ///
    /// A line like "2/3 cup finely chopped, raw pistachios" parses with the
    /// text *before* the comma as the name and the text *after* as the modifier,
    /// yielding name="finely chopped" / modifier="raw pistachios" — backwards.
    /// When the whole name is a single known prep phrase and a modifier is
    /// present, swap them so the prep phrase becomes the modifier and the real
    /// name is restored. The exact-match guard keeps descriptive names (e.g.
    /// "raw pistachios, finely chopped", where the name isn't a prep phrase) from
    /// ever being touched.
    pub(in crate::parser) fn fix_leading_prep_phrase(&self, parsed: &mut ParsedIngredient) {
        let name = parsed.name.trim();
        if name.is_empty() || !self.adjectives.contains(&name.to_lowercase()) {
            return;
        }
        let Some(modifier) = parsed.modifier_string() else {
            return;
        };
        let prep = name.to_string();
        parsed.name = modifier;
        parsed.modifier = vec![ModifierPart::Prep(prep)];
    }

    /// Recover from a leading subtractive clause that displaced the name, e.g.
    /// "½ cup minus 1 tablespoon flour" parses with "½ cup" as the amount and
    /// "minus 1 tablespoon flour" as the name. When the name begins with "minus"
    /// followed by a parseable measurement, move "minus <measure>" into the
    /// modifier and restore the real name ("flour"). The primary amount is left
    /// as stated (the subtraction isn't applied numerically).
    pub(in crate::parser) fn fix_leading_minus_clause(&self, parsed: &mut ParsedIngredient) {
        // Borrow for the prefix guard; only allocate once we've confirmed a match.
        let Some(rest) = parsed
            .name
            .strip_prefix("minus ")
            .or_else(|| parsed.name.strip_prefix("Minus "))
        else {
            return;
        };
        let mp = MeasurementParser::new(&self.units, MeasurementMode::IngredientList);
        let Ok((remaining, measures)) = mp.parse_measurement_list(rest) else {
            return;
        };
        if measures.is_empty() || remaining.trim().is_empty() {
            return;
        }
        let consumed = rest[..rest.len() - remaining.len()].trim();
        let clause = format!("minus {consumed}");
        let new_name = remaining.trim().to_string();
        // The `parsed.name` borrows (rest/remaining/consumed) all end above.
        parsed.name = new_name;
        // Prepend the subtractive clause so it leads the modifier ("minus …, …").
        parsed.modifier.insert(0, ModifierPart::Raw(clause));
    }

    /// Recover a head noun stranded behind a leading participle chain. The grammar
    /// carves the name at the first comma, so a line like "1/2 cup deribbed,
    /// seeded, and roughly chopped fresh hot green chiles, such as serrano" leaves
    /// name="deribbed" and the real ingredient ("fresh hot green chiles") buried in
    /// the `Raw` modifier. This is the mirror of [`Self::extract_trailing_prep_clause`]:
    /// it pulls the head noun *out of* the modifier *into* an all-participle name.
    ///
    /// Also handles a leading hyphenated/-less adjective chain ("bone-in, skin-on
    /// chicken legs" -> name "chicken legs", modifier "bone-in, skin-on").
    ///
    /// Tightly guarded to avoid touching legitimate names:
    /// - the name must be a *pure* prep chain (every token a participle "-ed"/"-ly",
    ///   a hyphenated/-less descriptor "bone-in"/"boneless", or an intensifier
    ///   adverb) — any real noun in the name and it bails, so "chopped onion" /
    ///   "peeled and diced potatoes" are untouched;
    /// - the modifier's first part must be `Raw` and yield a head noun whose first
    ///   word is not a stopword, so a prose modifier ("then served over ice") bails.
    ///
    /// Runs after [`Self::fix_leading_prep_phrase`] (so the vocab-adjective case
    /// "chopped, toasted walnuts" is already resolved and never reaches here) and
    /// before `extract_adjectives_from_name` (so the recovered name still gets the
    /// normal adjective scan).
    pub(in crate::parser) fn recover_head_noun_from_modifier(&self, parsed: &mut ParsedIngredient) {
        use crate::parser::token::{is_prep_token as is_prep, norm, offsets};
        let is_connector = |w: &str| {
            let wl = norm(w);
            wl == "and" || wl == "&"
        };

        // Precondition: the name is a pure leading prep chain.
        let name_pure_prep =
            !parsed.name.trim().is_empty() && parsed.name.split_whitespace().all(&is_prep);
        if !name_pure_prep {
            return;
        }

        // The first modifier part must be raw grammar text (the post-comma tail).
        let Some(ModifierPart::Raw(modtext)) = parsed.modifier.first() else {
            return;
        };
        let modtext = modtext.clone();

        // Walk tokens, skipping leading preps/connectors, to find the head noun's
        // byte offset within `modtext`.
        let head_start = offsets(&modtext)
            .find(|(_, w)| !is_prep(w) && !is_connector(w))
            .map(|(off, _)| off);
        let Some(head_start) = head_start else {
            return; // modifier was all prep — nothing to recover.
        };

        let rest = &modtext[head_start..];
        let first_word = rest.split_whitespace().next().unwrap_or("");
        let first_lower = norm(first_word);
        // Stopwords that, as the would-be head noun's first word, mean the modifier
        // is a prose clause, not "<preps> <head noun>".
        if crate::parser::vocab::MODIFIER_STOPWORDS.contains(&first_lower.as_str()) {
            return;
        }

        // The head noun runs to the next clause boundary (see
        // `vocab::CLAUSE_BOUNDARIES`). " (" ends it at a trailing parenthetical
        // aside ("chicken thighs (8 to 12 thighs, …)"), before the comma *inside*
        // that aside can truncate the noun.
        let mut end = rest.len();
        for pat in crate::parser::vocab::CLAUSE_BOUNDARIES {
            if let Some(p) = rest.find(pat) {
                end = end.min(p);
            }
        }
        let head_noun = rest[..end].trim();
        if head_noun.is_empty() {
            return;
        }
        let trailing = rest[end..]
            .trim_start_matches(|c: char| c == ',' || c.is_whitespace())
            .trim();

        // The prep prefix is the original name plus everything consumed up to the
        // head noun (preserving the "and"/commas), e.g.
        // "deribbed" + "seeded, and roughly chopped".
        let consumed = modtext[..head_start].trim().trim_end_matches(',').trim();
        let prep = if consumed.is_empty() {
            parsed.name.trim().to_string()
        } else {
            format!("{}, {}", parsed.name.trim(), consumed)
        };

        // Rebuild: head noun is the name; prep leads the modifier; the trailing
        // clause follows; any later modifier parts are preserved.
        let tail_parts = parsed.modifier.split_off(1);
        parsed.name = head_noun.to_string();
        parsed.modifier = vec![ModifierPart::Prep(prep)];
        if !trailing.is_empty() {
            parsed
                .modifier
                .push(ModifierPart::Raw(trailing.to_string()));
        }
        parsed.modifier.extend(tail_parts);
    }

    /// Recover a head noun stranded behind an inline parenthetical alias, e.g.
    /// "1 medium purple (red) cabbage (about 1 pound)" reaches refine as
    /// name="purple" and modifier="(red) cabbage (about 1 pound)". Move the
    /// leading "(red) cabbage" back into the name and leave later modifier text
    /// for the normal secondary-amount pass.
    pub(in crate::parser) fn recover_parenthetical_alias_from_modifier(
        &self,
        parsed: &mut ParsedIngredient,
    ) {
        let Some(ModifierPart::Raw(raw)) = parsed.modifier.first() else {
            return;
        };
        let raw = raw.clone();
        let trimmed = raw.trim_start();
        if !trimmed.starts_with('(') {
            return;
        }
        let Some(close) = crate::parser::token::matching_close_paren(trimmed) else {
            return;
        };
        let inner = trimmed[1..close].trim();
        // The "is this a bare alias?" test (non-empty, no digits/vulgar fractions)
        // is shared with `paren::classify` (ParenKind::Alias); this site keeps its
        // own position and head-noun recovery logic below.
        if !crate::parser::paren::is_alias(inner) {
            return;
        }

        let after = trimmed[close + 1..].trim_start();
        if !after.chars().next().is_some_and(char::is_alphabetic) {
            return;
        }

        let head_end = after
            .find(" (")
            .or_else(|| after.find(", "))
            .unwrap_or(after.len());
        let head = after[..head_end].trim();
        if head.is_empty() || !head.chars().any(char::is_alphabetic) {
            return;
        }

        let recovered = format!("({inner}) {head}");
        parsed.name = collapse_whitespace(&format!("{} {recovered}", parsed.name));

        let remainder = after[head_end..]
            .trim_start_matches(|c: char| c == ',' || c.is_whitespace())
            .trim()
            .to_string();
        if remainder.is_empty() {
            parsed.modifier.remove(0);
        } else if let Some(ModifierPart::Raw(raw)) = parsed.modifier.first_mut() {
            *raw = remainder;
        }
    }
}

/// A comma+or alternatives list whose final token is a curated shared head.
fn comma_or_shared_head(right: &str) -> Option<&str> {
    if !right.contains(',') || !right.to_lowercase().contains(" or ") {
        return None;
    }
    let head = right
        .split_whitespace()
        .next_back()
        .map(|token| token.trim_matches(|c: char| !c.is_alphanumeric()))?;
    crate::parser::vocab::SHARED_HEAD_NOUNS
        .contains(&head.to_lowercase().as_str())
        .then_some(head)
}

// This repair needs match spans so it can remove the amount parenthetical from
// the modifier while preserving the surrounding authored text.
fn extract_secondary_amounts(
    modifier: &str,
    units: &std::collections::HashSet<String>,
) -> (Vec<Measure>, String) {
    crate::lazy_regex!(
        SECONDARY_AMOUNT_PATTERN,
        r"\((?:from\s+)?(?:about|approximately|roughly|around)\s+([^)]+)\)"
    );
    crate::lazy_regex!(TRAILING_MEASURE_PATTERN, r"\(([^)]+)\)\s*$");

    let Some(caps) = SECONDARY_AMOUNT_PATTERN
        .captures(modifier)
        .or_else(|| TRAILING_MEASURE_PATTERN.captures(modifier))
    else {
        return (vec![], modifier.to_string());
    };
    let Some(full_match) = caps.get(0) else {
        return (vec![], modifier.to_string());
    };
    let Some(amount_match) = caps.get(1) else {
        return (vec![], modifier.to_string());
    };
    let amount_text = amount_match.as_str().trim();

    let mp = MeasurementParser::new(units, MeasurementMode::IngredientList);
    let Ok((remaining, measures)) = mp.parse_measurement_list(amount_text) else {
        return (vec![], modifier.to_string());
    };

    let is_distance = |m: &Measure| match m.unit() {
        unit::Unit::Inch => true,
        unit::Unit::Other(s) => crate::parser::is_distance_unit(s),
        _ => false,
    };
    if measures.iter().any(is_distance) {
        return (vec![], modifier.to_string());
    }

    let remaining = remaining.trim();
    let simple_remainder = remaining.is_empty()
        || (remaining.split_whitespace().count() == 1
            && remaining.chars().all(char::is_alphabetic));
    if !simple_remainder || measures.is_empty() {
        return (vec![], modifier.to_string());
    }

    let cleaned = collapse_whitespace(&format!(
        "{}{}",
        &modifier[..full_match.start()],
        &modifier[full_match.end()..]
    ));
    (measures, cleaned)
}

#[cfg(test)]
mod helper_tests {
    //! Direct coverage for the shared-head gates. The end-to-end behavior is
    //! pinned by `refine/tests.rs` and the accuracy corpus; these rows exercise
    //! each gate in isolation, including the two the corpus cannot reach.
    use super::*;
    use rstest::rstest;

    /// Fires on a comma+or list ending in a `SHARED_HEAD_NOUNS` word, preserving
    /// the source casing of the grafted head. The gates (comma AND " or " AND
    /// curated final noun) each reject when absent.
    #[rstest]
    #[case::fires("vegetable, or melted coconut oil", Some("oil"))]
    // Casing preserved: the vocab lookup lowercases, the returned head does not.
    #[case::casing_preserved("vegetable, or Coconut Oil", Some("Oil"))]
    #[case::no_comma("or oil", None)]
    #[case::no_or("vegetable, coconut oil", None)]
    #[case::final_not_curated("sugar, or baking soda", None)]
    fn comma_or_shared_head_gates(#[case] right: &str, #[case] expected: Option<&str>) {
        assert_eq!(comma_or_shared_head(right), expected, "right: {right}");
    }
}
