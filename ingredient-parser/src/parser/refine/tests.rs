#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use rstest::rstest;

#[test]
fn refine_pipeline_pass_ids_are_unique() {
    crate::assert_stage_pipeline!(REFINE_PIPELINE);
}

fn pass_index(id: PassId) -> usize {
    REFINE_PIPELINE
        .iter()
        .position(|pass| pass.id() == id)
        .expect("REFINE_PIPELINE missing expected pass")
}

/// Every declared ordering edge in [`ORDER_CONSTRAINTS`] holds positionally:
/// `before` precedes `after` in the pipeline. Paired with
/// `constraints_are_load_bearing` (which proves each edge actually matters), this
/// makes the pass order a two-sided contract.
#[test]
fn declared_order_matches_pipeline() {
    for c in ORDER_CONSTRAINTS {
        assert!(
            pass_index(c.before) < pass_index(c.after),
            "{:?} must run before {:?}: {}",
            c.before,
            c.after,
            c.reason
        );
    }
}

/// Purely positional invariants that aren't a two-pass ordering edge (so they
/// don't fit the witness-backed constraint table): the collapse pass sits between
/// the adjective peel and the alternatives split.
#[test]
fn refine_pipeline_positional_invariants() {
    assert!(
        pass_index(PassId::CollapseName) < pass_index(PassId::ExtractAlternativesFromName),
        "collapse before alternatives"
    );
}

/// Each edge in [`ORDER_CONSTRAINTS`] must be *load-bearing*: running its witness
/// with the two passes swapped produces a different `ParsedIngredient` than the
/// declared order. A constraint whose swap changes nothing is dead documentation
/// and fails here, naming itself.
#[test]
fn constraints_are_load_bearing() {
    let parser = IngredientParser::new();
    for c in ORDER_CONSTRAINTS {
        let (_, base) = parser.parse_ingredient(c.witness).unwrap();

        // Declared order: the pipeline as shipped.
        let declared: Vec<&RefinePass> = REFINE_PIPELINE.iter().collect();
        let mut in_order = base.clone();
        parser.refine_with_order(&declared, &mut in_order);

        // Swapped order: the same slice with `before`/`after` transposed.
        let mut swapped_passes = declared.clone();
        let bi = pass_index(c.before);
        let ai = pass_index(c.after);
        swapped_passes.swap(bi, ai);
        let mut swapped = base.clone();
        parser.refine_with_order(&swapped_passes, &mut swapped);

        assert_ne!(
            in_order, swapped,
            "constraint {:?} < {:?} is NOT load-bearing for witness {:?}: swapping \
             the two passes did not change the result, so the edge is dead \
             documentation. reason on file: {}",
            c.before, c.after, c.witness, c.reason
        );
    }
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
            .map(|m| vec![ModifierPart::Raw(m.to_string())])
            .unwrap_or_default(),
        optional: false,
    }
}

fn ing_with_amounts(name: &str, amounts: Vec<Measure>, modifier: Option<&str>) -> ParsedIngredient {
    ParsedIngredient {
        name: name.to_string(),
        amounts,
        modifier: modifier
            .map(|m| vec![ModifierPart::Raw(m.to_string())])
            .unwrap_or_default(),
        optional: false,
    }
}

/// A name that is exactly a known prep phrase swaps with the modifier; a
/// descriptive name is left alone (the exact-match guard).
#[rstest]
#[case::swaps(
    "finely chopped",
    Some("raw pistachios"),
    "raw pistachios",
    Some("finely chopped")
)]
#[case::no_swap_descriptive(
    "raw pistachios",
    Some("finely chopped"),
    "raw pistachios",
    Some("finely chopped")
)]
#[case::no_swap_no_modifier("chopped", None, "chopped", None)]
fn test_fix_leading_prep_phrase(
    #[case] name: &str,
    #[case] modifier: Option<&str>,
    #[case] want_name: &str,
    #[case] want_modifier: Option<&str>,
) {
    let parser = IngredientParser::new();
    let mut i = ing(name, modifier);
    parser.fix_leading_prep_phrase(&mut i);
    assert_eq!(i.name, want_name);
    assert_eq!(i.modifier_string().as_deref(), want_modifier);
}

/// "minus <measure> <name>" moves the subtractive clause to the modifier and
/// restores the real name.
#[test]
fn test_fix_leading_minus_clause() {
    let parser = IngredientParser::new();
    let mut i = ing("minus 1 tablespoon flour", None);
    parser.fix_leading_minus_clause(&mut i);
    assert_eq!(i.name, "flour");
    assert_eq!(i.modifier_string().as_deref(), Some("minus 1 tablespoon"));
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

#[rstest]
#[case::plain("thyme and/or rosemary", None, "thyme", Some("and/or rosemary"))]
#[case::before_raw(
    "cilantro and/or mint",
    Some("for serving"),
    "cilantro",
    Some("and/or mint, for serving")
)]
fn test_extract_and_or_alternative_from_name(
    #[case] name: &str,
    #[case] modifier: Option<&str>,
    #[case] want_name: &str,
    #[case] want_modifier: Option<&str>,
) {
    let parser = IngredientParser::new();
    let mut i = ing(name, modifier);
    parser.extract_and_or_alternative_from_name(&mut i);
    assert_eq!(i.name, want_name);
    assert_eq!(i.modifier_string().as_deref(), want_modifier);
}

#[rstest]
#[case::recovers_alias(
    "purple",
    Some("(red) cabbage (about 1 pound)"),
    "purple (red) cabbage",
    Some("(about 1 pound)")
)]
#[case::non_alias_amount_left_alone(
    "cabbage",
    Some("(about 1 pound)"),
    "cabbage",
    Some("(about 1 pound)")
)]
fn test_recover_parenthetical_alias_from_modifier(
    #[case] name: &str,
    #[case] modifier: Option<&str>,
    #[case] want_name: &str,
    #[case] want_modifier: Option<&str>,
) {
    let parser = IngredientParser::new();
    let mut i = ing(name, modifier);
    parser.recover_parenthetical_alias_from_modifier(&mut i);
    assert_eq!(i.name, want_name);
    assert_eq!(i.modifier_string().as_deref(), want_modifier);
}

/// "(about N unit)" in the modifier hoists a secondary amount; a distance
/// aside ("(about 3-inch)") is a shape descriptor and is left in place.
#[rstest]
#[case::hoists("chopped (about 2 cups)", 1)]
#[case::distance_kept("cut into (about 3-inch) strips", 0)]
// A bare trailing weight parenthetical hoists both measures (oz + g).
#[case::trailing_weight("coarsely chopped (2.1 oz / 60g)", 2)]
// A non-measure trailing parenthetical is left in place.
#[case::non_measure("chopped (softened)", 0)]
fn test_extract_secondary_amounts_from_modifier(
    #[case] modifier: &str,
    #[case] want_amounts: usize,
) {
    let parser = IngredientParser::new();
    let mut i = ing("scallions", Some(modifier));
    parser.extract_secondary_amounts_from_modifier(&mut i);
    assert_eq!(i.amounts.len(), want_amounts, "modifier: {modifier}");
}

/// A MID-modifier hoist must not leave a doubled internal space where the
/// parenthetical was excised (trim only fixes the ends).
#[test]
fn test_extract_secondary_amounts_mid_modifier_whitespace() {
    let parser = IngredientParser::new();
    let mut i = ing(
        "parsley",
        Some("chopped (about 2 cups) plus more for garnish"),
    );
    parser.extract_secondary_amounts_from_modifier(&mut i);
    assert_eq!(i.amounts.len(), 1);
    assert_eq!(
        i.modifier_string().as_deref(),
        Some("chopped plus more for garnish")
    );
}

/// A no-quantity "X or Y" alternative is split out of the name, with the head
/// noun reconstructed onto the primary when the left side is a lone adjective.
#[rstest]
// Lone adjective before "or": head noun shared onto the primary.
#[case::shared_head("red or white onion", "red onion", Some("or white onion"))]
#[case::shared_multiword_head(
    "fresh or frozen pitted sweet cherries",
    "fresh pitted sweet cherries",
    Some("or frozen pitted sweet cherries")
)]
// Distinct nouns (single- or multi-word left): primary = left, no reconstruct.
#[case::distinct_noun("flour or cornmeal", "flour", Some("or cornmeal"))]
#[case::multiword_left(
    "Nilla wafers or graham crackers",
    "Nilla wafers",
    Some("or graham crackers")
)]
// Guards: multi-coordination, prep adjective after "or", trailing stopword.
#[case::and_guard(
    "raw or roasted and salted shelled sunflower seeds",
    "raw or roasted and salted shelled sunflower seeds",
    None
)]
#[case::prep_adj_after_or("basil or chopped parsley", "basil", Some("or chopped parsley"))]
#[case::stopword_after_or("salt or pepper to taste", "salt", Some("or pepper to taste"))]
#[case::no_or("onion", "onion", None)]
// A size-word OR size-word pair is a size range of one ingredient, not a
// two-ingredient alternative — leave the name whole.
#[case::size_range("medium or large garlic clove", "medium or large garlic clove", None)]
// Path B: a trailing DISTRIBUTABLE_HEAD_NOUN distributes onto an open-ended
// left (no left-vocab match needed), including a multi-word left.
#[case::distribute_stock(
    "chicken or vegetable stock",
    "chicken stock",
    Some("or vegetable stock")
)]
#[case::distribute_mustard("grainy or Dijon mustard", "grainy mustard", Some("or Dijon mustard"))]
#[case::distribute_pepper("pink or black pepper", "pink pepper", Some("or black pepper"))]
#[case::distribute_multiword_left(
    "Little Gem or Bibb lettuce",
    "Little Gem lettuce",
    Some("or Bibb lettuce")
)]
// Guard: a head noun *not* in the list (oil/spirits) must not distribute —
// "butter" is a distinct ingredient, not a kind of oil.
#[case::distribute_excludes_oil("butter or olive oil", "butter", Some("or olive oil"))]
#[case::distribute_excludes_spirit("amaretto or dark rum", "amaretto", Some("or dark rum"))]
// Guard: a single-token right (the head noun itself) never distributes.
#[case::distribute_single_token_right("salt or pepper", "salt", Some("or pepper"))]
fn test_split_word_alternative(
    #[case] name: &str,
    #[case] want_name: &str,
    #[case] want_alternative: Option<&str>,
) {
    let parser = IngredientParser::new();
    let (got_name, got_alternative) =
        alternatives::split_word_alternative(name, &parser.adjectives);
    assert_eq!(got_name, want_name, "name: {name}");
    assert_eq!(got_alternative.as_deref(), want_alternative, "name: {name}");
}

/// A comma+or alternatives list whose shared head noun trails the final
/// option (stranded by the grammar's first-comma split) recovers the head
/// onto the single-token name; lists of complete ingredients are left alone.
#[rstest]
// Fires: bare options share the trailing head noun "oil".
#[case::oil(
    "canola",
    Some("vegetable, or melted coconut oil"),
    "canola oil",
    Some("or vegetable, or melted coconut oil")
)]
// Guard: final word isn't a curated shared head → no graft ("salt paprika").
#[case::complete_nouns("salt", Some("pepper, or paprika"), "salt", Some("pepper, or paprika"))]
#[case::baking_soda(
    "flour",
    Some("sugar, or baking soda"),
    "flour",
    Some("sugar, or baking soda")
)]
// Guard: no comma → just a two-way alternative, not a shared-head list.
#[case::no_comma("flour", Some("or oil"), "flour", Some("or oil"))]
// Guard: name already has a head noun (multi-token) → untouched.
#[case::multitoken_name(
    "olive oil",
    Some("vegetable, or canola oil"),
    "olive oil",
    Some("vegetable, or canola oil")
)]
fn test_recover_shared_head_from_alternatives(
    #[case] name: &str,
    #[case] modifier: Option<&str>,
    #[case] want_name: &str,
    #[case] want_modifier: Option<&str>,
) {
    let parser = IngredientParser::new();
    let mut i = ing(name, modifier);
    parser.recover_shared_head_from_alternatives(&mut i);
    assert_eq!(i.name, want_name, "name: {name}");
    assert_eq!(
        i.modifier_string().as_deref(),
        want_modifier,
        "name: {name}"
    );
}

/// The IR exposes a typed view of the modifier: extracted adjectives land in
/// `prep`, alternatives in `alternatives` — not a single opaque string.
#[test]
fn test_typed_modifier_view() {
    let parser = IngredientParser::new();

    let mut i = ing("chopped onion", None);
    parser.extract_adjectives_from_name(&mut i);
    assert_eq!(i.prep(), vec!["chopped"]);
    assert!(i.alternatives().is_empty());

    let mut i = ing("garlic or 1 teaspoon garlic powder", None);
    parser.extract_alternative_from_name(&mut i);
    assert_eq!(i.alternatives(), vec!["or 1 teaspoon garlic powder"]);
    assert!(i.prep().is_empty());
    // And it still flattens to the same modifier string.
    assert_eq!(
        Ingredient::from(i).modifier.as_deref(),
        Some("or 1 teaspoon garlic powder")
    );
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
    let (_, parsed) = parser.parse_ingredient(line).unwrap();

    let mut once = parsed.clone();
    parser.refine(&mut once);
    let mut twice = once.clone();
    parser.refine(&mut twice);

    assert_eq!(once, twice, "refine is not idempotent for {line:?}");
}
