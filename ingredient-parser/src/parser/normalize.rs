//! Pre-parse input normalization.
//!
//! These rewrites clean cookbook/EPUB text artifacts off the raw line *before*
//! the nom grammar sees it (non-breaking spaces, footnote glyphs, cross-reference
//! and equivalence parentheticals, attached-unit range notation, a leading
//! determiner), and split off a trailing/inline "(optional)" note. Each rewrite
//! returns a borrow when it changes nothing, so the common path does not allocate.
//!
//! Also houses `lift_inline_descriptive_paren`, a pre-parse rewrite that pulls a
//! descriptive parenthetical out from between name words (returning the cleaned
//! line plus the lifted aside) before the core parse runs.

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

/// Strip a leading list-bullet glyph — an en/em-dash, bullet, middot, or
/// asterisk followed by whitespace — that some cookbooks (e.g. hotpot ingredient
/// lists in *The Food of Sichuan*) prefix to each ingredient line. Left in place
/// it lands at the head of the name ("– shiitake mushrooms"). The trailing
/// whitespace requirement keeps a hyphenated/negative leading token untouched.
fn strip_leading_bullet(input: &str) -> Cow<'_, str> {
    use regex::Regex;
    use std::sync::LazyLock;
    static LEADING_BULLET: LazyLock<Regex> = LazyLock::new(|| {
        #[allow(clippy::expect_used)]
        Regex::new(r"^\s*[-\u{2013}\u{2014}\u{2022}\u{00B7}\u{2219}*]\s+")
            .expect("invalid leading-bullet regex")
    });
    LEADING_BULLET.replace(input, "")
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
        let frac = crate::fraction::VULGAR_FRACTIONS;
        #[allow(clippy::expect_used)]
        Regex::new(&format!(
            r"([0-9./{frac}]+)-\s*(to|through)\s+([0-9./{frac}]+)-([A-Za-z]+)"
        ))
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
        let frac = crate::fraction::VULGAR_FRACTIONS;
        #[allow(clippy::expect_used)]
        Regex::new(&format!(r"(?i)^the\s+([0-9{frac}])")).expect("invalid leading-the regex")
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

/// Drop a trailing "total" qualifier from inside a measurement parenthetical,
/// e.g. "(2 pounds total)" -> "(2 pounds)" in "Four … steaks (2 pounds total)".
/// "total" marks the parenthetical as the combined weight of all the items; the
/// word itself defeats `parse_parenthesized_amounts` (which needs the inner text
/// to fully parse as a measurement), so stripping it lets the weight hoist as a
/// secondary amount. Scoped to a parenthetical whose content starts with a number
/// so ordinary "(total recipe)"-style asides are untouched.
fn strip_total_in_measure_paren(input: &str) -> Cow<'_, str> {
    use regex::Regex;
    use std::sync::LazyLock;
    static TOTAL_PAREN: LazyLock<Regex> = LazyLock::new(|| {
        let frac = crate::fraction::VULGAR_FRACTIONS;
        #[allow(clippy::expect_used)]
        Regex::new(&format!(r"(?i)\(([0-9{frac}][^)]*?)\s+(?:in\s+)?total\)"))
            .expect("invalid total-paren regex")
    });
    TOTAL_PAREN.replace_all(input, "($1)")
}

/// Lift a leading dimension *descriptor* — a "<n>-inch-thick"-style token, bare or
/// parenthesized, wedged between a leading count and the ingredient name — out to
/// a trailing modifier. The descriptor describes the item's shape, not a quantity,
/// so leaving it inline either stalls the name grammar (parenthesized form) or
/// glues the descriptor onto the name. Moving it to the end lets the count and
/// name parse cleanly and routes the descriptor into the modifier. E.g.
/// "1 (1½-inch-thick) bone-in pork chop (about 1¼ pounds)" becomes
/// "1 bone-in pork chop (about 1¼ pounds), 1½-inch-thick", and
/// "Four ½-inch-thick boneless pork shoulder steaks (2 pounds)" becomes
/// "Four boneless pork shoulder steaks (2 pounds), ½-inch-thick".
///
/// Scoped tightly: the first token must be a count and the second must end in a
/// shape suffix (-thick/-long/-wide/-deep/-tall). A size like "10-ounce" (which is
/// hoisted as an *amount* by the count+size parser) ends in a unit, not a shape
/// word, so it is never moved.
fn lift_leading_dimension(input: &str) -> Cow<'_, str> {
    let mut tokens = input.split_whitespace();
    let (Some(first), Some(second)) = (tokens.next(), tokens.next()) else {
        return Cow::Borrowed(input);
    };
    if !is_count_token(first) {
        return Cow::Borrowed(input);
    }
    let descriptor = second.trim_start_matches('(').trim_end_matches(')');
    if !is_dimension_descriptor(descriptor) {
        return Cow::Borrowed(input);
    }
    let rest: Vec<&str> = tokens.collect();
    if rest.is_empty() {
        return Cow::Borrowed(input);
    }
    Cow::Owned(format!("{first} {}, {descriptor}", rest.join(" ")))
}

/// Lift a leading bare *dimension* sizing a container noun that is followed by a
/// weight parenthetical — "1-inch piece (20g) ginger", "¾-inch piece (15g)
/// ginger", "1–1½-inch piece (20–30g) ginger" — out to a trailing descriptor,
/// inserting an explicit count of 1. The dimension sizes the piece, it is not a
/// count (the "¾" in "¾-inch piece" is ¾ of an *inch*, not ¾ of a piece), and the
/// weight parenthetical after the container otherwise stalls the whole parse
/// (yielding name-only). Rewriting to "1 piece (20g) ginger, unpeeled, 1-inch"
/// routes the weight through the working count+container+size path
/// ([1 piece, 20 g]) and carries the dimension into the modifier.
///
/// Scoped tightly: the first token must be a bare `<number>-<distance-unit>`
/// dimension (no shape suffix — "-thick"/"-long" go to [`lift_leading_dimension`];
/// a size like "10-ounce" ends in a unit, not a distance word), the second a known
/// container noun, and the third a parenthetical starting with a number (the
/// weight). Without that weight paren the bare form "2-inch piece ginger" already
/// parses ([2", 1 piece]) and is left untouched.
fn lift_leading_piece_dimension(input: &str) -> Cow<'_, str> {
    let mut tokens = input.split_whitespace();
    let (Some(first), Some(second), Some(third)) = (tokens.next(), tokens.next(), tokens.next())
    else {
        return Cow::Borrowed(input);
    };
    if !is_bare_dimension(first) {
        return Cow::Borrowed(input);
    }
    if !crate::parser::vocab::CONTAINER_NOUNS.contains(&second.to_lowercase().as_str()) {
        return Cow::Borrowed(input);
    }
    // The third token must be a measurement parenthetical "(20g)" — the case that
    // breaks the parse. (The no-weight bare form is handled by the grammar.)
    let mut weight = third.chars();
    if weight.next() != Some('(')
        || !weight
            .next()
            .is_some_and(|c| c.is_ascii_digit() || crate::fraction::is_vulgar(c))
    {
        return Cow::Borrowed(input);
    }
    let rest: Vec<&str> = std::iter::once(third).chain(tokens).collect();
    Cow::Owned(format!("1 {second} {}, {first}", rest.join(" ")))
}

/// A bare dimension token: a number (digit/vulgar fraction, optionally a range)
/// joined by a hyphen to a distance-unit suffix — "1-inch", "¾-inch",
/// "1–1½-inch", "2-cm". A weight size like "10-ounce" ends in a unit, not a
/// distance word, so it returns false.
fn is_bare_dimension(tok: &str) -> bool {
    let starts_number = tok
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_digit() || crate::fraction::is_vulgar(c));
    let Some((_, suffix)) = tok.rsplit_once('-') else {
        return false;
    };
    starts_number && crate::parser::is_distance_unit(suffix)
}

/// A leading count: digits/decimal/fraction ("1", "1½", "2.5") or a spelled-out
/// small number ("one" … "twelve", "a"/"an").
fn is_count_token(tok: &str) -> bool {
    const SPELLED: &[&str] = &[
        "a", "an", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
        "eleven", "twelve",
    ];
    let lower = tok.to_lowercase();
    SPELLED.contains(&lower.as_str())
        || (!tok.is_empty()
            && tok.chars().all(|c| {
                c.is_ascii_digit() || c == '.' || c == '/' || crate::fraction::is_vulgar(c)
            }))
}

/// A dimension/shape descriptor token: starts with a number and ends in a shape
/// suffix, e.g. "1½-inch-thick", "2-inch-long". The shape suffix distinguishes it
/// from a hoistable size like "10-ounce".
fn is_dimension_descriptor(tok: &str) -> bool {
    const SHAPE_SUFFIXES: &[&str] = &["thick", "long", "wide", "deep", "tall"];
    if !tok.contains('-') {
        return false;
    }
    let first = tok.chars().next();
    let starts_number = first.is_some_and(|c| c.is_ascii_digit() || crate::fraction::is_vulgar(c));
    let ends_shape = tok
        .rsplit('-')
        .next()
        .is_some_and(|suffix| SHAPE_SUFFIXES.contains(&suffix.to_lowercase().as_str()));
    starts_number && ends_shape
}

/// A single pre-parse rewrite: takes the current text and returns it changed
/// (`Cow::Owned`) or unchanged (`Cow::Borrowed`).
type Rewrite = fn(&str) -> Cow<'_, str>;

/// The ordered pre-parse rewrite pipeline. Each entry runs on the output of the
/// previous one; a borrow result means "no change" and is threaded through
/// without allocating. Adding a rewrite is a one-line edit here.
const REWRITES: &[(&str, Rewrite)] = &[
    ("strip_nbsp", strip_nbsp),
    ("strip_leading_bullet", strip_leading_bullet),
    ("strip_footnote_markers", strip_footnote_markers),
    ("strip_cross_reference", strip_cross_reference),
    ("normalize_dimension_range", normalize_dimension_range),
    ("strip_leading_determiner", strip_leading_determiner),
    ("strip_minus_equivalence", strip_minus_equivalence),
    ("strip_total_in_measure_paren", strip_total_in_measure_paren),
    ("lift_leading_dimension", lift_leading_dimension),
    ("lift_leading_piece_dimension", lift_leading_piece_dimension),
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

/// Detect a *descriptive* parenthetical wedged between name words — a
/// temperature ("70° to 80°F") or distance ("¼ inch / 6 mm") aside flanked by
/// alphabetic name text on both sides. Returns the line with that parenthetical
/// removed plus the aside text (to become a modifier), or `None` when no such
/// parenthetical is present.
///
/// Deliberately narrow: requires a letter immediately before the `(` and name
/// text after the `)`, and only fires for temperature/distance asides. This
/// keeps mass/volume parentheticals like "(190 grams)" hoisted as amounts, and
/// leaves the count+size form "4 (½-inch) slices" (digit before the paren) and
/// trailing parentheticals like "water (100°F) — 472 g" to their own paths.
pub(super) fn lift_inline_descriptive_paren(input: &str) -> Option<(String, String)> {
    let open = input.find('(')?;
    // A letter must immediately precede the "(" (allowing one space): this is the
    // "name (aside) name" shape, not "<count> (size)" or a leading paren.
    let before = input[..open].trim_end();
    if !before.chars().next_back().is_some_and(char::is_alphabetic) {
        return None;
    }
    // Matching close paren (no nesting expected in these asides).
    let close_rel = input[open..].find(')')?;
    let close = open + close_rel;
    let inner = input[open + 1..close].trim();
    let after = input[close + 1..].trim_start();

    // Name text must follow the parenthetical (else it's a trailing paren).
    if !after.chars().next().is_some_and(char::is_alphabetic) {
        return None;
    }

    // Only lift descriptive asides: a temperature (°) or a distance unit token.
    let looks_descriptive = inner.contains('°')
        || inner
            .split(|c: char| !c.is_alphabetic())
            .any(|w| !w.is_empty() && crate::parser::is_distance_unit(w));
    if !looks_descriptive {
        return None;
    }

    let cleaned = format!("{before} {after}");
    Some((cleaned, inner.to_string()))
}

/// Collapse runs of whitespace to single spaces and trim. Shared with the
/// post-parse name cleanup in the refine phase.
pub(super) fn collapse_whitespace(input: &str) -> String {
    input.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    // Descriptive aside flanked by name words → lifted out.
    #[case::temp(
        "room-temperature (70° to 80°F) water",
        Some(("room-temperature water", "70° to 80°F"))
    )]
    #[case::distance(
        "sliced (¼ inch / 6 mm) green onions",
        Some(("sliced green onions", "¼ inch / 6 mm"))
    )]
    // Mass/volume parenthetical → left for the amount path.
    #[case::mass("flour (190 grams) sifted", None)]
    // Count + size ("4 (½-inch) slices"): digit before paren, not a name word.
    #[case::count_size("4 (½-inch) slices pork", None)]
    // Trailing paren (no name text after) → left for other paths.
    #[case::trailing("warm water (100°F)", None)]
    // Leading paren (optional-ingredient shape) → untouched.
    #[case::leading("(70°F) water", None)]
    fn test_lift_inline_descriptive_paren(
        #[case] input: &str,
        #[case] expected: Option<(&str, &str)>,
    ) {
        let got = lift_inline_descriptive_paren(input);
        assert_eq!(
            got,
            expected.map(|(c, a)| (c.to_string(), a.to_string())),
            "input: {input}"
        );
    }

    #[rstest]
    // Parenthesized leading descriptor → moved to a trailing modifier.
    #[case::paren_form(
        "1 (1½-inch-thick) bone-in pork chop (about 1¼ pounds)",
        "1 bone-in pork chop (about 1¼ pounds), 1½-inch-thick"
    )]
    // Bare leading descriptor after a spelled count → moved to the end.
    #[case::bare_form(
        "Four ½-inch-thick boneless pork shoulder steaks (2 pounds)",
        "Four boneless pork shoulder steaks (2 pounds), ½-inch-thick"
    )]
    // A size ("10-ounce") ends in a unit, not a shape word → left for the
    // count+size amount parser, never moved.
    #[case::size_untouched("1 (10-ounce) can tomatoes", "1 (10-ounce) can tomatoes")]
    // No leading count → untouched (avoids stealing a mid-line "¼-inch-thick").
    #[case::no_count(
        "onion, cut into ¼-inch-thick slices",
        "onion, cut into ¼-inch-thick slices"
    )]
    // A plain dimension ("8-inch") without a shape suffix → untouched.
    #[case::plain_dimension("1 8-inch cake pan", "1 8-inch cake pan")]
    fn test_lift_leading_dimension(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(
            lift_leading_dimension(input).as_ref(),
            expected,
            "input: {input}"
        );
    }

    #[rstest]
    // Bare dimension + container + weight paren → count 1 inserted, dimension
    // carried to the end so the count+container+weight path can parse.
    #[case::inch_piece(
        "1-inch piece (20g) ginger, unpeeled",
        "1 piece (20g) ginger, unpeeled, 1-inch"
    )]
    #[case::vulgar("¾-inch piece (15g) ginger", "1 piece (15g) ginger, ¾-inch")]
    #[case::range(
        "1–1½-inch piece (20–30g) ginger, unpeeled",
        "1 piece (20–30g) ginger, unpeeled, 1–1½-inch"
    )]
    #[case::cm_knob("2-cm knob (10g) ginger", "1 knob (10g) ginger, 2-cm")]
    // No weight paren → bare form parses fine on its own; left untouched.
    #[case::no_weight("2-inch piece ginger", "2-inch piece ginger")]
    // A weight size ("10-ounce") ends in a unit, not a distance word → untouched.
    #[case::weight_size("10-ounce can tomatoes", "10-ounce can tomatoes")]
    // Second token not a container noun → untouched ("pan" is not a container).
    #[case::not_container(
        "9-inch springform (about 23cm) pan",
        "9-inch springform (about 23cm) pan"
    )]
    fn test_lift_leading_piece_dimension(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(
            lift_leading_piece_dimension(input).as_ref(),
            expected,
            "input: {input}"
        );
    }

    #[rstest]
    // Leading list bullets (en/em-dash, bullet, asterisk) are stripped.
    #[case::en_dash("– shiitake mushrooms, whole", "shiitake mushrooms, whole")]
    #[case::em_dash("— bean sprouts", "bean sprouts")]
    #[case::bullet("• daikon, sliced", "daikon, sliced")]
    #[case::ascii_hyphen("- firm tofu", "firm tofu")]
    // A hyphenated leading token (no trailing space after the dash) is untouched.
    #[case::hyphenated_name("all-purpose flour", "all-purpose flour")]
    fn test_strip_leading_bullet(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(
            strip_leading_bullet(input).as_ref(),
            expected,
            "input: {input}"
        );
    }

    #[rstest]
    // "total" inside a measure parenthetical is dropped so the weight can hoist.
    #[case::pounds("steaks (2 pounds total)", "steaks (2 pounds)")]
    #[case::in_total("steaks (2 pounds in total)", "steaks (2 pounds)")]
    // A non-measure parenthetical (no leading number) is left alone.
    #[case::not_measure("(total recipe)", "(total recipe)")]
    fn test_strip_total_in_measure_paren(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(
            strip_total_in_measure_paren(input).as_ref(),
            expected,
            "input: {input}"
        );
    }
}
