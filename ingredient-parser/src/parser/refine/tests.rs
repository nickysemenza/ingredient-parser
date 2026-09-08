#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use rstest::rstest;

#[test]
fn refine_pipeline_pass_ids_are_unique() {
    crate::assert_stage_pipeline!(REFINE_PIPELINE);
}

#[rstest]
// Fully wrapped: outer parens are stripped.
#[case::simple("(sifted)", Some("sifted"))]
#[case::with_percent("(70% cacao)", Some("70% cacao"))]
#[case::inner_trimmed("(  softened  )", Some("softened"))]
// Not wrapped, or only partially: left untouched.
#[case::plain("softened", Some("softened"))]
#[case::open_only("(partial", Some("(partial"))]
#[case::close_only("partial)", Some("partial)"))]
// Internal parens must NOT be collapsed (would merge distinct clauses).
#[case::two_groups("(a) and (b)", Some("(a) and (b)"))]
#[case::nested("(note (nested))", Some("(note (nested))"))]
// An empty group collapses away entirely.
#[case::empty("()", None)]
fn test_strip_wrapping_parens(#[case] input: &str, #[case] expected: Option<&str>) {
    assert_eq!(
        strip_wrapping_parens(Some(input.to_string())),
        expected.map(str::to_string)
    );
}

#[test]
fn test_strip_wrapping_parens_none() {
    assert_eq!(strip_wrapping_parens(None), None);
}

// ------------------------------------------------------------------
// Per-pass guard tests. These exercise the subtle conditions in each
// refine pass directly (previously only covered end-to-end by the
// accuracy corpus), so a regression points at the exact pass.
// ------------------------------------------------------------------

fn ing(name: &str, modifier: Option<&str>) -> ParsedIngredient {
    ParsedIngredient {
        name: name.to_string(),
        amounts: vec![],
        modifier: modifier
            .map(|m| vec![ModifierPart::raw(m.to_string())])
            .unwrap_or_default(),
        optional: false,
        ..Default::default()
    }
}

fn ing_with_amounts(name: &str, amounts: Vec<Measure>, modifier: Option<&str>) -> ParsedIngredient {
    ParsedIngredient {
        name: name.to_string(),
        amounts,
        modifier: modifier
            .map(|m| vec![ModifierPart::raw(m.to_string())])
            .unwrap_or_default(),
        optional: false,
        ..Default::default()
    }
}

/// Adjectives are pulled from the name into the modifier, but only on word
/// boundaries (so "well-chopped" is left intact).
#[rstest]
#[case::extracts("chopped onion", "onion", Some("chopped"))]
#[case::boundary_guard("well-chopped onion", "well-chopped onion", None)]
// Two adjectives in one name exercise the loop's name/name_lower rebuild.
#[case::two_adjectives("chopped sifted flour", "flour", Some("chopped, sifted"))]
// An adjective inside an "or" alternative is left for the alternative
// passes ("chopped" describes parsley, not basil). One before "or" is
// still extracted.
#[case::after_or_left_alone("basil or chopped parsley", "basil or chopped parsley", None)]
#[case::before_or_extracted("chopped basil or parsley", "basil or parsley", Some("chopped"))]
// " and " guard: a mid-seam adjective belongs to the second conjunct and is
// left in the name (multi-ingredient lines with "and" are out of scope)…
#[case::and_guard_keeps_conjunct(
    "Kosher salt and freshly ground black pepper",
    "Kosher salt and freshly ground black pepper",
    None
)]
// …but a TRAILING phrase after "and" (end-of-string) is still extracted.
#[case::and_trailing_extracted("Salt and pepper to taste", "Salt and pepper", Some("to taste"))]
// bare "grated" extracts; "fresh" (implied default) extracts…
#[case::grated_extracts("grated lemon zest", "lemon zest", Some("grated"))]
#[case::cubed_extracts("cubed seedless watermelon", "seedless watermelon", Some("cubed"))]
#[case::hyphenated_prep("medium-diced onions", "onions", Some("medium-diced"))]
#[case::manner_prep(
    "gently cracked cardamom pods",
    "cardamom pods",
    Some("gently cracked")
)]
#[case::crushed_extracts("crushed pistachios", "pistachios", Some("crushed"))]
#[case::fresh_extracts("fresh mint", "mint", Some("fresh"))]
// …except "fresh or frozen" — a genuine contrast — keeps "fresh" in the name.
#[case::fresh_or_kept("fresh or frozen blueberries", "fresh or frozen blueberries", None)]
fn test_extract_adjectives_from_name(
    #[case] name: &str,
    #[case] want_name: &str,
    #[case] want_modifier: Option<&str>,
) {
    let parser = IngredientParser::new();
    let mut i = ing(name, None);
    parser.extract_adjectives_from_name(&mut i);
    assert_eq!(i.name, want_name);
    assert_eq!(i.modifier_string().as_deref(), want_modifier);
}

/// A leading "<participle> or <adjective> <noun>" prep alternative moves to
/// the modifier; a genuine two-ingredient alternative is left alone.
#[rstest]
#[case::prep_alt("grated or finely chopped lemon zest", "lemon zest", true)]
#[case::genuine_alt("basil or chopped parsley", "basil or chopped parsley", false)]
fn test_extract_leading_prep_alternative(
    #[case] name: &str,
    #[case] want_name: &str,
    #[case] moved: bool,
) {
    let parser = IngredientParser::new();
    let mut i = ing(name, None);
    parser.extract_leading_prep_alternative(&mut i);
    assert_eq!(i.name, want_name);
    assert_eq!(i.modifier_string().is_some(), moved, "name: {name}");
}

/// Postfix produce units: the trailing count noun becomes the unit and the
/// food becomes the name; leading descriptors move to the modifier. Idioms
/// (food not on the allowlist) and non-count leads are left untouched.
#[test]
fn test_extract_postfix_produce_unit() {
    let parser = IngredientParser::new();

    let mut i = ing_with_amounts(
        "medium garlic clove",
        vec![Measure::new("whole", 1.0)],
        None,
    );
    parser.extract_postfix_produce_unit(&mut i);
    assert_eq!(i.name, "garlic");
    assert_eq!(i.amounts, vec![Measure::new("clove", 1.0)]);
    assert_eq!(i.modifier_string().as_deref(), Some("medium"));

    // Idiom guard: cinnamon isn't a produce food, so "cinnamon stick" stays.
    let mut i = ing_with_amounts("cinnamon stick", vec![Measure::new("whole", 1.0)], None);
    parser.extract_postfix_produce_unit(&mut i);
    assert_eq!(i.name, "cinnamon stick");
    assert_eq!(i.amounts, vec![Measure::new("whole", 1.0)]);

    // A real volume/weight lead (not a plain count) → don't fire.
    let mut i = ing_with_amounts("garlic clove", vec![Measure::new("cup", 1.0)], None);
    parser.extract_postfix_produce_unit(&mut i);
    assert_eq!(i.name, "garlic clove");
}

/// Size-as-count-unit: a leading size descriptor on an explicit whole count
/// becomes the unit ("3 medium carrots" -> `{medium:3}` carrots), with guards
/// for ranges, no-count, another-unit, "baby", and the size-range "or".
#[test]
fn test_extract_size_unit_from_name() {
    let parser = IngredientParser::new();
    let fire = |name: &str, amounts: Vec<Measure>| {
        let mut i = ing_with_amounts(name, amounts, None);
        parser.extract_size_unit_from_name(&mut i);
        (i.name, i.amounts)
    };

    // Fires: size becomes the unit, name is the bare produce.
    let (n, a) = fire("medium carrots", vec![Measure::new("whole", 3.0)]);
    assert_eq!(
        (n.as_str(), a),
        ("carrots", vec![Measure::new("medium", 3.0)])
    );

    // Multi-word grade canonicalizes; "extra-large" spelling too.
    let (n, a) = fire("extra large eggs", vec![Measure::new("whole", 2.0)]);
    assert_eq!(
        (n.as_str(), a),
        ("eggs", vec![Measure::new("extra large", 2.0)])
    );
    let (n, a) = fire("extra-large eggs", vec![Measure::new("whole", 1.0)]);
    assert_eq!(
        (n.as_str(), a),
        ("eggs", vec![Measure::new("extra large", 1.0)])
    );

    // A size word other than the two "extra large" spellings passes through as
    // its own unit — the canonicalization matches those spellings EXACTLY, so a
    // future "extra small" entry could never be mis-mapped to "extra large".
    let (n, a) = fire("jumbo eggs", vec![Measure::new("whole", 6.0)]);
    assert_eq!((n.as_str(), a), ("eggs", vec![Measure::new("jumbo", 6.0)]));

    // Range upper_value is preserved.
    let (n, a) = fire(
        "medium onions",
        vec![Measure::with_range("whole", 1.0, 2.0)],
    );
    assert_eq!(
        (n.as_str(), a),
        ("onions", vec![Measure::with_range("medium", 1.0, 2.0)])
    );

    // Guards (name/amounts unchanged):
    // no explicit whole count → nothing to size.
    assert_eq!(fire("medium onion", vec![]).0, "medium onion");
    // another unit already fills the slot.
    let (n, _) = fire("large onion", vec![Measure::new("cup", 2.0)]);
    assert_eq!(n, "large onion");
    // "baby" is a variety, excluded from SIZE_UNIT_WORDS.
    assert_eq!(
        fire("baby carrots", vec![Measure::new("whole", 2.0)]).0,
        "baby carrots"
    );
    // a size *range* ("medium or large") is left whole.
    assert_eq!(
        fire("medium or large carrots", vec![Measure::new("whole", 1.0)]).0,
        "medium or large carrots"
    );
    // a bare size with no following noun does not fire.
    assert_eq!(fire("medium", vec![Measure::new("whole", 1.0)]).0, "medium");
}

/// A trailing "for `<gerund>` …" clause (object included) moves to the
/// modifier; a plain "<name> for <noun>" is left intact.
#[rstest]
#[case::gerund(
    "Extra-virgin olive oil for brushing the bread",
    "Extra-virgin olive oil",
    Some("for brushing the bread")
)]
#[case::non_gerund("flour for bread", "flour for bread", None)]
fn test_extract_purpose_gerund(
    #[case] name: &str,
    #[case] want_name: &str,
    #[case] want_modifier: Option<&str>,
) {
    let parser = IngredientParser::new();
    let mut i = ing(name, None);
    parser.extract_purpose_gerund(&mut i);
    assert_eq!(i.name, want_name);
    assert_eq!(i.modifier_string().as_deref(), want_modifier);
}

/// The ordered `REFINE_PIPELINE` must be idempotent: running it a second
/// time on its own output must change nothing. This is the invariant the
/// load-bearing pass order depends on — a pass that isn't a fixpoint (e.g. it
/// re-extracts an adjective it already moved, or re-splits an alternative)
/// would silently corrupt results when a later edit reorders the list. This
/// test fails the moment that happens, naming the offending line.
#[rstest]
#[case::leading_adjective("1 onion, finely chopped")]
#[case::name_adjective("1 cup packed brown sugar, sifted")]
#[case::word_alternative("red or white onion")]
#[case::shared_head_alternatives("canola, vegetable, or melted coconut oil")]
#[case::quantity_alternative("1 clove garlic or 1 teaspoon garlic powder")]
#[case::secondary_amount("1 stick butter (8 tablespoons)")]
#[case::leading_prep_phrase("grated zest of 1 lemon")]
#[case::plain_name("kosher salt")]
#[case::postfix_produce("1 medium or large garlic clove, peeled")]
#[case::purpose_gerund("Extra-virgin olive oil for brushing the bread")]
#[case::fresh_extracted("fresh mint")]
#[case::and_guard("Kosher salt and freshly ground black pepper")]
// Order-constraint witnesses (see `ORDER_CONSTRAINTS`): idempotency is the
// invariant the load-bearing order rests on, so every witness must also be a
// fixpoint.
#[case::witness_recover_head_noun(
    "1/2 cup deribbed, seeded, and roughly chopped fresh hot green chiles, such as serrano"
)]
#[case::witness_leading_prep_alt("1 teaspoon grated or finely chopped lemon zest")]
#[case::witness_adj_before_alt("chopped red or white onion")]
#[case::witness_trailing_prep("2 cups spinach chopped into ribbons")]
#[case::witness_shared_head("canola, vegetable, or coconut oil")]
fn refine_pipeline_is_idempotent(#[case] line: &str) {
    let parser = IngredientParser::new();
    let (_, parsed) = parser.parse_ingredient_segmented(line).unwrap();

    let mut once = parsed.clone();
    parser.refine(&mut once);
    let mut twice = once.clone();
    parser.refine(&mut twice);

    assert_eq!(once, twice, "refine is not idempotent for {line:?}");
}
