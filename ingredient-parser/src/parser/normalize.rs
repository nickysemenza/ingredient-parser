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

/// Strip a trailing footnote marker — an ASCII asterisk or dagger left at the
/// end of a line ("shredded zucchini (see note)*" → "shredded zucchini (see
/// note)"). Anchored to the end so a mid-name asterisk is untouched; the Unicode
/// circled-digit markers are handled by `strip_footnote_markers`.
fn strip_trailing_footnote_markers(input: &str) -> Cow<'_, str> {
    crate::lazy_regex!(TRAILING_MARK, r"\s*[*\u{2020}\u{2021}]+\s*$");
    TRAILING_MARK.replace(input, "")
}

/// Strip a leading list-bullet glyph — an en/em-dash, bullet, middot, or
/// asterisk followed by whitespace — that some cookbooks (e.g. hotpot ingredient
/// lists in *The Food of Sichuan*) prefix to each ingredient line. Left in place
/// it lands at the head of the name ("– shiitake mushrooms"). The trailing
/// whitespace requirement keeps a hyphenated/negative leading token untouched.
fn strip_leading_bullet(input: &str) -> Cow<'_, str> {
    crate::lazy_regex!(
        LEADING_BULLET,
        r"^\s*[-\u{2013}\u{2014}\u{2022}\u{00B7}\u{2219}*]\s+"
    );
    LEADING_BULLET.replace(input, "")
}

/// Strip a cross-reference parenthetical such as "(see this page)", "(this
/// page)", "(see page 123)", or a chain of them — "(this page to this page)",
/// "(this page, this page, or this page)" — a navigation artifact common in EPUB
/// cookbooks (links rendered as text). It carries no ingredient information, so
/// it is removed during normalization rather than leaking into the name or
/// modifier. The optional leading whitespace is absorbed so "walnuts (see this
/// page)," collapses cleanly to "walnuts,".
///
/// Scoped to a parenthetical whose content is ENTIRELY page references and the
/// connectors joining them (to / or / and / commas), so a paren that mixes a
/// page ref with real content ("(from Lamb Meat Soup, this page)") is left for
/// the modifier — only pure navigation cruft is dropped.
fn strip_cross_reference(input: &str) -> Cow<'_, str> {
    crate::lazy_regex!(
        CROSS_REF,
        r"(?i)\s*\(\s*(?:see\s+)?(?:this page|page\s+\d+)(?:[\s,;]*(?:to|or|and)?[\s,;]*(?:see\s+)?(?:this page|page\s+\d+))*\s*\)"
    );
    CROSS_REF.replace_all(input, "")
}

/// Drop a "(see note)" / "(note)" cross-reference parenthetical — a pointer to
/// the recipe's headnote, not ingredient information ("shredded zucchini (see
/// note)" → "shredded zucchini"). Scoped tightly to a paren whose content is
/// exactly an optional "see" plus "note"/"notes", so a paren carrying real
/// content ("(note the color)") is left for the modifier.
fn strip_note_reference(input: &str) -> Cow<'_, str> {
    crate::lazy_regex!(NOTE_REF, r"(?i)\s*\(\s*(?:see\s+)?notes?\s*\)");
    NOTE_REF.replace_all(input, "")
}

/// Rewrite a leading "N batch(es) of <recipe>" into "N recipe <recipe>" so the
/// existing `recipe` unit parses it as a sub-recipe reference ("1 batch of
/// Marshmallow Meringue" -> "1 recipe Marshmallow Meringue" -> name "Marshmallow
/// Meringue", `{recipe:1}`). Anchored to a leading quantity so prose uses of
/// "batch" elsewhere in a line are untouched; the quantity is preserved.
fn rewrite_batch_of_to_recipe(input: &str) -> Cow<'_, str> {
    crate::lazy_regex!(
        BATCH_OF,
        r"(?i)^(\s*[\d\x{00BC}-\x{00BE}\x{2150}-\x{215E}][\d\x{00BC}-\x{00BE}\x{2150}-\x{215E}\s./-]*\s+)batch(?:es)?\s+of\s+"
    );
    BATCH_OF.replace(input, "${1}recipe ")
}

/// Reduce a parenthetical that mixes a cross-reference with an "optional" note —
/// "(this page; optional)", "(see page 12, optional)" — down to just
/// "(optional)". The page ref is navigation cruft `strip_cross_reference` would
/// drop on its own, but it only fires on a paren that is *entirely* page refs, so
/// the mixed form would otherwise stall there and leak "this page; optional" into
/// the modifier. Running before `strip_cross_reference`, this peels off the ref
/// and leaves the bare "(optional)" for `strip_optional_note` to flag.
fn split_crossref_optional(input: &str) -> Cow<'_, str> {
    crate::lazy_regex!(
        CROSS_REF_OPTIONAL,
        r"(?i)\(\s*(?:see\s+)?(?:this page|page\s+\d+)(?:[\s,;]*(?:to|or|and)?[\s,;]*(?:see\s+)?(?:this page|page\s+\d+))*[\s,;]+optional\s*\)"
    );
    CROSS_REF_OPTIONAL.replace_all(input, "(optional)")
}

/// Strip a leading determiner ("the") sitting in front of a quantity, e.g.
/// "the ¼ cup of garlic chives" → "¼ cup of garlic chives". Scoped to "the"
/// immediately followed by a number so ordinary names ("the works seasoning")
/// are untouched. ("a"/"an" already read as a quantity of 1, so they're left.)
fn strip_leading_determiner(input: &str) -> Cow<'_, str> {
    crate::lazy_regex!(LEADING_THE, {
        use regex::Regex;
        let frac = crate::fraction::VULGAR_FRACTIONS;
        Regex::new(&format!(r"(?i)^the\s+([0-9{frac}])")).expect("invalid leading-the regex")
    });
    LEADING_THE.replace(input, "$1")
}

/// Drop an arithmetic-equivalence parenthetical containing "minus", e.g. the
/// "(2 sticks minus 1 tablespoon)" in "15 tablespoons (2 sticks minus 1
/// tablespoon) unsalted butter". The primary amount before it already states the
/// quantity; the aside is an equivalence note the structured parse can't use.
///
/// Guarded: the aside is only dropped when an amount is stated *elsewhere* on the
/// line (the redundant-restatement case). If the minus-paren is the line's only
/// quantity, it's kept — dropping it would silently zero the amount, whereas
/// leaving the digit lets the parse surface `unparsed_digit` for review instead.
fn strip_minus_equivalence(input: &str) -> Cow<'_, str> {
    crate::lazy_regex!(MINUS_PAREN, r"\s*\([^)]*\bminus\b[^)]*\)");
    let stripped = MINUS_PAREN.replace_all(input, "");
    // `replace_all` borrows unchanged when nothing matched.
    if matches!(stripped, Cow::Borrowed(_)) {
        return stripped;
    }
    let amount_remains = stripped
        .chars()
        .any(|c| c.is_ascii_digit() || crate::fraction::VULGAR_FRACTIONS.contains(c));
    if amount_remains {
        stripped
    } else {
        Cow::Borrowed(input)
    }
}

/// Drop a trailing "total" qualifier from inside a measurement parenthetical,
/// e.g. "(2 pounds total)" -> "(2 pounds)" in "Four … steaks (2 pounds total)".
/// "total" marks the parenthetical as the combined weight of all the items; the
/// word itself defeats `parse_parenthesized_amounts` (which needs the inner text
/// to fully parse as a measurement), so stripping it lets the weight hoist as a
/// secondary amount. Scoped to a parenthetical whose content starts with a number
/// so ordinary "(total recipe)"-style asides are untouched.
fn strip_total_in_measure_paren(input: &str) -> Cow<'_, str> {
    crate::lazy_regex!(TOTAL_PAREN, {
        use regex::Regex;
        let frac = crate::fraction::VULGAR_FRACTIONS;
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
    let lower = tok.to_lowercase();
    crate::parser::vocab::SPELLED_COUNTS.contains(&lower.as_str())
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

crate::define_stage_pipeline! {
    enum RewriteId,
    struct RewriteEntry,
    const REWRITES: &[RewriteEntry],
    type Rewrite = Rewrite,
    trace: none,
    (StripNbsp, "strip_nbsp", strip_nbsp),
    (StripLeadingBullet, "strip_leading_bullet", strip_leading_bullet),
    (StripFootnoteMarkers, "strip_footnote_markers", strip_footnote_markers),
    (
        StripTrailingFootnoteMarkers,
        "strip_trailing_footnote_markers",
        strip_trailing_footnote_markers
    ),
    (SplitCrossrefOptional, "split_crossref_optional", split_crossref_optional),
    (StripCrossReference, "strip_cross_reference", strip_cross_reference),
    (StripNoteReference, "strip_note_reference", strip_note_reference),
    (RewriteBatchOfToRecipe, "rewrite_batch_of_to_recipe", rewrite_batch_of_to_recipe),
    (StripLeadingDeterminer, "strip_leading_determiner", strip_leading_determiner),
    (StripMinusEquivalence, "strip_minus_equivalence", strip_minus_equivalence),
    (StripTotalInMeasureParen, "strip_total_in_measure_paren", strip_total_in_measure_paren),
    (LiftLeadingDimension, "lift_leading_dimension", lift_leading_dimension),
    (
        LiftLeadingPieceDimension,
        "lift_leading_piece_dimension",
        lift_leading_piece_dimension
    ),
}

/// Apply one rewrite to the accumulator, preserving its owned-ness: a borrowed
/// result means the rewrite changed nothing, so the accumulator is kept as-is
/// (no allocation on the common path). When tracing, a rewrite that *did* change
/// the line emits a before→after node so `--explain` shows which rewrite fired.
fn apply_rewrite<'a>(acc: Cow<'a, str>, rewrite: &RewriteEntry) -> Cow<'a, str> {
    let RewriteEntry { run, .. } = *rewrite;
    match run(acc.as_ref()) {
        Cow::Owned(rewritten) => {
            crate::trace::trace_on_change(rewrite.id().as_str(), acc.as_ref(), &rewritten, true);
            Cow::Owned(rewritten)
        }
        Cow::Borrowed(_) => acc,
    }
}

/// Run all pre-parse rewrites on a raw ingredient line, then collapse any
/// trailing/doubled whitespace a rewrite may have left behind.
pub(super) fn normalize_input(input: &str) -> Cow<'_, str> {
    let mut normalized = Cow::Borrowed(input);
    for rewrite in REWRITES {
        normalized = apply_rewrite(normalized, rewrite);
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
    crate::lazy_regex!(PAREN, r"(?i)\s*\(optional\)");
    crate::lazy_regex!(TRAIL_WORD, r"(?i),?\s+optional\s*$");

    let trimmed = input.trim();
    // Whole-line "(optional)" → leave for try_parse_optional_ingredient.
    if trimmed.eq_ignore_ascii_case("(optional)") {
        return (Cow::Borrowed(input), false);
    }

    // Check both patterns before allocating: the common case is no match, and
    // it must stay allocation-free (this runs on every parsed line).
    let paren_hit = PAREN.is_match(input);
    if !paren_hit && !TRAIL_WORD.is_match(input) {
        return (Cow::Borrowed(input), false);
    }

    let mut text = if paren_hit {
        PAREN.replace_all(input, "").into_owned()
    } else {
        input.to_string()
    };
    if TRAIL_WORD.is_match(&text) {
        text = TRAIL_WORD.replace(&text, "").into_owned();
    }
    (Cow::Owned(text), true)
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
    // The ambiguous bases "in" (the English preposition) and "m" only count
    // when number-adjacent, so "(packed in oil)" is not mistaken for a
    // dimension while "(1 in thick)" still is.
    let number_adjacent = |token_start: usize| {
        inner[..token_start]
            .chars()
            .rev()
            .find(|c| !c.is_whitespace() && *c != '-' && *c != '/')
            .is_some_and(|c| c.is_ascii_digit() || crate::fraction::is_vulgar(c))
    };
    let is_distance_token = |token_start: usize, w: &str| {
        crate::parser::is_distance_unit(w)
            && (!matches!(w.to_lowercase().as_str(), "in" | "m") || number_adjacent(token_start))
    };
    let has_distance_token = || {
        let mut token_start = 0usize;
        let mut in_token = false;
        for (i, c) in inner
            .char_indices()
            .chain(std::iter::once((inner.len(), ' ')))
        {
            if c.is_alphabetic() {
                if !in_token {
                    token_start = i;
                    in_token = true;
                }
            } else if in_token {
                if is_distance_token(token_start, &inner[token_start..i]) {
                    return true;
                }
                in_token = false;
            }
        }
        false
    };
    let looks_descriptive = inner.contains('°') || has_distance_token();
    if !looks_descriptive {
        return None;
    }

    let cleaned = format!("{before} {after}");
    Some((cleaned, inner.to_string()))
}

/// Collapse runs of whitespace to single spaces and trim. Shared with the
/// post-parse name cleanup in the refine phase.
pub(super) fn collapse_whitespace(input: &str) -> String {
    // Write words straight into a pre-sized buffer; the old
    // `split_whitespace().collect::<Vec<_>>().join(" ")` allocated a throwaway Vec
    // on every parse (this runs in both normalize and the refine name cleanup).
    let mut out = String::with_capacity(input.len());
    for word in input.split_whitespace() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(word);
    }
    out
}

#[cfg(test)]
mod tests;
