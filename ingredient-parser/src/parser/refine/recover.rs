use super::*;

impl IngredientParser {
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
    pub(super) fn fix_leading_prep_phrase(&self, parsed: &mut ParsedIngredient) {
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
    pub(super) fn fix_leading_minus_clause(&self, parsed: &mut ParsedIngredient) {
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
    pub(super) fn recover_head_noun_from_modifier(&self, parsed: &mut ParsedIngredient) {
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
    pub(super) fn recover_parenthetical_alias_from_modifier(&self, parsed: &mut ParsedIngredient) {
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
