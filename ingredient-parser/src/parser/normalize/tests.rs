#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use rstest::rstest;

#[test]
fn rewrite_ids_are_unique() {
    crate::assert_stage_pipeline!(REWRITES);
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
#[case("\t2\tcups\nflour\r\n", "2 cups flour")]
#[case("•\t½\u{a0}cup\nflour①", "½ cup flour")]
#[case("flour (optional) (see page 12)", "flour (optional) (see page 12)")]
#[case(
    "2 batches of filling (2 pounds in total)",
    "2 batches of filling (2 pounds in total)"
)]
fn plain_and_mapped_lexical_paths_agree(#[case] input: &str, #[case] expected: &str) {
    assert_eq!(normalize_input(input), expected);
    let mapped = normalize_source(input);
    assert_eq!(mapped.text, expected);
    for (index, ch) in mapped.text.char_indices() {
        let origins = mapped.project(index..index + ch.len_utf8());
        assert!(!origins.is_empty());
        for origin in origins {
            assert!(input.is_char_boundary(origin.start));
            assert!(input.is_char_boundary(origin.end));
        }
    }
}

#[test]
fn mapped_repeated_words_keep_occurrence_identity() {
    let input = "• flour①\tflour";
    let mapped = normalize_source(input);
    assert_eq!(mapped.text, "flour flour");
    let first = input.find("flour").unwrap();
    let last = input.rfind("flour").unwrap();
    assert_eq!(mapped.project(0..5), vec![first..first + 5]);
    assert_eq!(mapped.project(6..11), vec![last..last + 5]);
}

#[test]
fn whitespace_collapse_does_not_reclaim_deleted_footnote() {
    let input = "flour ① flour";
    let source = normalize_source(input);
    assert_eq!(source.text, "flour flour");
    let marker = input.find('①').unwrap();
    assert!(
        source
            .project(0..source.text.len())
            .iter()
            .all(|range| { range.end <= marker || range.start >= marker + '①'.len_utf8() })
    );
}
