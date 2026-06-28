#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use rstest::rstest;

#[test]
fn rewrite_ids_are_unique() {
    crate::assert_stage_pipeline!(REWRITES);
}

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
// The English preposition "in" must not read as the inch unit: only a
// number-adjacent "in"/"m" counts as a dimension.
#[case::preposition_in("tuna (packed in oil) drained", None)]
#[case::number_adjacent_in(
        "steak (1 in thick) trimmed",
        Some(("steak trimmed", "1 in thick"))
    )]
// Count + size ("4 (½-inch) slices"): digit before paren, not a name word.
#[case::count_size("4 (½-inch) slices pork", None)]
// Trailing paren (no name text after) → left for other paths.
#[case::trailing("warm water (100°F)", None)]
// Leading paren (optional-ingredient shape) → untouched.
#[case::leading("(70°F) water", None)]
fn test_lift_inline_descriptive_paren(#[case] input: &str, #[case] expected: Option<(&str, &str)>) {
    let got = lift_inline_descriptive_paren(input);
    assert_eq!(
        got,
        expected.map(|(c, a)| (c.to_string(), a.to_string())),
        "input: {input}"
    );
}

#[rstest]
#[case::nbsp("1\u{a0}cup\u{a0}flour", "1 cup flour")]
#[case::no_nbsp("1 cup flour", "1 cup flour")]
fn test_strip_nbsp(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(strip_nbsp(input), expected);
}

#[rstest]
// Circled-number footnote markers are dropped.
#[case::circled("rye flour \u{2460}", "rye flour ")]
#[case::dingbat("salt \u{2776}", "salt ")]
// No marker → passes through unchanged.
#[case::clean("rye flour", "rye flour")]
fn test_strip_footnote_markers(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(strip_footnote_markers(input), expected);
}

#[rstest]
// A trailing asterisk/dagger footnote marker is dropped.
#[case::asterisk("shredded zucchini (see note)*", "shredded zucchini (see note)")]
#[case::dagger("kosher salt \u{2020}", "kosher salt")]
#[case::double_dagger("flour\u{2021}", "flour")]
// A mid-name asterisk is NOT trailing → left alone.
#[case::mid("2% milk", "2% milk")]
// No marker → unchanged.
#[case::clean("rye flour", "rye flour")]
fn test_strip_trailing_footnote_markers(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(strip_trailing_footnote_markers(input), expected);
}

#[rstest]
// "the" directly before a quantity is a determiner → dropped.
#[case::the_fraction("the ¼ cup of garlic chives", "¼ cup of garlic chives")]
#[case::the_digit("the 2 cups flour", "2 cups flour")]
// "the" before a word is part of the name → left alone.
#[case::the_word("the works seasoning", "the works seasoning")]
fn test_strip_leading_determiner(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(strip_leading_determiner(input), expected);
}

#[rstest]
// An amount stated elsewhere → the redundant minus-paren is dropped.
#[case::redundant(
    "15 tablespoons (2 sticks minus 1 tablespoon) unsalted butter",
    "15 tablespoons unsalted butter"
)]
// Sole-quantity guard: when the minus-paren is the line's ONLY quantity it is
// KEPT, so the parse can surface `unparsed_digit` rather than silently zeroing
// the amount. (Regression for the minus-equivalence guard.)
#[case::sole_quantity(
    "butter (2 sticks minus 1 tablespoon)",
    "butter (2 sticks minus 1 tablespoon)"
)]
// No minus-paren at all → unchanged.
#[case::no_paren("2 sticks butter", "2 sticks butter")]
fn test_strip_minus_equivalence(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(strip_minus_equivalence(input), expected);
}

#[rstest]
// Parenthetical "(optional)" mid/end of a line → stripped, flagged optional.
#[case::paren("flour (optional)", "flour", true)]
// Trailing ", optional" word form → stripped, flagged.
#[case::trailing_word("toasted sesame seeds, optional", "toasted sesame seeds", true)]
// Whole-line "(optional)" is left for try_parse_optional_ingredient.
#[case::whole_line("(optional)", "(optional)", false)]
// No optional marker → unchanged, not flagged.
#[case::none("flour", "flour", false)]
fn test_strip_optional_note(
    #[case] input: &str,
    #[case] expected_text: &str,
    #[case] expected_flag: bool,
) {
    let (text, flag) = strip_optional_note(input);
    assert_eq!(text, expected_text, "text for: {input}");
    assert_eq!(flag, expected_flag, "flag for: {input}");
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

#[rstest]
// Single page reference (the original cases).
#[case::this_page("walnuts (this page)", "walnuts")]
#[case::see_this_page("walnuts (see this page),", "walnuts,")]
#[case::page_n("flour (page 123)", "flour")]
// Chains of page refs joined by connectors (Xi'an Famous Foods).
#[case::to_chain("8 dumplings (this page to this page)", "8 dumplings")]
#[case::or_list(
    "1 recipe filling (this page, this page, or this page)",
    "1 recipe filling"
)]
// A paren that MIXES a page ref with real content is left alone.
#[case::mixed(
    "8 cups broth (from Lamb Soup, this page)",
    "8 cups broth (from Lamb Soup, this page)"
)]
// A page ref mixed with "optional" is left by strip_cross_reference itself —
// split_crossref_optional (which runs first) reduces it to "(optional)".
#[case::with_optional(
    "XFF Chili Oil (this page; optional)",
    "XFF Chili Oil (this page; optional)"
)]
fn test_strip_cross_reference(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(
        strip_cross_reference(input).as_ref(),
        expected,
        "input: {input}"
    );
}

#[rstest]
#[case::semicolon("XFF Chili Oil (this page; optional)", "XFF Chili Oil (optional)")]
#[case::comma_see_page("flour (see page 12, optional)", "flour (optional)")]
// No "optional" → untouched (strip_cross_reference handles the pure ref).
#[case::pure_ref("walnuts (this page)", "walnuts (this page)")]
// No page ref → untouched (a bare "(optional)" is strip_optional_note's job).
#[case::pure_optional("walnuts (optional)", "walnuts (optional)")]
fn test_split_crossref_optional(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(
        split_crossref_optional(input).as_ref(),
        expected,
        "input: {input}"
    );
}

#[rstest]
// "(see note)" / "(note)" headnote cross-refs are dropped.
#[case::see_note("shredded zucchini (see note)", "shredded zucchini")]
#[case::bare_note("zucchini (note)", "zucchini")]
#[case::see_notes("zucchini (see notes)", "zucchini")]
// A paren carrying real content is left for the modifier.
#[case::real_content("zucchini (note the color)", "zucchini (note the color)")]
// No note paren → unchanged.
#[case::clean("shredded zucchini", "shredded zucchini")]
fn test_strip_note_reference(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(
        strip_note_reference(input).as_ref(),
        expected,
        "input: {input}"
    );
}
