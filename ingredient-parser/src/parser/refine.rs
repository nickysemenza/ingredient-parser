//! Post-parse refinement passes.
//!
//! After the grammar captures the raw shape, these passes recover misplaced
//! names, pull preparation adjectives and alternatives out of the name into the
//! modifier, and hoist secondary amounts. They run in a fixed, load-bearing
//! order (see `postprocess_ingredient`).

use std::cmp::Reverse;

use super::ir::{ModifierPart, ParsedIngredient};
use super::normalize::collapse_whitespace;
use crate::parser::{MeasurementMode, MeasurementParser};
use crate::unit::{self, Measure};
use crate::{Ingredient, IngredientParser};

impl IngredientParser {
    /// Run the ordered refinement passes on the parsed IR, then lower it to the
    /// public [`Ingredient`] (which joins the typed modifier parts back into a
    /// string and finalizes it).
    pub(super) fn postprocess_ingredient(&self, mut parsed: ParsedIngredient) -> Ingredient {
        self.refine(&mut parsed);
        parsed.into()
    }

    /// Run the ordered refinement passes in place, without lowering. Split out so
    /// a caller that needs to append more modifier text *after* refinement (the
    /// inline-descriptive-paren path) can do so through the IR before lowering,
    /// rather than hand-joining the public modifier string.
    pub(super) fn refine(&self, parsed: &mut ParsedIngredient) {
        // When tracing, emit a node for each pass that actually changed the
        // ingredient (a before→after view) so the egui tree shows what each pass
        // did. The clone is gated behind the tracing flag, so the hot path stays
        // allocation-free.
        if crate::trace::is_tracing_enabled() {
            for (name, pass) in POST_PASSES {
                let before = parsed.clone();
                pass(self, parsed);
                if *parsed != before {
                    crate::trace::trace_enter(name, &before.name);
                    crate::trace::trace_exit_success(
                        0,
                        &format!(
                            "{} | {}",
                            parsed.name,
                            parsed.modifier_string().as_deref().unwrap_or("-")
                        ),
                    );
                }
            }
        } else {
            for (_name, pass) in POST_PASSES {
                pass(self, parsed);
            }
        }
    }

    /// Collapse runs of whitespace left in the name by earlier passes. A pass in
    /// its own right so the ordered `POST_PASSES` list stays the single source of
    /// truth for the sequence.
    fn collapse_name(&self, parsed: &mut ParsedIngredient) {
        parsed.name = collapse_whitespace(&parsed.name);
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
    fn fix_leading_prep_phrase(&self, parsed: &mut ParsedIngredient) {
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
    fn fix_leading_minus_clause(&self, parsed: &mut ParsedIngredient) {
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

    /// Postfix produce count-units: "1 medium garlic clove" -> name "garlic",
    /// amount `{clove:1}`, with leading descriptors ("medium") moved to the
    /// modifier. Only fires for the curated [`vocab::POSTFIX_PRODUCE_UNITS`]
    /// pairs and only when the count is a plain whole number (or absent), so
    /// weights/volumes and idioms like "cinnamon stick" / "wood ear mushroom"
    /// are untouched.
    ///
    /// [`vocab::POSTFIX_PRODUCE_UNITS`]: crate::parser::vocab::POSTFIX_PRODUCE_UNITS
    fn extract_postfix_produce_unit(&self, parsed: &mut ParsedIngredient) {
        // The count must be a plain whole number (the default count unit) or
        // there must be no amount at all; a real volume/weight lead means the
        // trailing word isn't acting as the count unit.
        let whole_idx = parsed
            .amounts
            .iter()
            .position(|m| matches!(m.unit(), unit::Unit::Whole));
        if whole_idx.is_none() && !parsed.amounts.is_empty() {
            return;
        }

        let name_lower = parsed.name.to_lowercase();
        for (food, units) in crate::parser::vocab::POSTFIX_PRODUCE_UNITS {
            for unit_word in *units {
                let suffix = format!("{food} {unit_word}");
                if name_lower != suffix && !name_lower.ends_with(&format!(" {suffix}")) {
                    continue;
                }
                // `suffix` is ASCII produce, so lowercasing preserved byte
                // lengths and this offset is a valid char boundary in `name`.
                let food_start = parsed.name.len() - suffix.len();
                let count = whole_idx.map(|i| parsed.amounts[i].value()).unwrap_or(1.0);
                let measure = Measure::new(unit_word, count);
                match whole_idx {
                    Some(i) => parsed.amounts[i] = measure,
                    None => parsed.amounts.push(measure),
                }
                let prefix = parsed.name[..food_start].trim().to_string();
                parsed.name = (*food).to_string();
                if !prefix.is_empty() {
                    parsed.modifier.insert(0, ModifierPart::Prep(prefix));
                }
                return;
            }
        }
    }

    fn extract_adjectives_from_name(&self, parsed: &mut ParsedIngredient) {
        let mut name_lower = parsed.name.to_lowercase();
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
            // still after it is skipped. (Two ingredients on one line is really
            // a parse_multi concern — see the TODO in recognize.rs.)
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
            // `pos`/`end` are byte offsets into the lowercased name. Lowercasing
            // can change byte lengths for some Unicode (e.g. 'İ' -> "i̇"), so these
            // offsets may not fall on char boundaries in the original `name`.
            // Skip rather than panic when slicing `name` would split a char.
            if !name.is_char_boundary(pos) || !name.is_char_boundary(end) {
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

            // Fold a stranded intensifier adverb ("very") sitting immediately
            // before the adjective into the modifier too, and extend the cut so it
            // doesn't get left behind in the name ("very thinly sliced chives" ->
            // name "chives", modifier "very thinly sliced"). Only the word directly
            // abutting the adjective is consumed.
            let mut prep = adjective.clone();
            let mut cut = pos;
            if let Some(prev) = name_lower[..pos].split_whitespace().next_back()
                && crate::parser::vocab::INTENSIFIER_ADVERBS.contains(&prev)
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
    fn extract_trailing_prep_clause(&self, parsed: &mut ParsedIngredient) {
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

    /// Recover a head noun stranded behind a leading participle chain. The grammar
    /// carves the name at the first comma, so a line like "1/2 cup deribbed,
    /// seeded, and roughly chopped fresh hot green chiles, such as serrano" leaves
    /// name="deribbed" and the real ingredient ("fresh hot green chiles") buried in
    /// the `Raw` modifier. This is the mirror of [`Self::extract_trailing_prep_clause`]:
    /// it pulls the head noun *out of* the modifier *into* an all-participle name.
    ///
    /// Tightly guarded to avoid touching legitimate names:
    /// - the name must be a *pure* prep chain (every token a participle "-ed"/"-ly"
    ///   or an intensifier adverb) — any real noun in the name and it bails, so
    ///   "chopped onion" / "peeled and diced potatoes" are untouched;
    /// - the modifier's first part must be `Raw` and yield a head noun whose first
    ///   word is not a stopword, so a prose modifier ("then served over ice") bails.
    ///
    /// Runs after [`Self::fix_leading_prep_phrase`] (so the vocab-adjective case
    /// "chopped, toasted walnuts" is already resolved and never reaches here) and
    /// before `extract_adjectives_from_name` (so the recovered name still gets the
    /// normal adjective scan).
    fn recover_head_noun_from_modifier(&self, parsed: &mut ParsedIngredient) {
        // A "prep" token: a preparation participle ("-ed"), an "-ly" adverb
        // ("roughly"/"finely"), or a known intensifier. Deliberately NOT the broad
        // adjective set — a descriptive adjective like "fresh" must lead the head
        // noun, not be swallowed as prep.
        let is_prep = |w: &str| {
            let wl = w
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase();
            wl.ends_with("ed")
                || wl.ends_with("ly")
                || crate::parser::vocab::INTENSIFIER_ADVERBS.contains(&wl.as_str())
        };
        let is_connector = |w: &str| {
            let wl = w
                .trim_matches(|c: char| !c.is_alphanumeric())
                .to_lowercase();
            wl == "and" || wl == "&"
        };
        // Stopwords that, as the would-be head noun's first word, mean the modifier
        // is a prose clause, not "<preps> <head noun>".
        const STOPWORDS: &[&str] = &[
            "then", "to", "for", "with", "if", "until", "or", "such", "as", "plus", "about", "per",
            "from", "into", "over", "on", "in", "at", "the", "a", "an", "of",
        ];

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
        let head_start = modtext
            .split_whitespace()
            .map(|w| (w.as_ptr() as usize - modtext.as_ptr() as usize, w))
            .find(|(_, w)| !is_prep(w) && !is_connector(w))
            .map(|(off, _)| off);
        let Some(head_start) = head_start else {
            return; // modifier was all prep — nothing to recover.
        };

        let rest = &modtext[head_start..];
        let first_word = rest.split_whitespace().next().unwrap_or("");
        let first_lower = first_word
            .trim_matches(|c: char| !c.is_alphanumeric())
            .to_lowercase();
        if STOPWORDS.contains(&first_lower.as_str()) {
            return;
        }

        // The head noun runs to the next clause boundary.
        let mut end = rest.len();
        for pat in [", ", " such as ", " or ", " to taste"] {
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
    fn extract_purpose_gerund(&self, parsed: &mut ParsedIngredient) {
        use regex::Regex;
        use std::sync::LazyLock;

        // Match the first word-boundary " for " on the original string so the
        // byte offsets stay valid for slicing.
        static FOR_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
            #[allow(clippy::expect_used)]
            Regex::new(r"(?i)\s+for\s+").expect("invalid for-clause regex")
        });

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

    /// Recover a leading preparation *alternative* that displaced the name, e.g.
    /// "grated or finely chopped lemon zest" parses with "grated or finely
    /// chopped lemon zest" as the name. When the name begins with
    /// "`<participle> or <known-adjective>`" — a prep word (typically `-ed`),
    /// "or", then a recognized adjective phrase — that whole prefix is a
    /// preparation note. Move it to the modifier and keep the trailing head noun
    /// as the name ("lemon zest", modifier "grated or finely chopped").
    ///
    /// Guarded tightly so genuine two-ingredient alternatives ("basil or chopped
    /// parsley") are left alone: the first word must look like a participle
    /// (`-ed`) or be a known adjective, the word after "or" must be a known
    /// adjective phrase, and a head noun must remain.
    fn extract_leading_prep_alternative(&self, parsed: &mut ParsedIngredient) {
        let trimmed = parsed.name.trim();
        let words: Vec<&str> = trimmed.split_whitespace().collect();
        if words.len() < 4 {
            return;
        }
        // Every guard below matches tokens against lowercase vocab ("or", known
        // adjectives), so lowercase each token once up front instead of repeating
        // `words[i].to_lowercase()` per check.
        let words_lower: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
        if words_lower[1] != "or" {
            return;
        }
        let first = &words_lower[0];
        let first_is_prep = first.ends_with("ed") || self.adjectives.contains(first);
        if !first.chars().all(char::is_alphabetic) || !first_is_prep {
            return;
        }
        // A known adjective phrase (two words then one) immediately after "or".
        // Only build the two-word key when there's room for it — the common
        // short-name case never allocates the `format!`.
        let two_word_adj = words.len() >= 5
            && words_lower.get(3).is_some_and(|w3| {
                self.adjectives
                    .contains(&format!("{} {}", words_lower[2], w3))
            });
        let adj_len = if two_word_adj {
            2
        } else if self.adjectives.contains(&words_lower[2]) {
            1
        } else {
            return;
        };
        let name_start = 2 + adj_len;
        if name_start >= words.len() {
            return;
        }
        let prefix = words[..name_start].join(" ");
        let new_name = words[name_start..].join(" ");
        // `words` (borrowing parsed.name) is no longer read past this point.
        parsed.name = new_name;
        parsed.push_modifier(ModifierPart::Prep(prefix));
    }

    fn extract_alternative_from_name(&self, parsed: &mut ParsedIngredient) {
        let (name, alternative) = extract_alternative(&parsed.name);
        parsed.name = name;
        if let Some(alternative) = alternative {
            parsed.push_modifier(ModifierPart::Alternative(alternative));
        }
    }

    /// Split a no-quantity "X or Y" alternative left in the name into the
    /// modifier. The quantity form is already gone (handled by
    /// [`Self::extract_alternative_from_name`]), so any "or" remaining here is a
    /// plain ingredient/adjective alternative sharing the primary's amount.
    fn extract_word_alternative_from_name(&self, parsed: &mut ParsedIngredient) {
        let (name, alternative) = split_word_alternative(&parsed.name, &self.adjectives);
        parsed.name = name;
        if let Some(alternative) = alternative {
            parsed.push_modifier(ModifierPart::Alternative(alternative));
        }
    }

    fn extract_secondary_amounts_from_modifier(&self, parsed: &mut ParsedIngredient) {
        let Some(modifier) = parsed.modifier_string() else {
            return;
        };

        let (secondary_amounts, cleaned_modifier) =
            extract_secondary_amounts(&modifier, &self.units);
        // Only rewrite the modifier when an amount was actually hoisted; otherwise
        // leave the typed parts untouched (the cleaned string equals the original).
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
}

/// A single post-parse refinement pass: a named mutation of the parsed
/// ingredient. `&IngredientParser` carries the parse context (units, adjectives,
/// rich-text mode) each pass needs.
type Pass = fn(&IngredientParser, &mut ParsedIngredient);

/// The ordered refinement pipeline. The order is load-bearing — e.g. whitespace
/// is collapsed *between* adjective and alternative extraction. The modifier is
/// finalized when the IR is lowered to `Ingredient`. Adding or reordering a step
/// is a one-line edit here.
const POST_PASSES: &[(&str, Pass)] = &[
    (
        "fix_leading_prep_phrase",
        IngredientParser::fix_leading_prep_phrase,
    ),
    (
        "fix_leading_minus_clause",
        IngredientParser::fix_leading_minus_clause,
    ),
    (
        "extract_postfix_produce_unit",
        IngredientParser::extract_postfix_produce_unit,
    ),
    (
        "extract_leading_prep_alternative",
        IngredientParser::extract_leading_prep_alternative,
    ),
    (
        "extract_trailing_prep_clause",
        IngredientParser::extract_trailing_prep_clause,
    ),
    (
        "recover_head_noun_from_modifier",
        IngredientParser::recover_head_noun_from_modifier,
    ),
    (
        "extract_adjectives_from_name",
        IngredientParser::extract_adjectives_from_name,
    ),
    ("collapse_name", IngredientParser::collapse_name),
    (
        "extract_purpose_gerund",
        IngredientParser::extract_purpose_gerund,
    ),
    (
        "extract_alternative_from_name",
        IngredientParser::extract_alternative_from_name,
    ),
    (
        "extract_word_alternative_from_name",
        IngredientParser::extract_word_alternative_from_name,
    ),
    (
        "extract_secondary_amounts_from_modifier",
        IngredientParser::extract_secondary_amounts_from_modifier,
    ),
];

/// Strip a single pair of parentheses that wraps the *entire* modifier, e.g.
/// "(softened)" -> "softened". Modifiers with internal parentheses or only
/// partial wrapping are left untouched.
pub(super) fn strip_wrapping_parens(modifier: Option<String>) -> Option<String> {
    let modifier = modifier?;
    let trimmed = modifier.trim();
    if let Some(inner) = trimmed.strip_prefix('(').and_then(|s| s.strip_suffix(')'))
        && !inner.contains('(')
        && !inner.contains(')')
    {
        let inner = inner.trim();
        return (!inner.is_empty()).then(|| inner.to_string());
    }
    Some(modifier)
}

pub(super) fn clean_modifier(modifier: Option<String>) -> Option<String> {
    modifier.and_then(|modifier| {
        let trimmed = modifier.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

/// Extract alternative ingredients from the name (e.g., "garlic or 1 teaspoon garlic powder")
///
/// Returns `(cleaned_name, optional_alternative)` where:
/// - `cleaned_name`: The ingredient name with alternative removed
/// - `optional_alternative`: The alternative portion to be added to modifier
fn extract_alternative(name: &str) -> (String, Option<String>) {
    use regex::Regex;
    use std::sync::LazyLock;

    static ALTERNATIVE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
        let frac = crate::fraction::VULGAR_FRACTIONS;
        #[allow(clippy::expect_used)]
        Regex::new(&format!(r"(?i)\s+or\s+(\d+|[{frac}]|a\s+|an\s+)"))
            .expect("invalid alternative pattern regex")
    });

    let Some(matched) = ALTERNATIVE_PATTERN.find(name) else {
        return (name.to_string(), None);
    };

    let (ingredient_part, alternative_part) = name.split_at(matched.start());
    let alternative = alternative_part.trim();
    if alternative.is_empty() {
        return (name.to_string(), None);
    }

    (
        ingredient_part.trim().to_string(),
        Some(alternative.to_string()),
    )
}

/// Split a no-quantity "X or Y" alternative out of the name into the modifier,
/// e.g. "red or white onion" -> ("red onion", Some("or white onion")).
///
/// Returns `(primary_name, optional_alternative)`. The alternative keeps its
/// "or " prefix to match the existing quantity-alternative modifier style.
///
/// When the word before "or" is a single token and the part after "or" begins
/// with an adjective modifying a *shared head noun* ("red or **white onion**"),
/// the head noun is reconstructed onto the primary ("red onion"). Reconstruction
/// is gated to the cases a grammar can recognize without a food ontology; when
/// unsure it falls back to `primary = left` and still captures the alternative.
/// Known limitation: a single-token *noun* on the left with a distinct
/// multi-word alternative ("salt or chicken broth") over-reconstructs to "salt
/// broth" — rare, not in the corpus, and the alternative stays correct.
fn split_word_alternative(
    name: &str,
    adjectives: &std::collections::HashSet<String>,
) -> (String, Option<String>) {
    use regex::Regex;
    use std::sync::LazyLock;

    // First word-boundary " or ", case-insensitive. Matching on the original
    // `name` (not a lowercased copy) keeps the byte offsets valid for slicing.
    static OR_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
        #[allow(clippy::expect_used)]
        Regex::new(r"(?i)\s+or\s+").expect("invalid or-split regex")
    });

    let Some(m) = OR_PATTERN.find(name) else {
        return (name.to_string(), None);
    };
    let left = name[..m.start()].trim();
    let right = name[m.end()..].trim();
    if left.is_empty() || right.is_empty() {
        return (name.to_string(), None);
    }

    // Multiple coordinations ("raw or roasted and salted ...", "a or b or c")
    // are too ambiguous to split — keep the name whole.
    let right_lower = right.to_lowercase();
    if right_lower.contains(" and ") || right_lower.contains(" or ") {
        return (name.to_string(), None);
    }

    let left_tokens: Vec<&str> = left.split_whitespace().collect();
    let right_tokens: Vec<&str> = right.split_whitespace().collect();

    // A size-word OR size-word pair ("medium or large") is a size *range* of one
    // ingredient, never a two-ingredient alternative — leave the name whole.
    let is_size = |w: &str| crate::parser::vocab::SIZE_WORDS.contains(&w.to_lowercase().as_str());
    if left_tokens.len() == 1 && is_size(left) && is_size(right_tokens[0]) {
        return (name.to_string(), None);
    }

    // A possessive-brand left ("Hellmann's or Best Foods mayonnaise") sharing a
    // lowercase head noun on the right is one ingredient with two brand options,
    // not an "X or Y" alternative — keep the name whole. Deliberately narrow:
    // broader brand detection (capitalization, or "Best Foods or Hellmann's")
    // would over-fire on title-cased lines and strand real alternatives like
    // "Fresh or Frozen Blueberries".
    // Match a possessive "'s" with either a straight (') or curly (’) apostrophe.
    let left_has_possessive = left_tokens
        .iter()
        .any(|t| t.ends_with("'s") || t.ends_with("\u{2019}s"));
    let right_ends_lowercase = right_tokens
        .last()
        .and_then(|t| t.chars().next())
        .is_some_and(|c| c.is_ascii_lowercase());
    if left_has_possessive && right_ends_lowercase {
        return (name.to_string(), None);
    }

    // Stopwords/prepositions signal `right` is a noun + trailing phrase
    // ("pepper to taste"), not "adjective + shared head" ("white onion").
    const STOPWORDS: &[&str] = &[
        "to", "for", "with", "if", "such", "plus", "about", "as", "per", "from", "into", "over",
        "on", "in", "at", "the", "a", "an", "of",
    ];

    // Only an *adjective* left can share the right side's head noun ("fresh or
    // frozen blueberries" -> "fresh blueberries"). A complete-noun left absorbs
    // nothing ("amaretto or dark rum" stays "amaretto", not "amaretto rum").
    let left_lower = left.to_lowercase();
    let left_is_premodifier = crate::parser::vocab::SHARED_HEAD_MODIFIERS
        .contains(&left_lower.as_str())
        || adjectives.contains(&left_lower);

    let reconstruct = left_tokens.len() == 1
        && right_tokens.len() >= 2
        && left_is_premodifier
        && !adjectives.contains(&right_tokens[0].to_lowercase())
        && !right_tokens
            .iter()
            .any(|t| STOPWORDS.contains(&t.to_lowercase().as_str()));

    let primary = if reconstruct {
        // The single left adjective replaces `right`'s leading adjective, sharing
        // the trailing head noun: "red" + "white onion" -> "red onion".
        format!("{} {}", left, right_tokens[1..].join(" "))
    } else {
        left.to_string()
    };

    (primary, Some(format!("or {right}")))
}

/// Extract secondary amounts from modifier patterns like "(from about 15 sprigs)"
/// or a bare trailing measure parenthetical like "coarsely chopped (2.1 oz / 60g)".
///
/// Returns `(extracted_amounts, cleaned_modifier)` where:
/// - `extracted_amounts`: `Vec<Measure>` parsed from the pattern
/// - `cleaned_modifier`: The modifier with the pattern removed
fn extract_secondary_amounts(
    modifier: &str,
    units: &std::collections::HashSet<String>,
) -> (Vec<Measure>, String) {
    use regex::Regex;
    use std::sync::LazyLock;

    // An explicit approximation aside, anywhere in the modifier: "(about 2 cups)".
    static SECONDARY_AMOUNT_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
        #[allow(clippy::expect_used)]
        Regex::new(r"\((?:from\s+)?(?:about|approximately|roughly|around)\s+([^)]+)\)")
            .expect("invalid secondary amount regex")
    });
    // A bare trailing measure parenthetical: "coarsely chopped (2.1 oz / 60g)" —
    // a weight/volume equivalence stated for the prepped ingredient. Anchored to
    // the end and validated below (the inner text must fully parse as a
    // non-distance measurement), so non-measure asides like "(softened)" or
    // "(70% cacao)" fall through untouched.
    static TRAILING_MEASURE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
        #[allow(clippy::expect_used)]
        Regex::new(r"\(([^)]+)\)\s*$").expect("invalid trailing-measure regex")
    });

    // The approximation aside wins (it strips the "about" off the amount text);
    // otherwise fall back to a bare trailing measure parenthetical.
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

    // A *dimension* aside like "(about 3-inch)" inside a prep phrase ("cut into
    // long (about 3-inch) strips") describes shape, not a secondary quantity.
    // Leave it in the modifier rather than hoisting a spurious inch amount.
    let is_distance = |m: &Measure| match m.unit() {
        unit::Unit::Inch => true,
        unit::Unit::Other(s) => crate::parser::is_distance_unit(s),
        _ => false,
    };
    if measures.iter().any(is_distance) {
        return (vec![], modifier.to_string());
    }

    let remaining_trimmed = remaining.trim();
    let is_simple_remaining = remaining_trimmed.is_empty()
        || (remaining_trimmed.split_whitespace().count() == 1
            && remaining_trimmed.chars().all(char::is_alphabetic));

    if !is_simple_remaining || measures.is_empty() {
        return (vec![], modifier.to_string());
    }

    // Collapse, don't just trim: a mid-modifier match ("chopped (about 2 cups)
    // plus more") leaves the spaces on both sides of the excised parenthetical
    // adjacent, which trim() can't fix.
    let cleaned = super::normalize::collapse_whitespace(&format!(
        "{}{}",
        &modifier[..full_match.start()],
        &modifier[full_match.end()..]
    ));

    (measures, cleaned)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    // Fully wrapped: outer parens are stripped.
    #[case::simple("(sifted)", Some("sifted"))]
    #[case::with_percent("(70% cacao)", Some("70% cacao"))]
    #[case::inner_trimmed("(  softened  )", Some("softened"))]
    // Not wrapped, or only partially: left untouched.
    #[case::plain("softened", Some("softened"))]
    #[case::open_only("(partial", Some("(partial"))]
    #[case::close_only("partial)", Some("partial)"))]
    // Internal parens must NOT be collapsed (would merge distinct clauses).
    #[case::two_groups("(a) and (b)", Some("(a) and (b)"))]
    #[case::nested("(note (nested))", Some("(note (nested))"))]
    // An empty group collapses away entirely.
    #[case::empty("()", None)]
    fn test_strip_wrapping_parens(#[case] input: &str, #[case] expected: Option<&str>) {
        assert_eq!(
            strip_wrapping_parens(Some(input.to_string())),
            expected.map(str::to_string)
        );
    }

    #[test]
    fn test_strip_wrapping_parens_none() {
        assert_eq!(strip_wrapping_parens(None), None);
    }

    // ------------------------------------------------------------------
    // Per-pass guard tests. These exercise the subtle conditions in each
    // refine pass directly (previously only covered end-to-end by the
    // accuracy corpus), so a regression points at the exact pass.
    // ------------------------------------------------------------------

    fn ing(name: &str, modifier: Option<&str>) -> ParsedIngredient {
        ParsedIngredient {
            name: name.to_string(),
            amounts: vec![],
            modifier: modifier
                .map(|m| vec![ModifierPart::Raw(m.to_string())])
                .unwrap_or_default(),
            optional: false,
        }
    }

    /// A name that is exactly a known prep phrase swaps with the modifier; a
    /// descriptive name is left alone (the exact-match guard).
    #[rstest]
    #[case::swaps(
        "finely chopped",
        Some("raw pistachios"),
        "raw pistachios",
        Some("finely chopped")
    )]
    #[case::no_swap_descriptive(
        "raw pistachios",
        Some("finely chopped"),
        "raw pistachios",
        Some("finely chopped")
    )]
    #[case::no_swap_no_modifier("chopped", None, "chopped", None)]
    fn test_fix_leading_prep_phrase(
        #[case] name: &str,
        #[case] modifier: Option<&str>,
        #[case] want_name: &str,
        #[case] want_modifier: Option<&str>,
    ) {
        let parser = IngredientParser::new();
        let mut i = ing(name, modifier);
        parser.fix_leading_prep_phrase(&mut i);
        assert_eq!(i.name, want_name);
        assert_eq!(i.modifier_string().as_deref(), want_modifier);
    }

    /// "minus <measure> <name>" moves the subtractive clause to the modifier and
    /// restores the real name.
    #[test]
    fn test_fix_leading_minus_clause() {
        let parser = IngredientParser::new();
        let mut i = ing("minus 1 tablespoon flour", None);
        parser.fix_leading_minus_clause(&mut i);
        assert_eq!(i.name, "flour");
        assert_eq!(i.modifier_string().as_deref(), Some("minus 1 tablespoon"));
    }

    /// Adjectives are pulled from the name into the modifier, but only on word
    /// boundaries (so "well-chopped" is left intact).
    #[rstest]
    #[case::extracts("chopped onion", "onion", Some("chopped"))]
    #[case::boundary_guard("well-chopped onion", "well-chopped onion", None)]
    // Two adjectives in one name exercise the loop's name/name_lower rebuild.
    #[case::two_adjectives("chopped sifted flour", "flour", Some("chopped, sifted"))]
    // An adjective inside an "or" alternative is left for the alternative
    // passes ("chopped" describes parsley, not basil). One before "or" is
    // still extracted.
    #[case::after_or_left_alone("basil or chopped parsley", "basil or chopped parsley", None)]
    #[case::before_or_extracted("chopped basil or parsley", "basil or parsley", Some("chopped"))]
    // " and " guard: a mid-seam adjective belongs to the second conjunct and is
    // left in the name (it's really two ingredients — a parse_multi concern)…
    #[case::and_guard_keeps_conjunct(
        "Kosher salt and freshly ground black pepper",
        "Kosher salt and freshly ground black pepper",
        None
    )]
    // …but a TRAILING phrase after "and" (end-of-string) is still extracted.
    #[case::and_trailing_extracted("Salt and pepper to taste", "Salt and pepper", Some("to taste"))]
    // bare "grated" extracts; "fresh" (implied default) extracts…
    #[case::grated_extracts("grated lemon zest", "lemon zest", Some("grated"))]
    #[case::fresh_extracts("fresh mint", "mint", Some("fresh"))]
    // …except "fresh or frozen" — a genuine contrast — keeps "fresh" in the name.
    #[case::fresh_or_kept("fresh or frozen blueberries", "fresh or frozen blueberries", None)]
    fn test_extract_adjectives_from_name(
        #[case] name: &str,
        #[case] want_name: &str,
        #[case] want_modifier: Option<&str>,
    ) {
        let parser = IngredientParser::new();
        let mut i = ing(name, None);
        parser.extract_adjectives_from_name(&mut i);
        assert_eq!(i.name, want_name);
        assert_eq!(i.modifier_string().as_deref(), want_modifier);
    }

    /// A leading "<participle> or <adjective> <noun>" prep alternative moves to
    /// the modifier; a genuine two-ingredient alternative is left alone.
    #[rstest]
    #[case::prep_alt("grated or finely chopped lemon zest", "lemon zest", true)]
    #[case::genuine_alt("basil or chopped parsley", "basil or chopped parsley", false)]
    fn test_extract_leading_prep_alternative(
        #[case] name: &str,
        #[case] want_name: &str,
        #[case] moved: bool,
    ) {
        let parser = IngredientParser::new();
        let mut i = ing(name, None);
        parser.extract_leading_prep_alternative(&mut i);
        assert_eq!(i.name, want_name);
        assert_eq!(i.modifier_string().is_some(), moved, "name: {name}");
    }

    /// "(about N unit)" in the modifier hoists a secondary amount; a distance
    /// aside ("(about 3-inch)") is a shape descriptor and is left in place.
    #[rstest]
    #[case::hoists("chopped (about 2 cups)", 1)]
    #[case::distance_kept("cut into (about 3-inch) strips", 0)]
    // A bare trailing weight parenthetical hoists both measures (oz + g).
    #[case::trailing_weight("coarsely chopped (2.1 oz / 60g)", 2)]
    // A non-measure trailing parenthetical is left in place.
    #[case::non_measure("chopped (softened)", 0)]
    fn test_extract_secondary_amounts_from_modifier(
        #[case] modifier: &str,
        #[case] want_amounts: usize,
    ) {
        let parser = IngredientParser::new();
        let mut i = ing("scallions", Some(modifier));
        parser.extract_secondary_amounts_from_modifier(&mut i);
        assert_eq!(i.amounts.len(), want_amounts, "modifier: {modifier}");
    }

    /// A MID-modifier hoist must not leave a doubled internal space where the
    /// parenthetical was excised (trim only fixes the ends).
    #[test]
    fn test_extract_secondary_amounts_mid_modifier_whitespace() {
        let parser = IngredientParser::new();
        let mut i = ing(
            "parsley",
            Some("chopped (about 2 cups) plus more for garnish"),
        );
        parser.extract_secondary_amounts_from_modifier(&mut i);
        assert_eq!(i.amounts.len(), 1);
        assert_eq!(
            i.modifier_string().as_deref(),
            Some("chopped plus more for garnish")
        );
    }

    /// A no-quantity "X or Y" alternative is split out of the name, with the head
    /// noun reconstructed onto the primary when the left side is a lone adjective.
    #[rstest]
    // Lone adjective before "or": head noun shared onto the primary.
    #[case::shared_head("red or white onion", "red onion", Some("or white onion"))]
    #[case::shared_multiword_head(
        "fresh or frozen pitted sweet cherries",
        "fresh pitted sweet cherries",
        Some("or frozen pitted sweet cherries")
    )]
    // Distinct nouns (single- or multi-word left): primary = left, no reconstruct.
    #[case::distinct_noun("flour or cornmeal", "flour", Some("or cornmeal"))]
    #[case::multiword_left(
        "Nilla wafers or graham crackers",
        "Nilla wafers",
        Some("or graham crackers")
    )]
    // Guards: multi-coordination, prep adjective after "or", trailing stopword.
    #[case::and_guard(
        "raw or roasted and salted shelled sunflower seeds",
        "raw or roasted and salted shelled sunflower seeds",
        None
    )]
    #[case::prep_adj_after_or("basil or chopped parsley", "basil", Some("or chopped parsley"))]
    #[case::stopword_after_or("salt or pepper to taste", "salt", Some("or pepper to taste"))]
    #[case::no_or("onion", "onion", None)]
    // A size-word OR size-word pair is a size range of one ingredient, not a
    // two-ingredient alternative — leave the name whole.
    #[case::size_range("medium or large garlic clove", "medium or large garlic clove", None)]
    fn test_split_word_alternative(
        #[case] name: &str,
        #[case] want_name: &str,
        #[case] want_alternative: Option<&str>,
    ) {
        let parser = IngredientParser::new();
        let (got_name, got_alternative) = split_word_alternative(name, &parser.adjectives);
        assert_eq!(got_name, want_name, "name: {name}");
        assert_eq!(got_alternative.as_deref(), want_alternative, "name: {name}");
    }

    /// The IR exposes a typed view of the modifier: extracted adjectives land in
    /// `prep`, alternatives in `alternatives` — not a single opaque string.
    #[test]
    fn test_typed_modifier_view() {
        let parser = IngredientParser::new();

        let mut i = ing("chopped onion", None);
        parser.extract_adjectives_from_name(&mut i);
        assert_eq!(i.prep(), vec!["chopped"]);
        assert!(i.alternatives().is_empty());

        let mut i = ing("garlic or 1 teaspoon garlic powder", None);
        parser.extract_alternative_from_name(&mut i);
        assert_eq!(i.alternatives(), vec!["or 1 teaspoon garlic powder"]);
        assert!(i.prep().is_empty());
        // And it still flattens to the same modifier string.
        assert_eq!(
            Ingredient::from(i).modifier.as_deref(),
            Some("or 1 teaspoon garlic powder")
        );
    }

    /// Postfix produce units: the trailing count noun becomes the unit and the
    /// food becomes the name; leading descriptors move to the modifier. Idioms
    /// (food not on the allowlist) and non-count leads are left untouched.
    #[test]
    fn test_extract_postfix_produce_unit() {
        let parser = IngredientParser::new();

        let mut i = ParsedIngredient {
            name: "medium garlic clove".into(),
            amounts: vec![Measure::new("whole", 1.0)],
            modifier: vec![],
            optional: false,
        };
        parser.extract_postfix_produce_unit(&mut i);
        assert_eq!(i.name, "garlic");
        assert_eq!(i.amounts, vec![Measure::new("clove", 1.0)]);
        assert_eq!(i.modifier_string().as_deref(), Some("medium"));

        // Idiom guard: cinnamon isn't a produce food, so "cinnamon stick" stays.
        let mut i = ParsedIngredient {
            name: "cinnamon stick".into(),
            amounts: vec![Measure::new("whole", 1.0)],
            modifier: vec![],
            optional: false,
        };
        parser.extract_postfix_produce_unit(&mut i);
        assert_eq!(i.name, "cinnamon stick");
        assert_eq!(i.amounts, vec![Measure::new("whole", 1.0)]);

        // A real volume/weight lead (not a plain count) → don't fire.
        let mut i = ParsedIngredient {
            name: "garlic clove".into(),
            amounts: vec![Measure::new("cup", 1.0)],
            modifier: vec![],
            optional: false,
        };
        parser.extract_postfix_produce_unit(&mut i);
        assert_eq!(i.name, "garlic clove");
    }

    /// A trailing "for `<gerund>` …" clause (object included) moves to the
    /// modifier; a plain "<name> for <noun>" is left intact.
    #[rstest]
    #[case::gerund(
        "Extra-virgin olive oil for brushing the bread",
        "Extra-virgin olive oil",
        Some("for brushing the bread")
    )]
    #[case::non_gerund("flour for bread", "flour for bread", None)]
    fn test_extract_purpose_gerund(
        #[case] name: &str,
        #[case] want_name: &str,
        #[case] want_modifier: Option<&str>,
    ) {
        let parser = IngredientParser::new();
        let mut i = ing(name, None);
        parser.extract_purpose_gerund(&mut i);
        assert_eq!(i.name, want_name);
        assert_eq!(i.modifier_string().as_deref(), want_modifier);
    }

    /// The ordered `POST_PASSES` pipeline must be idempotent: running it a second
    /// time on its own output must change nothing. This is the invariant the
    /// load-bearing pass order depends on — a pass that isn't a fixpoint (e.g. it
    /// re-extracts an adjective it already moved, or re-splits an alternative)
    /// would silently corrupt results when a later edit reorders the list. This
    /// test fails the moment that happens, naming the offending line.
    #[rstest]
    #[case::leading_adjective("1 onion, finely chopped")]
    #[case::name_adjective("1 cup packed brown sugar, sifted")]
    #[case::word_alternative("red or white onion")]
    #[case::quantity_alternative("1 clove garlic or 1 teaspoon garlic powder")]
    #[case::secondary_amount("1 stick butter (8 tablespoons)")]
    #[case::leading_prep_phrase("grated zest of 1 lemon")]
    #[case::plain_name("kosher salt")]
    #[case::postfix_produce("1 medium or large garlic clove, peeled")]
    #[case::purpose_gerund("Extra-virgin olive oil for brushing the bread")]
    #[case::fresh_extracted("fresh mint")]
    #[case::and_guard("Kosher salt and freshly ground black pepper")]
    fn refine_pipeline_is_idempotent(#[case] line: &str) {
        let parser = IngredientParser::new();
        let (_, parsed) = parser.parse_ingredient(line).unwrap();

        let mut once = parsed.clone();
        parser.refine(&mut once);
        let mut twice = once.clone();
        parser.refine(&mut twice);

        assert_eq!(once, twice, "refine is not idempotent for {line:?}");
    }
}
