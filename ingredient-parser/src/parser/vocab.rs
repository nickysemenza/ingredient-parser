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
    "finely grated",
    "finely chopped",
    "coarsely chopped",
    "roughly chopped",
    "thinly sliced",
    "sliced",
    // Bare participle: "grated lemon zest" -> "lemon zest" / "grated". The
    // multiword "freshly grated"/"finely grated" win via longest-match-first.
    "grated",
    // "fresh" is the *implied default* state of herbs/produce/juice — "fresh
    // cilantro" is just cilantro. The marked forms ("dried"/"frozen") are named
    // explicitly and stay in the name. So extract "fresh" to the modifier.
    // Guarded in `extract_adjectives_from_name` against "fresh or frozen …",
    // where it is a genuine contrast, not an implied default.
    "fresh",
    "plain",
    "to taste",
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
    "loaf", "loaves", "ear", "ears",
];

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
    // fat / size
    "skim",
    "nonfat",
    "lean",
    "large",
    "small",
    "medium",
    "jumbo",
    "baby",
    // common attributive nouns that premodify a shared head ("lemon zest")
    "lemon",
    "lime",
    "orange",
    "grapefruit",
];

/// Intensifier adverbs that precede a preparation phrase ("very thinly sliced").
/// They carry no ingredient meaning on their own, so when one is stranded
/// immediately before an extracted prep adjective it is folded into the modifier
/// too — otherwise "very thinly sliced chives" leaves "very chives" as the name.
/// See `refine::extract_adjectives_from_name`.
pub(crate) const INTENSIFIER_ADVERBS: &[&str] = &["very", "really"];

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
