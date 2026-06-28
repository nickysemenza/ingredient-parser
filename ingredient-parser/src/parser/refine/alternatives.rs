use super::*;

impl IngredientParser {
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
    pub(super) fn extract_leading_prep_alternative(&self, parsed: &mut ParsedIngredient) {
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

    fn apply_alternative_split(parsed: &mut ParsedIngredient, split: (String, Option<String>)) {
        let (name, alternative) = split;
        parsed.name = name;
        if let Some(alternative) = alternative {
            parsed.push_modifier(ModifierPart::Alternative(alternative));
        }
    }

    pub(super) fn extract_alternative_from_name(&self, parsed: &mut ParsedIngredient) {
        Self::apply_alternative_split(parsed, extract_alternative(&parsed.name));
    }

    /// Split a no-quantity "X or Y" alternative left in the name into the
    /// modifier. The quantity form is already gone (handled by
    /// [`Self::extract_alternative_from_name`]), so any "or" remaining here is a
    /// plain ingredient/adjective alternative sharing the primary's amount.
    pub(super) fn extract_word_alternative_from_name(&self, parsed: &mut ParsedIngredient) {
        Self::apply_alternative_split(
            parsed,
            split_word_alternative(&parsed.name, &self.adjectives),
        );
    }

    /// Split an inclusive "X and/or Y" coordination out of the name into the
    /// modifier. The slash is part of the ingredient text grammar so the whole
    /// phrase survives parsing; this pass then keeps the primary ingredient in
    /// `name` and preserves the author's "and/or" wording in the modifier.
    pub(super) fn extract_and_or_alternative_from_name(&self, parsed: &mut ParsedIngredient) {
        crate::lazy_regex!(AND_OR_PATTERN, r"(?i)\s+and/or\s+");

        let Some(m) = AND_OR_PATTERN.find(&parsed.name) else {
            return;
        };
        let left = parsed.name[..m.start()].trim().to_string();
        let right = parsed.name[m.end()..].trim().to_string();
        if left.is_empty() || right.is_empty() {
            return;
        }

        parsed.name = left;
        let alternative = ModifierPart::Alternative(format!("and/or {right}"));
        let insert_at = parsed
            .modifier
            .iter()
            .position(|part| matches!(part, ModifierPart::Raw(_)))
            .unwrap_or(parsed.modifier.len());
        parsed.modifier.insert(insert_at, alternative);
    }

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
    pub(super) fn recover_shared_head_from_alternatives(&self, parsed: &mut ParsedIngredient) {
        // Name must be a single bare token that isn't already the head noun.
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
        // The modifier must read as a comma-separated alternatives list joined by
        // "or" — both signals that the trailing noun is a shared head, not a
        // standalone alternative ("flour or oil" stays two ingredients).
        if !modifier.contains(',') || !modifier.to_lowercase().contains(" or ") {
            return;
        }
        // Its final token must be a curated shared head noun the bare alternatives
        // can all premodify ("oil"), so grafting it produces a real ingredient.
        let Some(head) = modifier
            .split_whitespace()
            .next_back()
            .map(|t| t.trim_matches(|c: char| !c.is_alphanumeric()))
        else {
            return;
        };
        if !crate::parser::vocab::SHARED_HEAD_NOUNS.contains(&head.to_lowercase().as_str()) {
            return;
        }
        parsed.name = format!("{name_word} {head}");
        parsed.modifier = vec![ModifierPart::Alternative(format!("or {modifier}"))];
    }
}

/// Extract alternative ingredients from the name (e.g., "garlic or 1 teaspoon garlic powder")
///
/// Returns `(cleaned_name, optional_alternative)` where:
/// - `cleaned_name`: The ingredient name with alternative removed
/// - `optional_alternative`: The alternative portion to be added to modifier
fn extract_alternative(name: &str) -> (String, Option<String>) {
    crate::lazy_regex!(ALTERNATIVE_PATTERN, {
        use regex::Regex;
        let frac = crate::fraction::VULGAR_FRACTIONS;
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
/// the head noun is reconstructed onto the primary ("red onion"). A second path
/// gates on the trailing head noun itself ([`vocab::DISTRIBUTABLE_HEAD_NOUNS`]) so
/// an open-ended left distributes too ("chicken or vegetable **stock**" -> "chicken
/// stock"). Reconstruction is gated to the cases a grammar can recognize without a
/// food ontology; when unsure it falls back to `primary = left` and still captures
/// the alternative.
/// Known limitation: a single-token *noun* on the left with a distinct
/// multi-word alternative ("salt or chicken broth") over-reconstructs to "salt
/// broth" — rare, not in the corpus, and the alternative stays correct.
pub(super) fn split_word_alternative(
    name: &str,
    adjectives: &std::collections::HashSet<String>,
) -> (String, Option<String>) {
    // First word-boundary " or ", case-insensitive. Matching on the original
    // `name` (not a lowercased copy) keeps the byte offsets valid for slicing.
    crate::lazy_regex!(OR_PATTERN, r"(?i)\s+or\s+");

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
    let left_lower = left.to_lowercase();
    let left_is_premodifier = crate::parser::vocab::is_shared_head_modifier(&left_lower)
        || adjectives.contains(&left_lower);

    // Shared by both reconstruction paths: the right side must read as
    // "<premodifier> <head noun>" — at least two tokens, not led by a prep
    // adjective ("basil or chopped parsley" keeps "chopped" with parsley), and
    // free of stopwords ("pepper to taste" isn't a shared head).
    let right_is_modifier_plus_head = right_tokens.len() >= 2
        && !adjectives.contains(&right_tokens[0].to_lowercase())
        && !right_tokens
            .iter()
            .any(|t| crate::parser::vocab::MODIFIER_STOPWORDS.contains(&t.to_lowercase().as_str()));

    // Path A — a single known-premodifier left shares the right's head noun:
    // "red or white onion" -> "red onion".
    let reconstruct = left_tokens.len() == 1 && left_is_premodifier && right_is_modifier_plus_head;

    // Path B — the right's *trailing head noun* is one that essentially always
    // carries a variety/type premodifier, so an open-ended (even multi-word) left
    // distributes onto it without needing a left-vocab match: "chicken or
    // vegetable stock" -> "chicken stock", "Little Gem or Bibb lettuce" ->
    // "Little Gem lettuce".
    let right_head_noun = right_tokens.last().copied().unwrap_or_default();
    let head_noun_distribute = right_is_modifier_plus_head
        && crate::parser::vocab::DISTRIBUTABLE_HEAD_NOUNS
            .contains(&right_head_noun.to_lowercase().as_str());

    let primary = if reconstruct {
        // The single left adjective replaces `right`'s leading adjective, sharing
        // the trailing head noun: "red" + "white onion" -> "red onion".
        format!("{} {}", left, right_tokens[1..].join(" "))
    } else if head_noun_distribute {
        // Graft just the trailing head noun onto the (possibly multi-word) left;
        // the alternative's own premodifier stays in the "or …" modifier.
        format!("{left} {right_head_noun}")
    } else {
        left.to_string()
    };

    (primary, Some(format!("or {right}")))
}
