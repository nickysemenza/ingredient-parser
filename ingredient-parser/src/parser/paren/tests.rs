use super::*;

/// A unit set covering the units the Amount examples use.
fn units() -> HashSet<String> {
    [
        "cup",
        "cups",
        "g",
        "gram",
        "grams",
        "pound",
        "pounds",
        "tablespoon",
        "ounce",
        "ounces",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}

/// Each kind gets ≥3 example inners, including the edge cases the module docs
/// call out (mixed-content page ref, "(note the color)").
#[test]
fn classify_table() {
    let u = units();
    let u = Some(&u);

    // CrossReference — pure navigation cruft.
    for inner in [
        "see this page",
        "page 12",
        "this page, this page, or this page",
    ] {
        assert_eq!(classify(inner, u), ParenKind::CrossReference, "{inner:?}");
    }

    // NoteReference.
    for inner in ["see note", "notes", "note"] {
        assert_eq!(classify(inner, u), ParenKind::NoteReference, "{inner:?}");
    }

    // MinusEquivalence.
    for inner in [
        "2 sticks minus 1 tablespoon",
        "1 cup minus 2 tablespoons",
        "minus a pinch",
    ] {
        assert_eq!(classify(inner, u), ParenKind::MinusEquivalence, "{inner:?}");
    }

    // Optional.
    for inner in ["optional", "Optional", " optional "] {
        assert_eq!(classify(inner, u), ParenKind::Optional, "{inner:?}");
    }

    // Descriptive — temperature / distance asides.
    for inner in ["70° to 80°F", "¼ inch / 6 mm", "1 in thick"] {
        assert_eq!(classify(inner, u), ParenKind::Descriptive, "{inner:?}");
    }

    // Amount — measurement parentheticals.
    for inner in ["about 2 cups", "120 g", "1 pound"] {
        assert_eq!(classify(inner, u), ParenKind::Amount, "{inner:?}");
    }

    // Alias — bare words, no digits.
    for inner in ["red", "dark green", "Granny Smith"] {
        assert_eq!(classify(inner, u), ParenKind::Alias, "{inner:?}");
    }
}

/// Edge cases: mixed content is NOT the tight kind.
#[test]
fn classify_edge_cases() {
    let u = units();
    let u = Some(&u);

    // Mixed content — a page ref plus real content is not a CrossReference; with
    // no digits it falls through to Alias.
    assert_eq!(
        classify("from Lamb Meat Soup, this page", u),
        ParenKind::Alias,
        "mixed page-ref content is not CrossReference",
    );

    // "note the color" is real content, not a NoteReference; it falls to Alias.
    assert_ne!(classify("note the color", u), ParenKind::NoteReference);
    assert_eq!(classify("note the color", u), ParenKind::Alias);

    // Empty inner is Other (fails alias non-empty guard).
    assert_eq!(classify("", u), ParenKind::Other);
}

/// Ordering: a specific kind wins over Amount even when the inner could parse as
/// a measurement (minus-equivalence contains a quantity but is caught first).
#[test]
fn classify_ordering() {
    let u = units();
    let u = Some(&u);
    assert_eq!(
        classify("2 sticks minus 1 tablespoon", u),
        ParenKind::MinusEquivalence,
    );
    // Without units, the Amount check is skipped; a pure measure falls to Other.
    assert_eq!(classify("120 g", None), ParenKind::Other);
}

/// The shared predicates agree with the classifier's individual arms.
#[test]
fn predicates_match_arms() {
    assert!(is_cross_reference("see page 5"));
    assert!(!is_cross_reference("from Lamb Meat Soup, this page"));
    assert!(is_note_reference("see note"));
    assert!(!is_note_reference("note the color"));
    assert!(is_minus_equivalence("2 sticks minus 1 tablespoon"));
    assert!(is_optional("optional"));
    assert!(is_descriptive("70°F"));
    assert!(is_alias("red"));
    assert!(!is_alias("2 cups"));
    let u = units();
    assert!(is_amount("about 2 cups", &u));
    assert!(is_amount("120 g", &u));
}

/// `spans` yields each top-level parenthetical with a byte range that slices
/// back to the original text, and nested parens stay inside their outer span.
#[test]
fn spans_yields_top_level() {
    let s = "purple (red) cabbage (about 1 pound)";
    let got: Vec<(std::ops::Range<usize>, &str)> =
        spans(s).map(|p| (p.range.clone(), p.inner)).collect();
    assert_eq!(got.len(), 2);
    assert_eq!(&s[got[0].0.clone()], "(red)");
    assert_eq!(got[0].1, "red");
    assert_eq!(&s[got[1].0.clone()], "(about 1 pound)");
    assert_eq!(got[1].1, "about 1 pound");
}

#[test]
fn spans_nested_stays_outer() {
    let s = "chicken (a (b) c) rest";
    let got: Vec<&str> = spans(s).map(|p| p.inner).collect();
    assert_eq!(got, vec!["a (b) c"]);
}

#[test]
fn spans_unbalanced_stops() {
    let s = "flour (no close here";
    assert_eq!(spans(s).count(), 0);
}
