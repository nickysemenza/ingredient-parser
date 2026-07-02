//! Parser vocabulary — the static word lists the parser recognizes, gathered in
//! one place. Previously these slices were scattered across `lib.rs`,
//! `measurement/composite.rs`, and `measurement/guards.rs`; centralizing them
//! makes "what words does the parser know" a single, scannable answer. Each list
//! is consumed where it was before (seeding the parser's `HashSet`s, or via
//! `.contains` checks); only the data's home moved.

/// Spelled-out count tokens recognized as leading quantities ("one" … "twelve",
/// "a"/"an"). Consumed by `normalize::is_count_token`.
pub(crate) const SPELLED_COUNTS: &[&str] = &[
    "a", "an", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
    "eleven", "twelve",
];

/// Spelled-out number words parsed as amounts. Order matches `helpers::text_number`
/// precedence (longest/most-specific first). Articles "a"/"an" are handled separately
/// there (they require a trailing space).
pub(crate) const NUMBER_WORDS: &[(&str, f64)] = &[
    ("twelve", 12.0),
    ("eleven", 11.0),
    ("ten", 10.0),
    ("nine", 9.0),
    ("eight", 8.0),
    ("seven", 7.0),
    ("six", 6.0),
    ("five", 5.0),
    ("four", 4.0),
    ("three", 3.0),
    ("two", 2.0),
    ("one", 1.0),
    ("dozen", 12.0),
    ("half", 0.5),
];

/// Stopwords that signal a modifier clause is prose, not a shared head noun. Union
/// of the lists used in `refine::recover` and `refine::alternatives`.
pub(crate) const MODIFIER_STOPWORDS: &[&str] = &[
    "then", "to", "for", "with", "if", "until", "or", "such", "as", "plus", "about", "per", "from",
    "into", "over", "on", "in", "at", "the", "a", "an", "of",
];

/// Preparation adjectives that get extracted from the name into the modifier.
/// These describe how an ingredient is prepared before use. Multi-word forms
/// (e.g. "firmly packed") win over their single-word substring ("packed") via
/// the longest-match-first ordering in `refine::extract_adjectives_from_name`.
pub(crate) const DEFAULT_PREPARATION_ADJECTIVES: &[&str] = &[
    "chopped",
    "minced",
    "diced",
    "cubed",
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
    // Serving-temperature states (e.g. "warm water", "lukewarm milk", "chilled
    // butter") describe how the ingredient is brought to the recipe, not its
    // identity. Deliberately excludes "hot"/"cold" — those carry identity in
    // "hot sauce"/"hot dog"/"cold brew"/"cold cuts", which extraction would
    // corrupt to "sauce"/"brew".
    "warm",
    "lukewarm",
    "chilled",
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
    "recipe",
    "recipes",
    "packet",
    "packets",
    "sticks",
    "stick",
    "cloves",
    "clove",
    "bunch",
    "bunches",
    "head",
    "heads",
    "pinch",
    "pinches",
    "package",
    "packages",
    "slice",
    "slices",
    "standard",
    "can",
    "cans",
    "leaf",
    "leaves",
    "strand",
    "strands",
    "tin",
    "tins",
    "rib",
    "ribs",
    "sprig",
    "sprigs",
    "pint",
    "pints",
    "piece",
    "pieces",
    "disk",
    "disks",
    "stalk",
    "stalks",
    "loaf",
    "loaves",
    "ear",
    "ears",
    "handful",
    "handfuls",
    "dash",
    "dashes",
    // Sealed retail containers/packages, mirroring can/tin/package: a leading
    // "Bottle of red wine vinegar" / "jar of salsa" / "bag of flour" has an
    // implied count of 1 ({bottle:1} vinegar). Most also appear in CONTAINER_NOUNS
    // for the parenthesized-size path; listing them here makes the plain (no-paren)
    // form recognize them too. Matching is exact-word at the unit slot, so these
    // never strip a name that merely contains the substring (e.g. "jardinière").
    "bottle",
    "bottles",
    "jar",
    "jars",
    "tub",
    "tubs",
    "carton",
    "cartons",
    "container",
    "containers",
    "box",
    "boxes",
    "bag",
    "bags",
    "pouch",
    "pouches",
    "sachet",
    "sachets",
    "tube",
    "tubes",
    "bar",
    "bars",
    "block",
    "blocks",
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
pub(crate) const DISTRIBUTABLE_HEAD_NOUNS: &[&str] = &[
    "stock", "broth", "mustard", "pepper", "lettuce", "cabbage", "flour",
];

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

/// Clause boundaries that end a recovered head noun. When
/// `refine::recover::recover_head_noun_from_modifier` pulls a head noun out of a
/// modifier, the noun runs up to the next clause boundary: a comma, a
/// "such as"/"or"/"to taste" prose lead-in, or " (" — the last ends the noun at a
/// trailing parenthetical aside ("chicken thighs (8 to 12 thighs, …)"), before the
/// comma *inside* that aside can truncate the noun. Consumed by `refine::recover`.
pub(crate) const CLAUSE_BOUNDARIES: &[&str] = &[", ", " such as ", " or ", " to taste", " ("];

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

    // The shared-head lists diverge on purpose. "broth"/"stock" belong to BOTH:
    // they read as "<type> broth" whether the list is a bare "X, Y, or Z broth"
    // (SHARED_HEAD_NOUNS) or an inline "A or B broth" (DISTRIBUTABLE_HEAD_NOUNS).
    // "oil"/"vinegar" are in SHARED_HEAD_NOUNS only — in "butter or olive oil" the
    // left is a *distinct* ingredient, so distributing would graft nonsense
    // ("butter oil"); see the DISTRIBUTABLE_HEAD_NOUNS doc (vocab.rs ~428-431).
    // This pins the divergence so a future "cleanup" that merges the two lists
    // fails loudly. NOT a subset relation in either direction — do not add one.
    #[test]
    fn shared_head_lists_diverge_deliberately() {
        for w in ["broth", "stock"] {
            assert!(
                SHARED_HEAD_NOUNS.contains(&w),
                "{w:?} must be in SHARED_HEAD_NOUNS"
            );
            assert!(
                DISTRIBUTABLE_HEAD_NOUNS.contains(&w),
                "{w:?} must be in DISTRIBUTABLE_HEAD_NOUNS"
            );
        }
        for w in ["oil", "vinegar"] {
            assert!(
                SHARED_HEAD_NOUNS.contains(&w),
                "{w:?} must be in SHARED_HEAD_NOUNS"
            );
            assert!(
                !DISTRIBUTABLE_HEAD_NOUNS.contains(&w),
                "{w:?} must NOT be in DISTRIBUTABLE_HEAD_NOUNS (grafting \"butter oil\" is nonsense)"
            );
        }
    }

    // `refine::extract_size_unit_from_name` matches SIZE_UNIT_WORDS in order and
    // stops at the first hit, so every multi-word entry must precede any single
    // word it contains — otherwise "large" would match first inside "extra large"
    // / "extra-large" and strand the "extra". Pin the ordering.
    #[test]
    fn size_unit_words_are_longest_first() {
        for (i, entry) in SIZE_UNIT_WORDS.iter().enumerate() {
            for word in entry.split(['-', ' ']) {
                if word == *entry {
                    continue; // single-word entry, nothing longer to compare
                }
                if let Some(j) = SIZE_UNIT_WORDS.iter().position(|e| *e == word) {
                    assert!(
                        i < j,
                        "multi-word {entry:?} (index {i}) must precede its substring {word:?} (index {j})"
                    );
                }
            }
        }
    }

    // Adverb-folding (INTENSIFIER/MANNER_ADVERBS) and adjective extraction
    // (DEFAULT_PREPARATION_ADJECTIVES) both run in `refine::prep`. If a word were
    // in both an adverb list and the adjective set, the two passes would compete
    // for the same token. Pin them disjoint on the single-word adjective entries
    // (multi-word adjectives can't collide with the single-word adverbs anyway).
    #[test]
    fn adverbs_disjoint_from_single_word_adjectives() {
        let single_word_adjectives: std::collections::HashSet<&str> =
            DEFAULT_PREPARATION_ADJECTIVES
                .iter()
                .filter(|a| !a.contains(' '))
                .copied()
                .collect();
        for &adverb in INTENSIFIER_ADVERBS.iter().chain(MANNER_ADVERBS) {
            assert!(
                !single_word_adjectives.contains(adverb),
                "adverb {adverb:?} also appears in DEFAULT_PREPARATION_ADJECTIVES"
            );
        }
    }

    // NUMBER_WORDS (spelled amounts) and SPELLED_COUNTS (leading count tokens)
    // overlap on the plain integers: every number word is also a count token,
    // except "dozen"/"half" which SPELLED_COUNTS deliberately omits (a leading
    // "dozen"/"half" isn't a count slot). Pin the overlap so the two lists stay
    // in step.
    #[test]
    fn number_words_are_spelled_counts_except_dozen_and_half() {
        for &(word, _) in NUMBER_WORDS {
            if word == "dozen" || word == "half" {
                assert!(
                    !SPELLED_COUNTS.contains(&word),
                    "{word:?} is excluded from SPELLED_COUNTS by design"
                );
                continue;
            }
            assert!(
                SPELLED_COUNTS.contains(&word),
                "NUMBER_WORDS entry {word:?} missing from SPELLED_COUNTS"
            );
        }
    }

    // CLAUSE_BOUNDARIES (where a recovered head noun ends) and MODIFIER_STOPWORDS
    // (where a modifier turns to prose) both encode "prose starts here", so the
    // word-boundary boundaries' lead word must be a stopword. ", " and " (" are
    // punctuation, not words; the rest (" such as ", " or ", " to taste") lead
    // with "such"/"or"/"to", all stopwords.
    #[test]
    fn clause_boundaries_lead_words_are_stopwords() {
        assert!(!CLAUSE_BOUNDARIES.is_empty(), "CLAUSE_BOUNDARIES is empty");
        for &boundary in CLAUSE_BOUNDARIES {
            // Only the word-boundary entries encode "prose starts here"; the
            // punctuation-only ones (", ", " (") have no lead word to check.
            let Some(first) = boundary
                .split_whitespace()
                .next()
                .filter(|w| w.chars().all(char::is_alphabetic))
            else {
                continue;
            };
            assert!(
                MODIFIER_STOPWORDS.contains(&first),
                "CLAUSE_BOUNDARIES entry {boundary:?} leads with {first:?}, not a MODIFIER_STOPWORD"
            );
        }
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
