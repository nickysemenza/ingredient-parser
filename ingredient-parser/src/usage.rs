//! Ingredient usage classification — what *role* a line plays in a recipe.
//!
//! Most ingredient lines describe food that is measured and eaten as written,
//! but some describe a *medium* or *flourish* whose consumed amount differs
//! from the line: oil the food is fried in, salt added "to taste", butter for
//! the pan, flour for dusting, a marinade that is mostly discarded. Downstream
//! consumers (recipe costing/nutrition) use the role to decide how much of the
//! ingredient is actually consumed; this module only answers the language
//! question of which role the text declares.
//!
//! Classification is phrase-anchored ("for frying"), never bare-verb
//! ("fry"/"fried"), so names that merely *contain* a cooking word — "refried
//! beans", "stir-fry sauce", "dried Thai chiles, fried" — stay [`Normal`].
//! Accuracy is ratcheted by the corpus (`tests/corpus/corpus.jsonl`), which
//! labels a `usage` expectation per row.
//!
//! [`Normal`]: IngredientUsage::Normal

use serde::{Deserialize, Serialize};

use crate::parser::vocab::{
    DREDGING_PHRASES, FRYING_PHRASES, GARNISH_PHRASES, MARINADE_PHRASES, MARINADE_SECTION_WORDS,
    PAN_GREASE_PHRASES, SEASONING_PHRASES,
};

/// The role an ingredient line plays in a recipe, as declared by its text.
///
/// Serialized in `snake_case` (`"frying_medium"`, …). The field is required on
/// [`Ingredient`](crate::Ingredient) with no serde default — every parse
/// produces a value, and absence in serialized data means the producer is
/// stale, which should fail loudly rather than quietly read as `Normal`.
#[derive(
    Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum IngredientUsage {
    /// An ordinary ingredient: measured and consumed as written.
    #[default]
    Normal,
    /// The fat the food is cooked in ("oil, for frying") — mostly not consumed;
    /// only the absorbed fraction is eaten.
    FryingMedium,
    /// Fat for greasing the cooking vessel ("butter, for the pan").
    PanGrease,
    /// Unmeasured seasoning ("salt, to taste").
    Seasoning,
    /// Dry coating that only partially adheres ("flour, for dredging").
    Dredging,
    /// Decorative finish ("parsley, for garnish").
    Garnish,
    /// Marinade/brine component, mostly discarded after use. Usually signaled
    /// by the *section* name ("For the marinade"), not the line.
    Marinade,
}

/// Rules in precedence order: the first phrase hit wins. Garnish outranks
/// frying so "crispy fried shallots, for garnish" classifies by its declared
/// purpose, not the cooking method embedded in the name.
const RULES: &[(&[&str], IngredientUsage)] = &[
    (GARNISH_PHRASES, IngredientUsage::Garnish),
    (FRYING_PHRASES, IngredientUsage::FryingMedium),
    (PAN_GREASE_PHRASES, IngredientUsage::PanGrease),
    (DREDGING_PHRASES, IngredientUsage::Dredging),
    (SEASONING_PHRASES, IngredientUsage::Seasoning),
    (MARINADE_PHRASES, IngredientUsage::Marinade),
];

/// Classify an ingredient line's usage from its parsed parts.
///
/// Searches the modifier first (where the parser's purpose-phrase extraction
/// puts "for frying" etc.), then the raw line (covers phrases the extractor
/// missed or rows that never went through the parser), then the name (covers
/// fallback parses where the whole line became the name). `section_name` is
/// consulted last, only for marinade/brine sections.
///
/// ```
/// use ingredient::usage::{classify_usage, IngredientUsage};
///
/// assert_eq!(
///     classify_usage("vegetable oil", Some("for frying"), None, None),
///     IngredientUsage::FryingMedium
/// );
/// // Surplus mentions describe the extra, not the row: still Normal.
/// assert_eq!(
///     classify_usage("flour", Some("plus more for dusting"), None, None),
///     IngredientUsage::Normal
/// );
/// assert_eq!(
///     classify_usage("soy sauce", None, None, Some("For the marinade")),
///     IngredientUsage::Marinade
/// );
/// ```
pub fn classify_usage(
    name: &str,
    modifier: Option<&str>,
    raw_line: Option<&str>,
    section_name: Option<&str>,
) -> IngredientUsage {
    let haystacks: Vec<String> = [modifier, raw_line, Some(name)]
        .into_iter()
        .flatten()
        .map(str::to_lowercase)
        .collect();

    for (phrases, usage) in RULES {
        for hay in &haystacks {
            for phrase in *phrases {
                if let Some(pos) = find_phrase(hay, phrase) {
                    // "plus more for dusting" / "plus 20 or so for garnish"
                    // describe surplus beyond the measured amount — the row's
                    // own role stays Normal. Only "for …" phrases can be
                    // surplus-qualified; "or more to taste" is still Seasoning.
                    if phrase.starts_with("for ") && is_surplus_mention(hay, pos) {
                        continue;
                    }
                    return *usage;
                }
            }
        }
    }

    if let Some(section) = section_name {
        let section = section.to_lowercase();
        if MARINADE_SECTION_WORDS
            .iter()
            .any(|w| find_phrase(&section, w).is_some())
        {
            return IngredientUsage::Marinade;
        }
    }

    IngredientUsage::Normal
}

/// Find `phrase` in `haystack` at word boundaries (both already lowercase).
/// Boundary = start/end of string or a non-alphanumeric char on each side, so
/// "brine" does not match inside "brined" and "for frying" does not match
/// inside a longer word run.
fn find_phrase(haystack: &str, phrase: &str) -> Option<usize> {
    let mut search_from = 0;
    while let Some(rel) = haystack[search_from..].find(phrase) {
        let pos = search_from + rel;
        let before_ok = pos == 0
            || !haystack[..pos]
                .chars()
                .next_back()
                .is_some_and(char::is_alphanumeric);
        let end = pos + phrase.len();
        let after_ok = end == haystack.len()
            || !haystack[end..]
                .chars()
                .next()
                .is_some_and(char::is_alphanumeric);
        if before_ok && after_ok {
            return Some(pos);
        }
        search_from = pos + 1;
    }
    None
}

/// True when the phrase at `pos` is preceded by a surplus marker: the word
/// directly before is "more"/"extra", or "plus" appears within the four
/// preceding words ("plus 2 tablespoons for the pan").
fn is_surplus_mention(haystack: &str, pos: usize) -> bool {
    // Last up-to-four alphanumeric words before `pos`, nearest first — `rsplit`
    // walks back from `pos` so we never scan the whole preceding text.
    let recent: Vec<&str> = haystack[..pos]
        .rsplit(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .take(4)
        .collect();
    matches!(recent.first(), Some(&"more") | Some(&"extra")) || recent.contains(&"plus")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    // Each variant, signal in the modifier (the common parsed shape).
    #[case(
        "vegetable oil",
        Some("for frying"),
        None,
        None,
        IngredientUsage::FryingMedium
    )]
    #[case(
        "peanut oil",
        Some("for deep-frying"),
        None,
        None,
        IngredientUsage::FryingMedium
    )]
    #[case("butter", Some("for greasing"), None, None, IngredientUsage::PanGrease)]
    #[case("butter", Some("for the pan"), None, None, IngredientUsage::PanGrease)]
    #[case("salt", Some("to taste"), None, None, IngredientUsage::Seasoning)]
    #[case(
        "Salt and pepper",
        Some("to taste"),
        None,
        None,
        IngredientUsage::Seasoning
    )]
    #[case(
        "cayenne pepper",
        Some("or more to taste"),
        None,
        None,
        IngredientUsage::Seasoning
    )]
    #[case("flour", Some("for dredging"), None, None, IngredientUsage::Dredging)]
    #[case(
        "Confectioners' sugar",
        Some("for dusting"),
        None,
        None,
        IngredientUsage::Dredging
    )]
    #[case(
        "Fresh parsley",
        Some("for garnish"),
        None,
        None,
        IngredientUsage::Garnish
    )]
    #[case(
        "soy sauce",
        Some("for the marinade"),
        None,
        None,
        IngredientUsage::Marinade
    )]
    // Section-name marinade/brine: a perfectly normal line, classified by where
    // it sits. Word-anchored: "Marinated Chicken" section title still hits the
    // "marinating" word only when present as a word.
    #[case("soy sauce", None, None, Some("Marinade"), IngredientUsage::Marinade)]
    #[case(
        "soy sauce",
        None,
        None,
        Some("For the marinade"),
        IngredientUsage::Marinade
    )]
    #[case("kosher salt", None, None, Some("Brine"), IngredientUsage::Marinade)]
    #[case("chicken thighs", None, None, Some("Chicken"), IngredientUsage::Normal)]
    // raw_line-only signal (manual rows that never parsed a modifier).
    #[case(
        "oil",
        None,
        Some("oil for frying"),
        None,
        IngredientUsage::FryingMedium
    )]
    // Fallback parses: the whole line ended up in the name.
    #[case("oil for frying", None, None, None, IngredientUsage::FryingMedium)]
    // Precedence: declared purpose beats cooking-method words in the name.
    #[case(
        "crispy fried shallots",
        Some("for garnish"),
        None,
        None,
        IngredientUsage::Garnish
    )]
    // Phrase anchoring: bare "fry"/"fried" never classifies.
    #[case(
        "refried beans",
        None,
        Some("1 can refried beans"),
        None,
        IngredientUsage::Normal
    )]
    #[case(
        "stir-fry sauce",
        None,
        Some("2 tbsp stir-fry sauce"),
        None,
        IngredientUsage::Normal
    )]
    #[case(
        "dried Thai chiles",
        Some("fried"),
        None,
        None,
        IngredientUsage::Normal
    )]
    #[case(
        "fried onions",
        None,
        Some("1 cup fried onions"),
        None,
        IngredientUsage::Normal
    )]
    // Surplus guard: the for-phrase describes the extra, not the row.
    #[case(
        "all-purpose flour",
        Some("plus more for dusting"),
        None,
        None,
        IngredientUsage::Normal
    )]
    #[case(
        "fresh cranberries",
        Some("plus 20 or so for garnish"),
        None,
        None,
        IngredientUsage::Normal
    )]
    #[case(
        "parsley",
        Some("chopped plus more for garnish"),
        None,
        None,
        IngredientUsage::Normal
    )]
    #[case(
        "butter",
        Some("plus extra for greasing"),
        None,
        None,
        IngredientUsage::Normal
    )]
    // ...but a non-surplus phrase elsewhere in the line still wins.
    #[case(
        "flour",
        Some("for dusting"),
        Some("flour, for dusting"),
        None,
        IngredientUsage::Dredging
    )]
    #[case("nothing special", None, None, None, IngredientUsage::Normal)]
    fn classify(
        #[case] name: &str,
        #[case] modifier: Option<&str>,
        #[case] raw_line: Option<&str>,
        #[case] section: Option<&str>,
        #[case] expected: IngredientUsage,
    ) {
        assert_eq!(
            classify_usage(name, modifier, raw_line, section),
            expected,
            "name={name:?} modifier={modifier:?} raw_line={raw_line:?} section={section:?}"
        );
    }

    #[test]
    fn serde_snake_case_round_trip() {
        let json = serde_json::to_string(&IngredientUsage::FryingMedium).unwrap();
        assert_eq!(json, "\"frying_medium\"");
        let back: IngredientUsage = serde_json::from_str(&json).unwrap();
        assert_eq!(back, IngredientUsage::FryingMedium);
    }
}
