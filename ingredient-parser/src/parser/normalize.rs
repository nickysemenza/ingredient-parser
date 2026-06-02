//! Pre-parse input normalization.
//!
//! These rewrites clean cookbook/EPUB text artifacts off the raw line *before*
//! the nom grammar sees it (non-breaking spaces, footnote glyphs, cross-reference
//! and equivalence parentheticals, attached-unit range notation, a leading
//! determiner), and split off a trailing/inline "(optional)" note. Each rewrite
//! returns a borrow when it changes nothing, so the common path does not allocate.

use std::borrow::Cow;

/// A circled-number glyph (①②③ …) used as a footnote/technique-note marker in
/// some cookbooks (e.g. Claire Saffitz's *Dessert Person*). They're not part of
/// the ingredient, so they're stripped during normalization rather than leaking
/// into the name or modifier.
fn is_footnote_marker(c: char) -> bool {
    matches!(c,
        '\u{2460}'..='\u{2473}'   // ① .. ⑳  circled 1–20
        | '\u{2474}'..='\u{2487}' // ⑴ .. ⒈  parenthesized / full-stop digits
        | '\u{2488}'..='\u{249B}'
        | '\u{24EA}'              // ⓪ circled zero
        | '\u{24F5}'..='\u{24FF}' // double-circled / negative-circled digits
        | '\u{2776}'..='\u{2793}' // dingbat negative/sans-serif circled digits
    )
}

/// Replace non-breaking spaces (common in PDF/EPUB-extracted text) with ASCII
/// spaces so the grammar's space handling works.
fn strip_nbsp(input: &str) -> Cow<'_, str> {
    if input.contains('\u{a0}') {
        Cow::Owned(input.replace('\u{a0}', " "))
    } else {
        Cow::Borrowed(input)
    }
}

/// Drop footnote markers (e.g. "rye flour ①" → "rye flour ").
fn strip_footnote_markers(input: &str) -> Cow<'_, str> {
    if input.chars().any(is_footnote_marker) {
        Cow::Owned(input.chars().filter(|c| !is_footnote_marker(*c)).collect())
    } else {
        Cow::Borrowed(input)
    }
}

/// Strip a cross-reference parenthetical such as "(see this page)", "(this
/// page)", or "(see page 123)" — a navigation artifact common in EPUB cookbooks
/// (links rendered as text). It carries no ingredient information, so it is
/// removed during normalization rather than leaking into the name or modifier.
/// The optional leading whitespace is absorbed so "walnuts (see this page),"
/// collapses cleanly to "walnuts,".
fn strip_cross_reference(input: &str) -> Cow<'_, str> {
    use regex::Regex;
    use std::sync::LazyLock;
    static CROSS_REF: LazyLock<Regex> = LazyLock::new(|| {
        #[allow(clippy::expect_used)]
        Regex::new(r"(?i)\s*\((?:see\s+)?(?:this page|page\s+\d+)\)")
            .expect("invalid cross-reference regex")
    });
    CROSS_REF.replace_all(input, "")
}

/// Normalize the cookbook "range-with-attached-unit" notation
/// "3½- to 4-pound" / "4½- to 5½-pound" into the parseable "3½ to 4 pound", so
/// it folds into a single ranged `Measure`. The hyphens attach the dash to the
/// first number and the unit to the second number, which otherwise defeats the
/// range parser. Scoped to the `<num>- to <num>-<word>` shape so ordinary
/// hyphenated names ("all-purpose") are untouched.
fn normalize_dimension_range(input: &str) -> Cow<'_, str> {
    use regex::Regex;
    use std::sync::LazyLock;
    static DIM_RANGE: LazyLock<Regex> = LazyLock::new(|| {
        #[allow(clippy::expect_used)]
        Regex::new(
            r"([0-9./¼½¾⅓⅔⅕⅖⅗⅘⅙⅚⅛⅜⅝⅞]+)-\s*(to|through)\s+([0-9./¼½¾⅓⅔⅕⅖⅗⅘⅙⅚⅛⅜⅝⅞]+)-([A-Za-z]+)",
        )
        .expect("invalid dimension-range regex")
    });
    DIM_RANGE.replace_all(input, "$1 $2 $3 $4")
}

/// Strip a leading determiner ("the") sitting in front of a quantity, e.g.
/// "the ¼ cup of garlic chives" → "¼ cup of garlic chives". Scoped to "the"
/// immediately followed by a number so ordinary names ("the works seasoning")
/// are untouched. ("a"/"an" already read as a quantity of 1, so they're left.)
fn strip_leading_determiner(input: &str) -> Cow<'_, str> {
    use regex::Regex;
    use std::sync::LazyLock;
    static LEADING_THE: LazyLock<Regex> = LazyLock::new(|| {
        #[allow(clippy::expect_used)]
        Regex::new(r"(?i)^the\s+([0-9¼½¾⅓⅔⅕⅖⅗⅘⅙⅚⅛⅜⅝⅞])").expect("invalid leading-the regex")
    });
    LEADING_THE.replace(input, "$1")
}

/// Drop an arithmetic-equivalence parenthetical containing "minus", e.g. the
/// "(2 sticks minus 1 tablespoon)" in "15 tablespoons (2 sticks minus 1
/// tablespoon) unsalted butter". The primary amount before it already states the
/// quantity; the aside is an equivalence note the structured parse can't use.
fn strip_minus_equivalence(input: &str) -> Cow<'_, str> {
    use regex::Regex;
    use std::sync::LazyLock;
    static MINUS_PAREN: LazyLock<Regex> = LazyLock::new(|| {
        #[allow(clippy::expect_used)]
        Regex::new(r"\s*\([^)]*\bminus\b[^)]*\)").expect("invalid minus-equivalence regex")
    });
    MINUS_PAREN.replace_all(input, "")
}

/// A single pre-parse rewrite: takes the current text and returns it changed
/// (`Cow::Owned`) or unchanged (`Cow::Borrowed`).
type Rewrite = fn(&str) -> Cow<'_, str>;

/// The ordered pre-parse rewrite pipeline. Each entry runs on the output of the
/// previous one; a borrow result means "no change" and is threaded through
/// without allocating. Adding a rewrite is a one-line edit here.
const REWRITES: &[(&str, Rewrite)] = &[
    ("strip_nbsp", strip_nbsp),
    ("strip_footnote_markers", strip_footnote_markers),
    ("strip_cross_reference", strip_cross_reference),
    ("normalize_dimension_range", normalize_dimension_range),
    ("strip_leading_determiner", strip_leading_determiner),
    ("strip_minus_equivalence", strip_minus_equivalence),
];

/// Apply one rewrite to the accumulator, preserving its owned-ness: a borrowed
/// result means the rewrite changed nothing, so the accumulator is kept as-is
/// (no allocation on the common path).
fn apply_rewrite<'a>(acc: Cow<'a, str>, rewrite: Rewrite) -> Cow<'a, str> {
    match rewrite(acc.as_ref()) {
        Cow::Owned(rewritten) => Cow::Owned(rewritten),
        Cow::Borrowed(_) => acc,
    }
}

/// Run all pre-parse rewrites on a raw ingredient line, then collapse any
/// trailing/doubled whitespace a rewrite may have left behind.
pub(super) fn normalize_input(input: &str) -> Cow<'_, str> {
    let mut normalized = Cow::Borrowed(input);
    for (_name, rewrite) in REWRITES {
        normalized = apply_rewrite(normalized, *rewrite);
    }

    let has_multiple_spaces = normalized
        .as_bytes()
        .windows(2)
        .any(|w| w[0] == b' ' && w[1] == b' ');

    // A stripped marker can leave a trailing/doubled space ("rye flour ").
    let needs_trim =
        has_multiple_spaces || normalized.starts_with(' ') || normalized.ends_with(' ');

    if needs_trim {
        Cow::Owned(collapse_whitespace(normalized.as_ref()))
    } else {
        normalized
    }
}

/// Strip an "(optional)" note from a line, returning the cleaned line plus
/// whether the note was present. Handles a parenthesized note anywhere (trailing
/// "X (optional)" or mid-line "X (optional), chopped") and a trailing word form
/// (", optional"). A mid-line note is removed so it neither lands in the modifier
/// nor blocks a trailing weight parenthetical from being hoisted. Returns a
/// borrow when nothing changed, to avoid allocating on the common path. A
/// whole-line "(optional)" (nothing else) is left for the optional-ingredient
/// path and not treated as a note.
pub(super) fn strip_optional_note(input: &str) -> (Cow<'_, str>, bool) {
    use regex::Regex;
    use std::sync::LazyLock;
    static PAREN: LazyLock<Regex> = LazyLock::new(|| {
        #[allow(clippy::expect_used)]
        Regex::new(r"(?i)\s*\(optional\)").expect("invalid optional-note regex")
    });
    static TRAIL_WORD: LazyLock<Regex> = LazyLock::new(|| {
        #[allow(clippy::expect_used)]
        Regex::new(r"(?i),?\s+optional\s*$").expect("invalid optional-word regex")
    });

    let trimmed = input.trim();
    // Whole-line "(optional)" → leave for try_parse_optional_ingredient.
    if trimmed.eq_ignore_ascii_case("(optional)") {
        return (Cow::Borrowed(input), false);
    }

    let mut found = false;
    let mut text = if PAREN.is_match(input) {
        found = true;
        PAREN.replace_all(input, "").into_owned()
    } else {
        input.to_string()
    };
    if TRAIL_WORD.is_match(&text) {
        found = true;
        text = TRAIL_WORD.replace(&text, "").into_owned();
    }

    if found {
        (Cow::Owned(text), true)
    } else {
        (Cow::Borrowed(input), false)
    }
}

/// Collapse runs of whitespace to single spaces and trim. Shared with the
/// post-parse name cleanup in the refine phase.
pub(super) fn collapse_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}
