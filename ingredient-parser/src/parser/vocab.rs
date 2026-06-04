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
    "thinly sliced",
    "sliced",
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

/// Non-standard units that aren't really convertible, seeded into the parser's
/// unit set. Note: "whole" is deliberately NOT included — it's the built-in
/// `Unit::Whole`, and listing it here would parse "whole wheat flour" as having
/// unit "whole" instead of keeping "whole wheat" in the name.
pub(crate) const NON_STANDARD_UNITS: &[&str] = &[
    "recipe", "packet", "sticks", "stick", "cloves", "clove", "bunch", "head", "pinch", "package",
    "slice", "slices", "standard", "can", "leaf", "leaves", "strand", "tin", "rib", "ribs",
    "sprig", "sprigs", "pint", "pints", "piece", "pieces", "disk", "disks", "stalk", "stalks",
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
