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
        let first_is_prep = crate::parser::token::is_participle(first, &self.adjectives);
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

    /// Pull every "X or Y" / "X and/or Y" alternative out of the name into the
    /// modifier. Runs three phases in a fixed, load-bearing order (each shares
    /// the alternatives vocab/guards), merged into one refine pass so the trace
    /// and pipeline see a single `extract_alternatives_from_name` step:
    ///
    /// 1. quantity form — "garlic or 1 teaspoon garlic powder"
    ///    ([`Self::extract_alternative_from_name`]);
    /// 2. no-quantity "X or Y" — "red or white onion"
    ///    ([`Self::extract_word_alternative_from_name`]); the quantity form must
    ///    be gone first so the leftover "or" is a plain alternative;
    /// 3. inclusive "X and/or Y" — "thyme and/or rosemary"
    ///    ([`Self::extract_and_or_alternative_from_name`]).
    pub(super) fn extract_alternatives_from_name(&self, parsed: &mut ParsedIngredient) {
        self.extract_alternative_from_name(parsed);
        self.extract_word_alternative_from_name(parsed);
        self.extract_and_or_alternative_from_name(parsed);
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
        let Some(head) = distributable_head(&modifier, SharedHeadContext::CommaOrList) else {
            return;
        };
        parsed.name = graft(name_word, head, GraftMode::AppendTrailingHead);
        parsed.modifier = vec![ModifierPart::Alternative(format!("or {modifier}"))];
    }
}

/// Which coordination shape asked "does `right`'s trailing noun distribute onto
/// the left". Each variant keeps its own EXACT gates in [`distributable_head`];
/// the two shapes intentionally consult different vocab lists.
enum SharedHeadContext<'a> {
    /// A comma+or alternatives list stranded in the modifier
    /// ("vegetable, or melted coconut oil"), gated on
    /// [`vocab::SHARED_HEAD_NOUNS`].
    CommaOrList,
    /// An inline "A or B `<head>`" right side ("white onion"), gated on
    /// [`vocab::DISTRIBUTABLE_HEAD_NOUNS`]. Carries the parser's adjective set
    /// for the "not led by a prep adjective" guard.
    InlineOr {
        adjectives: &'a std::collections::HashSet<String>,
    },
}

/// Decide whether `right`'s trailing noun is a *shared head* that can be grafted
/// onto the left conjunct, returning that head token (source casing preserved)
/// when so. Each context applies its own gates:
///
/// - [`SharedHeadContext::CommaOrList`]: `right` must read as a comma-separated
///   alternatives list joined by "or" (both signals a shared head, not a
///   standalone alternative), and its final token must be in
///   [`vocab::SHARED_HEAD_NOUNS`]. Casing note: the last token is trimmed of
///   surrounding punctuation but *not* lowercased for the graft — only the vocab
///   lookup lowercases — so the grafted head preserves the source casing.
/// - [`SharedHeadContext::InlineOr`]: `right` must read as
///   "`<premodifier> <head noun>`" (at least two tokens, not led by a prep
///   adjective, free of stopwords) and its trailing head noun must be in
///   [`vocab::DISTRIBUTABLE_HEAD_NOUNS`].
fn distributable_head<'a>(right: &'a str, ctx: SharedHeadContext) -> Option<&'a str> {
    match ctx {
        SharedHeadContext::CommaOrList => {
            // The modifier must read as a comma-separated alternatives list
            // joined by "or" — both signals that the trailing noun is a shared
            // head, not a standalone alternative ("flour or oil" stays two
            // ingredients).
            if !right.contains(',') || !right.to_lowercase().contains(" or ") {
                return None;
            }
            // Its final token must be a curated shared head noun the bare
            // alternatives can all premodify ("oil"), so grafting it produces a
            // real ingredient. Trim (but don't lowercase) the last token: the
            // gate lowercases for the vocab lookup, while the graft preserves the
            // source casing.
            let head = right
                .split_whitespace()
                .next_back()
                .map(|t| t.trim_matches(|c: char| !c.is_alphanumeric()))?;
            crate::parser::vocab::SHARED_HEAD_NOUNS
                .contains(&head.to_lowercase().as_str())
                .then_some(head)
        }
        SharedHeadContext::InlineOr { adjectives } => {
            let right_tokens: Vec<&str> = right.split_whitespace().collect();
            if !right_is_modifier_plus_head(&right_tokens, adjectives) {
                return None;
            }
            // The right's *trailing head noun* must be one that essentially
            // always carries a variety/type premodifier, so an open-ended (even
            // multi-word) left distributes onto it: "chicken or vegetable stock"
            // -> "chicken stock", "Little Gem or Bibb lettuce" -> "Little Gem
            // lettuce".
            let head_noun = right_tokens.last().copied()?;
            crate::parser::vocab::DISTRIBUTABLE_HEAD_NOUNS
                .contains(&head_noun.to_lowercase().as_str())
                .then_some(head_noun)
        }
    }
}

/// The right side must read as "`<premodifier> <head noun>`": at least two
/// tokens, not led by a prep adjective ("basil or chopped parsley" keeps
/// "chopped" with parsley), and free of stopwords ("pepper to taste" isn't a
/// shared head). Shared by both [`SharedHeadContext::InlineOr`] reconstruction
/// paths in [`split_word_alternative`].
fn right_is_modifier_plus_head(
    right_tokens: &[&str],
    adjectives: &std::collections::HashSet<String>,
) -> bool {
    right_tokens.len() >= 2
        && !adjectives.contains(&right_tokens[0].to_lowercase())
        && !right_tokens
            .iter()
            .any(|t| crate::parser::vocab::MODIFIER_STOPWORDS.contains(&t.to_lowercase().as_str()))
}

/// How a shared head grafts onto the left conjunct.
enum GraftMode {
    /// The single left adjective *replaces* `right`'s leading adjective, sharing
    /// the trailing head noun: "red" + "white onion" -> "red onion". `head` is
    /// the right side *after* its leading adjective (e.g. "onion").
    ReplaceLeadingAdjective,
    /// Append the trailing head noun onto the (possibly multi-word) left: "canola"
    /// + "oil" -> "canola oil".
    AppendTrailingHead,
}

/// Graft a shared `head` onto `left`. Both modes just join with a space — the
/// distinction is which slice of the right side the caller passes as `head`
/// (see [`GraftMode`]).
fn graft(left: &str, head: &str, mode: GraftMode) -> String {
    match mode {
        GraftMode::ReplaceLeadingAdjective | GraftMode::AppendTrailingHead => {
            format!("{left} {head}")
        }
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

    let left_tokens: Vec<&str> = left.split_whitespace().collect();
    let right_tokens: Vec<&str> = right.split_whitespace().collect();

    if keep_whole(left, &left_tokens, right, &right_tokens) {
        return (name.to_string(), None);
    }

    let primary = match graft_decision(left, &left_tokens, right, &right_tokens, adjectives) {
        // A single left adjective replaces `right`'s leading adjective, keeping
        // the trailing head noun: "red or white onion" -> "red onion".
        Graft::ReplaceLeadingAdjective => graft(
            left,
            &right_tokens[1..].join(" "),
            GraftMode::ReplaceLeadingAdjective,
        ),
        // An open-ended left distributes onto the right's trailing head noun:
        // "chicken or vegetable stock" -> "chicken stock".
        Graft::AppendTrailingHead(head) => graft(left, head, GraftMode::AppendTrailingHead),
        Graft::None => left.to_string(),
    };

    (primary, Some(format!("or {right}")))
}

/// The graft outcome for a no-quantity "X or Y" split: how (if at all) the
/// right's shared head noun folds back onto the left conjunct. `None` keeps the
/// primary as the bare left and still captures the alternative.
enum Graft<'a> {
    /// Path A — the single left premodifier replaces `right`'s leading adjective.
    ReplaceLeadingAdjective,
    /// Path B — append `right`'s trailing head noun (carried here) onto the left.
    AppendTrailingHead(&'a str),
    /// No reconstruction: `primary = left`.
    None,
}

/// Named early-outs that keep the "X or Y" name whole (no split at all). Each is
/// a distinct reason the "or" is *not* a two-ingredient alternative:
/// - multi-coordination: a second " and "/" or " on the right is too ambiguous
///   ("raw or roasted and salted …", "a or b or c");
/// - size range: "medium or large" is one ingredient's size range, not two
///   ingredients (both sides single [`vocab::SIZE_WORDS`]);
/// - possessive/brand: "Hellmann's or Best Foods mayonnaise" is one ingredient
///   with two brand options (a possessive left sharing a lowercase head on the
///   right). Deliberately narrow: broader brand detection would over-fire on
///   title-cased lines and strand real alternatives like "Fresh or Frozen
///   Blueberries".
fn keep_whole(left: &str, left_tokens: &[&str], right: &str, right_tokens: &[&str]) -> bool {
    // Multiple coordinations ("raw or roasted and salted ...", "a or b or c").
    let right_lower = right.to_lowercase();
    if right_lower.contains(" and ") || right_lower.contains(" or ") {
        return true;
    }

    // A size-word OR size-word pair ("medium or large") is a size *range*.
    let is_size = |w: &str| crate::parser::vocab::SIZE_WORDS.contains(&w.to_lowercase().as_str());
    if left_tokens.len() == 1 && is_size(left) && is_size(right_tokens[0]) {
        return true;
    }

    // A possessive-brand left sharing a lowercase head noun on the right.
    // Match a possessive "'s" with either a straight (') or curly (’) apostrophe.
    let left_has_possessive = left_tokens
        .iter()
        .any(|t| t.ends_with("'s") || t.ends_with("\u{2019}s"));
    let right_ends_lowercase = right_tokens
        .last()
        .and_then(|t| t.chars().next())
        .is_some_and(|c| c.is_ascii_lowercase());
    left_has_possessive && right_ends_lowercase
}

/// Decide how the right's shared head grafts onto the left, having already
/// cleared [`keep_whole`]. Path A is tried first: a single known-premodifier left
/// ("red") replacing the right's leading adjective. Path B is the fallback: the
/// right's trailing head noun is one that always carries a variety/type
/// premodifier ([`vocab::DISTRIBUTABLE_HEAD_NOUNS`], via [`distributable_head`]),
/// so an open-ended (even multi-word) left distributes onto it without a
/// left-vocab match. Both paths share the [`right_is_modifier_plus_head`] guard.
fn graft_decision<'a>(
    left: &str,
    left_tokens: &[&str],
    right: &'a str,
    right_tokens: &[&str],
    adjectives: &std::collections::HashSet<String>,
) -> Graft<'a> {
    // Stopwords/prepositions signal `right` is a noun + trailing phrase
    // ("pepper to taste"), not "adjective + shared head" ("white onion").
    let left_lower = left.to_lowercase();
    let left_is_premodifier = crate::parser::vocab::is_shared_head_modifier(&left_lower)
        || adjectives.contains(&left_lower);

    if left_tokens.len() == 1
        && left_is_premodifier
        && right_is_modifier_plus_head(right_tokens, adjectives)
    {
        return Graft::ReplaceLeadingAdjective;
    }

    match distributable_head(right, SharedHeadContext::InlineOr { adjectives }) {
        Some(head) => Graft::AppendTrailingHead(head),
        None => Graft::None,
    }
}

#[cfg(test)]
mod helper_tests {
    //! Direct coverage for the extracted shared-head decision module. The
    //! end-to-end behavior is pinned by `refine/tests.rs` and the accuracy
    //! corpus; these rows exercise the two contexts' gates and both graft modes
    //! in isolation.
    use super::*;
    use rstest::rstest;

    /// `CommaOrList`: fires on a comma+or list ending in a `SHARED_HEAD_NOUNS`
    /// word, preserving the source casing of the grafted head. The gates
    /// (comma AND " or " AND curated final noun) each reject when absent.
    #[rstest]
    #[case::fires("vegetable, or melted coconut oil", Some("oil"))]
    // Casing preserved: the vocab lookup lowercases, the returned head does not.
    #[case::casing_preserved("vegetable, or Coconut Oil", Some("Oil"))]
    #[case::no_comma("or oil", None)]
    #[case::no_or("vegetable, coconut oil", None)]
    #[case::final_not_curated("sugar, or baking soda", None)]
    fn test_distributable_head_comma_or_list(#[case] right: &str, #[case] expected: Option<&str>) {
        assert_eq!(
            distributable_head(right, SharedHeadContext::CommaOrList),
            expected,
            "right: {right}"
        );
    }

    /// `InlineOr`: fires when the right is "`<premodifier> <head noun>`" whose
    /// trailing noun is in `DISTRIBUTABLE_HEAD_NOUNS`. The guards reject a
    /// single-token right, a prep-adjective-led right, a stopword-bearing right,
    /// and a trailing noun off the curated list.
    #[rstest]
    #[case::fires("vegetable stock", Some("stock"))]
    #[case::single_token("stock", None)]
    #[case::prep_adj_led("chopped parsley", None)]
    #[case::stopword("pepper to taste", None)]
    #[case::not_distributable("olive oil", None)]
    fn test_distributable_head_inline_or(#[case] right: &str, #[case] expected: Option<&str>) {
        let adjectives = IngredientParser::new().adjectives;
        assert_eq!(
            distributable_head(
                right,
                SharedHeadContext::InlineOr {
                    adjectives: &adjectives
                }
            ),
            expected,
            "right: {right}"
        );
    }

    /// Both graft modes join left + head with a single space; the caller picks
    /// which slice of the right side is the head.
    #[rstest]
    #[case::append("canola", "oil", GraftMode::AppendTrailingHead, "canola oil")]
    #[case::replace("red", "onion", GraftMode::ReplaceLeadingAdjective, "red onion")]
    fn test_graft(
        #[case] left: &str,
        #[case] head: &str,
        #[case] mode: GraftMode,
        #[case] expected: &str,
    ) {
        assert_eq!(graft(left, head, mode), expected);
    }

    /// `keep_whole`: each early-out reason keeps the name whole; a genuine "X or
    /// Y" alternative ("red or white onion") passes through to reconstruction.
    #[rstest]
    #[case::multi_or("a", "b or c", true)]
    #[case::multi_and("raw", "roasted and salted", true)]
    #[case::size_range("medium", "large", true)]
    #[case::possessive_brand("Hellmann's", "Best Foods mayonnaise", true)]
    // A possessive left whose right ends title-cased is a real alternative.
    #[case::possessive_titlecase("Fresh", "Frozen Blueberries", false)]
    #[case::genuine_alternative("red", "white onion", false)]
    fn test_keep_whole(#[case] left: &str, #[case] right: &str, #[case] expected: bool) {
        let left_tokens: Vec<&str> = left.split_whitespace().collect();
        let right_tokens: Vec<&str> = right.split_whitespace().collect();
        assert_eq!(
            keep_whole(left, &left_tokens, right, &right_tokens),
            expected,
            "{left} or {right}"
        );
    }

    /// `graft_decision`: Path A (single known-premodifier left) → replace; Path B
    /// (distributable trailing head) → append; otherwise → None.
    #[rstest]
    #[case::replace("red", "white onion", "replace")]
    #[case::append("chicken", "vegetable stock", "append:stock")]
    // A non-distributable trailing head with a non-premodifier left grafts nothing.
    #[case::none_non_distributable("butter", "olive oil", "none")]
    #[case::none_single_right("red", "white", "none")]
    fn test_graft_decision(#[case] left: &str, #[case] right: &str, #[case] expected: &str) {
        let adjectives = IngredientParser::new().adjectives;
        let left_tokens: Vec<&str> = left.split_whitespace().collect();
        let right_tokens: Vec<&str> = right.split_whitespace().collect();
        let tag = match graft_decision(left, &left_tokens, right, &right_tokens, &adjectives) {
            Graft::ReplaceLeadingAdjective => "replace".to_string(),
            Graft::AppendTrailingHead(head) => format!("append:{head}"),
            Graft::None => "none".to_string(),
        };
        assert_eq!(tag, expected, "{left} or {right}");
    }
}
