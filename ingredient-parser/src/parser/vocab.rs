//! Parser vocabulary — the static word lists the parser recognizes, gathered in
//! one place. Previously these slices were scattered across `lib.rs`,
//! `measurement/composite.rs`, and `measurement/guards.rs`; centralizing them
//! makes "what words does the parser know" a single, scannable answer. Each list
//! is consumed where it was before (seeding the parser's `HashSet`s, or via
//! `.contains` checks); only the data's home moved.

/// Preparation adjectives that get extracted from the name into the modifier.
/// These describe how an ingredient is prepared before use. Multi-word forms
/// (e.g. "firmly packed") win over their single-word substring ("packed") via
/// the longest-match-first ordering in `refine::extract_adjectives_from_name`.
pub(crate) const DEFAULT_PREPARATION_ADJECTIVES: &[&str] = &[
    "chopped",
    "minced",
    "diced",
    "freshly ground",
    "freshly grated",
    "freshly squeezed",
    "finely grated",
    "finely chopped",
    "coarsely chopped",
    "roughly chopped",
    "thinly sliced",
    "sliced",
    // Bare participle: "grated lemon zest" -> "lemon zest" / "grated". The
    // multiword "freshly grated"/"finely grated" win via longest-match-first.
    "grated",
    // "freshly squeezed lime juice" -> "lime juice" / "freshly squeezed". The
    // multiword "freshly squeezed" wins over this bare participle via longest-match.
    "squeezed",
    // "shredded zucchini" -> "zucchini" / "shredded", "shredded cheddar cheese"
    // -> "cheddar cheese" / "shredded". Multiword forms win via longest-match.
    "finely shredded",
    "coarsely shredded",
    "shredded",
    // "fresh" is the *implied default* state of herbs/produce/juice — "fresh
    // cilantro" is just cilantro. The marked forms ("dried"/"frozen") are named
    // explicitly and stay in the name. So extract "fresh" to the modifier.
    // Guarded in `extract_adjectives_from_name` against "fresh or frozen …",
    // where it is a genuine contrast, not an implied default.
    "fresh",
    "plain",
    // Quantity-is-unmeasured qualifiers (no fixed amount): kept here so they are
    // stripped from the name into the modifier, like "to taste".
    "to taste",
    "as needed",
    // State/prep words that describe how an ingredient is brought to the recipe
    // (e.g. "melted butter", "softened butter").
    "melted",
    "softened",
    // Measurement/preparation qualifiers that often appear *before* the name
    // (e.g. "1 cup packed brown sugar", "2 cups sifted flour").
    "firmly packed",
    "loosely packed",
    "lightly packed",
    "packed",
    "sifted",
    // Temperature/state qualifier (e.g. "room-temperature butter"). Both
    // spellings reduce to the same modifier.
    "room temperature",
    "room-temperature",
];

/// Purpose phrases that get extracted into the modifier (e.g. "for garnish").
/// These describe what the ingredient is used for.
pub(crate) const DEFAULT_PURPOSE_PHRASES: &[&str] = &[
    "for dusting",
    "for garnish",
    "for garnishing",
    "for serving",
    "for decoration",
    "for topping",
    "for the topping",
    // "for the pan" also lives in PAN_GREASE_PHRASES for usage classification;
    // listed here too so the extractor strips it from the name (the two tables
    // deliberately overlap — see the doc comment below).
    "for the pan",
    "for dipping",
    "for drizzling",
    "for sprinkling",
    "for rolling",
    "for coating",
    "for frying",
    "for greasing",
];

/// Usage-classification phrase tables (see `crate::usage`). Each maps a set of
/// purpose phrases to one [`IngredientUsage`](crate::usage::IngredientUsage)
/// role. They deliberately overlap `DEFAULT_PURPOSE_PHRASES` above — the
/// extractor moves these phrases into the modifier, the classifier reads them
/// back out — so keep the two in sync when adding a phrase.
///
/// All entries must be phrase-anchored ("for frying"), never bare verbs
/// ("fry"/"fried"): "refried beans", "stir-fry sauce", and "dried Thai chiles,
/// fried" are all Normal ingredients.
pub(crate) const GARNISH_PHRASES: &[&str] = &[
    "for garnish",
    "for garnishing",
    "for decoration",
    "for decorating",
];

pub(crate) const FRYING_PHRASES: &[&str] = &[
    "for frying",
    "for deep-frying",
    "for deep frying",
    "for pan-frying",
    "for pan frying",
    "for shallow frying",
    "for the fryer",
];

pub(crate) const PAN_GREASE_PHRASES: &[&str] =
    &["for greasing", "for the pan", "for buttering", "for oiling"];

pub(crate) const DREDGING_PHRASES: &[&str] = &["for dredging", "for dusting", "for coating"];

pub(crate) const SEASONING_PHRASES: &[&str] = &["to taste", "for seasoning"];

pub(crate) const MARINADE_PHRASES: &[&str] = &["for the marinade", "for marinating"];

/// Section-name words that mark every ingredient in the section as marinade
/// components (matched word-anchored against the section title, e.g.
/// "For the marinade", "Brine").
pub(crate) const MARINADE_SECTION_WORDS: &[&str] = &["marinade", "marinating", "brine"];

/// Non-standard units that aren't really convertible, seeded into the parser's
/// unit set. Note: "whole" is deliberately NOT included — it's the built-in
/// `Unit::Whole`, and listing it here would parse "whole wheat flour" as having
/// unit "whole" instead of keeping "whole wheat" in the name.
/// `is_addon_unit` is an exact-match lookup with no plural normalization, so
/// every plural form must be listed explicitly alongside its singular.
pub(crate) const NON_STANDARD_UNITS: &[&str] = &[
    "recipe", "recipes", "packet", "packets", "sticks", "stick", "cloves", "clove", "bunch",
    "bunches", "head", "heads", "pinch", "pinches", "package", "packages", "slice", "slices",
    "standard", "can", "cans", "leaf", "leaves", "strand", "strands", "tin", "tins", "rib", "ribs",
    "sprig", "sprigs", "pint", "pints", "piece", "pieces", "disk", "disks", "stalk", "stalks",
    "loaf", "loaves", "ear", "ears", "handful", "handfuls", "dash", "dashes",
];

/// Informal/imprecise measures where a leading SIZE word ("small handful", "large
/// pinch") describes the *measure*, not the food, so it is discarded like the
/// shape qualifiers ("generous"/"heaping"). Every entry MUST also be a recognized
/// unit (see [`NON_STANDARD_UNITS`]) — the gate only fires when the unit parser
/// will accept the following word, otherwise the size word would be dropped with
/// no unit to show for it. Consumed by `single::amount_qualifier_between` (along
/// with [`SIZE_QUALIFIABLE_UNITS`]). "can" stays excluded so "1 small can
/// tomatoes" keeps its size word.
pub(crate) const VAGUE_UNITS: &[&str] =
    &["pinch", "pinches", "handful", "handfuls", "dash", "dashes"];

/// Bunch/head produce measures where a leading SIZE word ("large bunch", "small
/// head") describes the *bunch/head*, not the produce variety, so it is discarded
/// as a measure qualifier exactly like [`VAGUE_UNITS`] ("1 large bunch kale" -> 1
/// bunch kale). Kept separate from `VAGUE_UNITS` because these are countable
/// containers, not imprecise measures. Every entry MUST also be in
/// [`NON_STANDARD_UNITS`]. Consumed by `single::amount_qualifier_between`.
pub(crate) const SIZE_QUALIFIABLE_UNITS: &[&str] = &["bunch", "bunches", "head", "heads"];

/// Curated `<food>` -> allowed trailing count units for the POSTFIX produce form
/// ("1 garlic clove" = `{clove:1} garlic`, not `{whole:1} "garlic clove"`). The
/// food allowlist is deliberately narrow: it is what keeps idioms where the
/// trailing word is part of the *name* — "cinnamon stick", "wood ear mushroom",
/// "short rib" — from being mis-parsed (cinnamon/wood/short aren't foods here).
/// Consumed by `refine::extract_postfix_produce_unit`. Add a produce row to
/// extend; a general postfix rule is intentionally avoided.
pub(crate) const POSTFIX_PRODUCE_UNITS: &[(&str, &[&str])] = &[
    ("garlic", &["clove", "cloves", "head", "heads"]),
    ("celery", &["stalk", "stalks", "rib", "ribs"]),
    ("corn", &["ear", "ears"]),
    ("lettuce", &["head", "heads"]),
    ("cabbage", &["head", "heads"]),
];

/// Size descriptors. A "size-word OR size-word" pair ("medium or large") is a
/// range of one ingredient, never a two-ingredient alternative, so
/// `refine::split_word_alternative` must not split/reconstruct it.
pub(crate) const SIZE_WORDS: &[&str] = &["small", "medium", "large", "jumbo", "baby"];

/// Size descriptors consumed as the *count unit* for an explicitly-counted produce
/// item ("3 medium carrots" -> `{medium:3}` carrots), so the size maps to USDA
/// portion data via the unit graph. Consumed by `refine::extract_size_unit_from_name`.
///
/// A separate list from [`SIZE_WORDS`] on purpose: "baby" is excluded because it
/// reads as a *variety* ("baby spinach"/"baby kale"/"baby corn"), not a portion
/// size; "extra large" / "extra-large" (a USDA egg grade) is added. Multi-word
/// forms are matched longest-first by the pass; both spellings normalize to the
/// canonical unit string "extra large".
pub(crate) const SIZE_UNIT_WORDS: &[&str] = &[
    "extra large",
    "extra-large",
    "small",
    "medium",
    "large",
    "jumbo",
];

/// Premodifier words used to gate the "A or B C" alternative reconstruction in
/// `refine::split_word_alternative`. Only when the left side is one of these — a
/// word that commonly *premodifies* a head noun, i.e. a descriptor adjective or
/// an attributive noun — is the right side's head grafted on: "fresh or frozen
/// blueberries" -> "fresh blueberries", "lemon or orange zest" -> "lemon zest".
/// A complete *ingredient* noun on the left ("amaretto or dark rum", "walnuts or
/// macadamia nuts") is whole on its own and must NOT absorb the alternative's
/// head noun, so it stays "amaretto" / "walnuts" with the rest in the modifier.
///
/// A heuristic allowlist by necessity: "lemon" and "amaretto" are both nouns, so
/// only world knowledge separates "lemon zest" (good) from "amaretto rum" (bad).
/// Missing a premodifier just leaves the bare left as the name (mildly wrong);
/// wrongly including an ingredient noun would graft nonsense — so bias the list
/// toward true modifiers and common attributive nouns, not standalone foods.
pub(crate) const SHARED_HEAD_MODIFIERS: &[&str] = &[
    // state / preparation
    "fresh",
    "frozen",
    "dried",
    "raw",
    "roasted",
    "toasted",
    "cooked",
    "melted",
    "softened",
    "salted",
    "unsalted",
    "smoked",
    "pickled",
    "canned",
    "cured",
    "shelled",
    // ripeness / texture
    "ripe",
    "firm",
    "soft",
    "smooth",
    "crunchy",
    "fine",
    "coarse",
    "ground",
    "whole",
    // color
    "red",
    "white",
    "green",
    "yellow",
    "black",
    "brown",
    "golden",
    "purple",
    "dark",
    "light",
    // flavor / heat
    "sweet",
    "hot",
    "mild",
    "spicy",
    "bitter",
    "sour",
    "savory",
    "bittersweet",
    "semisweet",
    // processing / grade
    "instant",
    "rapid",
    "quick",
    "bleached",
    "unbleached",
    "refined",
    "virgin",
    "fancy",
    // fat
    "skim",
    "nonfat",
    "lean",
    // size words (small/medium/large/jumbo/baby): see SIZE_WORDS, folded in by
    // is_shared_head_modifier so the size vocabulary has a single source of truth.
    // common attributive nouns that premodify a shared head ("lemon zest")
    "lemon",
    "lime",
    "orange",
    "grapefruit",
];

/// Whether `word` can premodify a shared head noun in the "A or B C" alternative
/// reconstruction (`refine::split_word_alternative`). Folds [`SIZE_WORDS`] into
/// [`SHARED_HEAD_MODIFIERS`] so size words live in exactly one place; both are tiny
/// slices, so the two linear scans cost the same as the previous single `.contains`.
pub(crate) fn is_shared_head_modifier(word: &str) -> bool {
    SHARED_HEAD_MODIFIERS.contains(&word) || SIZE_WORDS.contains(&word)
}

/// Head nouns that an "X, Y, or Z <noun>" alternatives list can share, where the
/// noun appears only after the final alternative — "canola, vegetable, or melted
/// coconut oil" is three kinds of *oil*. The grammar splits the list on the first
/// comma, stranding the head noun ("oil") off the end of the modifier; the
/// `recover_shared_head_from_alternatives` refine pass grafts it back onto the
/// first alternative ("canola" -> "canola oil").
///
/// Deliberately tiny: only nouns where the bare-modifier-list construction is
/// idiomatic. "salt, pepper, or paprika" and "flour, sugar, or baking soda" are
/// lists of *complete* ingredients, not premodifiers of a shared head — including
/// their last word here would graft nonsense ("salt paprika"), so keep this to
/// nouns that genuinely read as "<type> <noun>".
pub(crate) const SHARED_HEAD_NOUNS: &[&str] = &["oil", "vinegar", "broth", "stock"];

/// Head nouns that an inline "A or B <noun>" alternative distributes onto the
/// primary: "chicken or vegetable stock" -> "chicken stock" (+ "or vegetable
/// stock" modifier). Unlike the [`SHARED_HEAD_MODIFIERS`] path (which gates on the
/// *left* being a known adjective), this gates on the *trailing head noun* — so an
/// open-ended left ("chicken", "grainy", "Little Gem") still distributes when the
/// noun reads as "<type> <noun>". Consumed by `refine::split_word_alternative`.
///
/// Deliberately excludes `oil`/`vinegar` and spirits: in "butter or olive oil" /
/// "amaretto or dark rum" the left is a *distinct* ingredient, not a type of the
/// head, so grafting ("butter oil") would be nonsense — those keep `name = left`.
/// Curate toward nouns that essentially always carry a variety/type premodifier.
pub(crate) const DISTRIBUTABLE_HEAD_NOUNS: &[&str] =
    &["stock", "broth", "mustard", "pepper", "lettuce", "cabbage"];

/// Intensifier adverbs that precede a preparation phrase ("very thinly sliced").
/// They carry no ingredient meaning on their own, so when one is stranded
/// immediately before an extracted prep adjective it is folded into the modifier
/// too — otherwise "very thinly sliced chives" leaves "very chives" as the name.
/// See `refine::extract_adjectives_from_name`.
pub(crate) const INTENSIFIER_ADVERBS: &[&str] = &["very", "really"];

/// Manner adverbs that precede a preparation adjective ("diagonally sliced",
/// "roughly diced"). Like [`INTENSIFIER_ADVERBS`] they carry no ingredient meaning
/// alone, so when one is stranded immediately before an extracted prep adjective it
/// is folded into the modifier too — otherwise "diagonally sliced scallions" leaves
/// "diagonally scallions" as the name. The common multiword forms ("thinly sliced",
/// "roughly chopped") are already whole entries in [`DEFAULT_PREPARATION_ADJECTIVES`]
/// and win via longest-match; this catches the bare-participle combinations that
/// aren't. See `refine::extract_adjectives_from_name`.
pub(crate) const MANNER_ADVERBS: &[&str] = &[
    "diagonally",
    "lengthwise",
    "crosswise",
    "thinly",
    "thickly",
    "roughly",
    "finely",
    "coarsely",
];

/// Container nouns that can follow a parenthesized size, e.g. the "piece" in
/// "1 (1-ounce) piece ginger". Kept narrow so the size-hoisting parser doesn't
/// over-match arbitrary parentheticals.
pub(crate) const CONTAINER_NOUNS: &[&str] = &[
    "piece",
    "pieces",
    "can",
    "cans",
    "knob",
    "knobs",
    "package",
    "packages",
    "packet",
    "packets",
    "bottle",
    "bottles",
    "jar",
    "jars",
    "block",
    "blocks",
    "bunch",
    "bunches",
    "head",
    "heads",
    "stick",
    "sticks",
    "fillet",
    "fillets",
    "loaf",
    "loaves",
    "slab",
    "slabs",
    "chunk",
    "chunks",
    "ball",
    "balls",
    "box",
    "boxes",
    "disk",
    "disks",
    "wedge",
    "wedges",
    "tube",
    "tubes",
    "envelope",
    "envelopes",
];

/// Distance unit base forms for dimension detection (see
/// `measurement::guards::is_distance_unit`, which also handles plurals).
pub(crate) const DISTANCE_UNIT_BASES: &[&str] = &[
    "inch",
    "in",
    "cm",
    "centimeter",
    "centimetre",
    "mm",
    "millimeter",
    "millimetre",
    "foot",
    "ft",
    "meter",
    "metre",
    "m",
    "yard",
    "yd",
];

#[cfg(test)]
mod tests {
    use super::*;

    /// `subset` ⊆ `superset`, reporting the offending entries when not.
    fn assert_subset(subset: &[&str], subset_name: &str, superset: &[&str], superset_name: &str) {
        let missing: Vec<&str> = subset
            .iter()
            .filter(|w| !superset.contains(*w))
            .copied()
            .collect();
        assert!(
            missing.is_empty(),
            "{subset_name} entries missing from {superset_name}: {missing:?}"
        );
    }

    // The size-qualifier gate in `single::amount_qualifier_between` only fires when
    // the unit parser will accept the following word, so every vague/size-qualifiable
    // measure MUST also be a recognized unit — otherwise the size word is dropped with
    // no unit to show for it. (vocab.rs doc comments on VAGUE_UNITS / SIZE_QUALIFIABLE_UNITS.)
    #[test]
    fn vague_and_size_qualifiable_units_are_recognized_units() {
        assert_subset(
            VAGUE_UNITS,
            "VAGUE_UNITS",
            NON_STANDARD_UNITS,
            "NON_STANDARD_UNITS",
        );
        assert_subset(
            SIZE_QUALIFIABLE_UNITS,
            "SIZE_QUALIFIABLE_UNITS",
            NON_STANDARD_UNITS,
            "NON_STANDARD_UNITS",
        );
    }

    // "small or large onion" needs the left size word recognized as a premodifier to
    // graft the shared head. After the Part 2 de-dup this holds by construction via
    // `is_shared_head_modifier`; the test pins both the containment and that helper.
    #[test]
    fn size_words_are_shared_head_modifiers() {
        for &w in SIZE_WORDS {
            assert!(
                is_shared_head_modifier(w),
                "size word {w:?} is not recognized as a shared-head modifier"
            );
        }
    }

    // The postfix-produce parse ("1 garlic clove" -> {clove:1} garlic) only works if
    // each trailing count unit also parses as a unit. (vocab.rs doc on POSTFIX_PRODUCE_UNITS.)
    #[test]
    fn postfix_produce_units_are_recognized_units() {
        for (food, units) in POSTFIX_PRODUCE_UNITS {
            for unit in *units {
                assert!(
                    NON_STANDARD_UNITS.contains(unit),
                    "POSTFIX_PRODUCE_UNITS[{food:?}] unit {unit:?} missing from NON_STANDARD_UNITS"
                );
            }
        }
    }

    // The extractor moves "for the pan" into the modifier; the classifier reads it back
    // out as PanGrease. The two tables deliberately overlap on this phrase — pin it so
    // neither side drops it silently. (vocab.rs doc on the usage phrase tables.)
    #[test]
    fn for_the_pan_is_in_both_purpose_and_pan_grease() {
        assert!(DEFAULT_PURPOSE_PHRASES.contains(&"for the pan"));
        assert!(PAN_GREASE_PHRASES.contains(&"for the pan"));
    }

    // Hygiene: every membership list is duplicate-free and lowercase. Consumers
    // lowercase input before matching, so an upper-cased entry would be dead code.
    #[test]
    fn lists_are_dup_free_and_lowercase() {
        let lists: &[(&str, &[&str])] = &[
            (
                "DEFAULT_PREPARATION_ADJECTIVES",
                DEFAULT_PREPARATION_ADJECTIVES,
            ),
            ("DEFAULT_PURPOSE_PHRASES", DEFAULT_PURPOSE_PHRASES),
            ("GARNISH_PHRASES", GARNISH_PHRASES),
            ("FRYING_PHRASES", FRYING_PHRASES),
            ("PAN_GREASE_PHRASES", PAN_GREASE_PHRASES),
            ("DREDGING_PHRASES", DREDGING_PHRASES),
            ("SEASONING_PHRASES", SEASONING_PHRASES),
            ("MARINADE_PHRASES", MARINADE_PHRASES),
            ("MARINADE_SECTION_WORDS", MARINADE_SECTION_WORDS),
            ("NON_STANDARD_UNITS", NON_STANDARD_UNITS),
            ("VAGUE_UNITS", VAGUE_UNITS),
            ("SIZE_QUALIFIABLE_UNITS", SIZE_QUALIFIABLE_UNITS),
            ("SIZE_WORDS", SIZE_WORDS),
            ("SHARED_HEAD_MODIFIERS", SHARED_HEAD_MODIFIERS),
            ("SHARED_HEAD_NOUNS", SHARED_HEAD_NOUNS),
            ("DISTRIBUTABLE_HEAD_NOUNS", DISTRIBUTABLE_HEAD_NOUNS),
            ("INTENSIFIER_ADVERBS", INTENSIFIER_ADVERBS),
            ("MANNER_ADVERBS", MANNER_ADVERBS),
            ("CONTAINER_NOUNS", CONTAINER_NOUNS),
            ("DISTANCE_UNIT_BASES", DISTANCE_UNIT_BASES),
        ];
        for (name, list) in lists {
            let mut seen = std::collections::HashSet::new();
            for &entry in *list {
                assert!(
                    entry == entry.to_lowercase(),
                    "{name} entry {entry:?} is not lowercase"
                );
                assert!(seen.insert(entry), "{name} has duplicate entry {entry:?}");
            }
        }
    }
}
