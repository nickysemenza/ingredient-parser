use nom::{
    Parser,
    branch::alt,
    bytes::complete::{tag, tag_no_case},
    character::complete::{space0, space1},
    combinator::{opt, recognize},
    error::{ParseError, context},
    number::complete::double,
};

use crate::Res;

/// Every unicode vulgar-fraction glyph the parser recognizes, as a single string.
///
/// This is the one source of truth for the glyph set: `v_frac_to_num` (and thus
/// [`is_vulgar`]) must map exactly these, and the pre/post-parse regexes that need
/// a fraction character class build it from this const rather than re-listing the
/// glyphs (which previously drifted — the regexes had omitted `⅐ ⅑ ⅒`). Kept in
/// lockstep with `v_frac_to_num` by `tests::vulgar_fractions_match_is_vulgar`.
pub const VULGAR_FRACTIONS: &str = "¼½¾⅐⅑⅒⅓⅔⅕⅖⅗⅘⅙⅚⅛⅜⅝⅞";

/// The glyph ↔ (numerator, denominator) table. ONE table, read in both
/// directions: [`v_frac_to_num`] parses a glyph, [`glyph_for`] renders one.
///
/// Before this was shared, the renderer in `util.rs` kept its own list of 15
/// under a comment claiming it mirrored the parser's — which accepted 18. The
/// three it lacked (`⅐ ⅑ ⅒`) parsed in and came back out as `0.14`, and
/// nothing failed. `fraction_glyphs_round_trip` now closes that.
const VULGAR_TABLE: &[(char, i32, i32)] = &[
    ('¼', 1, 4),
    ('½', 1, 2),
    ('¾', 3, 4),
    ('⅐', 1, 7),
    ('⅑', 1, 9),
    ('⅒', 1, 10),
    ('⅓', 1, 3),
    ('⅔', 2, 3),
    ('⅕', 1, 5),
    ('⅖', 2, 5),
    ('⅗', 3, 5),
    ('⅘', 4, 5),
    ('⅙', 1, 6),
    ('⅚', 5, 6),
    ('⅛', 1, 8),
    ('⅜', 3, 8),
    ('⅝', 5, 8),
    ('⅞', 7, 8),
];

fn v_frac_to_num(input: char) -> Option<f64> {
    VULGAR_TABLE
        .iter()
        .find(|(c, _, _)| *c == input)
        .map(|(_, n, d)| *n as f64 / *d as f64)
}

/// The glyph for a fractional value in `0..1`, within `tolerance`. The render
/// direction of [`VULGAR_TABLE`] — `util::format_quantity` calls this rather
/// than keeping a second list.
pub fn glyph_for(frac: f64, tolerance: f64) -> Option<&'static str> {
    VULGAR_TABLE
        .iter()
        .find(|(_, n, d)| (frac - (*n as f64 / *d as f64)).abs() < tolerance)
        .map(|(c, _, _)| -> &'static str {
            // Each glyph is a single char; return it as a &'static str without
            // allocating by slicing the const table's own storage.
            match c {
                '¼' => "¼",
                '½' => "½",
                '¾' => "¾",
                '⅐' => "⅐",
                '⅑' => "⅑",
                '⅒' => "⅒",
                '⅓' => "⅓",
                '⅔' => "⅔",
                '⅕' => "⅕",
                '⅖' => "⅖",
                '⅗' => "⅗",
                '⅘' => "⅘",
                '⅙' => "⅙",
                '⅚' => "⅚",
                '⅛' => "⅛",
                '⅜' => "⅜",
                '⅝' => "⅝",
                _ => "⅞",
            }
        })
}

/// Whether `c` is a unicode vulgar-fraction glyph (½, ⅓, ¼, …).
pub fn is_vulgar(c: char) -> bool {
    v_frac_to_num(c).is_some()
}

/// parses unicode vulgar fractions
fn v_fraction(input: &str) -> Res<&str, f64> {
    match input
        .chars()
        .next()
        .and_then(|c| v_frac_to_num(c).map(|val| (c, val)))
    {
        Some((c, val)) => Ok((&input[c.len_utf8()..], val)),
        None => Err(nom::Err::Error(
            nom_language::error::VerboseError::from_error_kind(
                input,
                nom::error::ErrorKind::Satisfy,
            ),
        )),
    }
}
/// Parse a finite f64, rejecting the non-finite spellings `nom::double` accepts.
///
/// Rust's float parser (and thus `nom::double`) treats "inf", "infinity", and
/// "nan" as valid floats, so without this guard "inf/2" or "nan ½" would parse
/// as a numeric value. Reject any non-finite result so only real numbers parse.
/// Shared with `measurement::number`, which uses it for the plain-decimal path.
pub(crate) fn finite_double(input: &str) -> Res<&str, f64> {
    let (remaining, value) = double(input)?;
    if value.is_finite() {
        Ok((remaining, value))
    } else {
        Err(nom::Err::Error(
            nom_language::error::VerboseError::from_error_kind(input, nom::error::ErrorKind::Float),
        ))
    }
}

fn n_fraction(input: &str) -> Res<&str, f64> {
    context("n_fraction", (finite_double, tag("/"), finite_double))
        .parse(input)
        .and_then(|(next_input, res)| {
            if res.2 == 0.0 {
                Err(nom::Err::Error(
                    nom_language::error::VerboseError::from_error_kind(
                        input,
                        nom::error::ErrorKind::Verify,
                    ),
                ))
            } else {
                Ok((next_input, res.0 / res.2))
            }
        })
}

/// Parses mixed number formats like `1 ⅛` or `1 1/8` into `1.125`
///
/// This parser handles both unicode vulgar fractions and standard slash-notation fractions,
/// either alone or with a whole number component.
pub fn fraction_number(input: &str) -> Res<&str, f64> {
    use crate::traced_parser;

    // Separator between a whole number and a vulgar fraction: a spelled-out
    // "and" ("1 and ½"), or optional whitespace (the glyph can attach: "1½").
    // The "and" form is tried first so the space isn't consumed by `space0`.
    let whole_vulgar_sep = alt((recognize((space1, tag_no_case("and"), space1)), space0));

    // Define parser for unicode vulgar fractions with optional whole number
    let vulgar_fraction_parser = (
        opt((finite_double, whole_vulgar_sep)), // Optional whole number + separator
        v_fraction,                             // Unicode vulgar fraction like ½, ¼, etc.
    );

    // Separator between a whole number and a slash fraction: either plain
    // whitespace ("1 1/2") or a spelled-out "and" ("1 and 1/2"). The "and" form
    // is tried first so the leading space isn't consumed by `space1` alone.
    let whole_fraction_sep = alt((recognize((space1, tag_no_case("and"), space1)), space1));

    // Define parser for slash-notation fractions with optional whole number
    let slash_fraction_parser = (
        opt((finite_double, whole_fraction_sep)), // Optional whole number + separator
        n_fraction,                               // Standard fraction notation like 1/4, 3/8, etc.
    );

    traced_parser!(
        "fraction_number",
        input,
        context(
            "fraction_number",
            alt((vulgar_fraction_parser, slash_fraction_parser)),
        )
        .parse(input)
        .map(|(next_input, res)| {
            let (whole_number, fractional_part) = res;
            let whole_value = whole_number.map_or(0.0, |(num, _)| num);
            (next_input, whole_value + fractional_part)
        }),
        |v: &f64| format!("{v}"),
        "no fraction"
    )
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use nom::Err as NomErr;
    use nom::error::ErrorKind;
    use nom_language::error::{VerboseError, VerboseErrorKind};
    use rstest::rstest;

    use super::{VULGAR_FRACTIONS, fraction_number, glyph_for, v_frac_to_num};

    // ============================================================================
    // Unicode Vulgar Fraction Character Tests
    // ============================================================================

    #[rstest]
    #[case::half('½', 0.5)]
    #[case::quarter('¼', 0.25)]
    #[case::three_quarter('¾', 0.75)]
    #[case::eighth('⅛', 0.125)]
    #[case::three_eighths('⅜', 0.375)]
    #[case::five_eighths('⅝', 0.625)]
    #[case::seven_eighths('⅞', 0.875)]
    #[case::third('⅓', 1.0 / 3.0)]
    #[case::two_thirds('⅔', 2.0 / 3.0)]
    #[case::fifth('⅕', 0.2)]
    #[case::two_fifths('⅖', 0.4)]
    #[case::three_fifths('⅗', 0.6)]
    #[case::four_fifths('⅘', 0.8)]
    #[case::sixth('⅙', 1.0 / 6.0)]
    #[case::five_sixths('⅚', 5.0 / 6.0)]
    #[case::seventh('⅐', 1.0 / 7.0)]
    #[case::ninth('⅑', 1.0 / 9.0)]
    #[case::tenth('⅒', 0.1)]
    fn test_v_frac_to_num(#[case] char: char, #[case] expected: f64) {
        assert_eq!(v_frac_to_num(char), Some(expected));
    }

    #[test]
    fn test_v_frac_to_num_invalid() {
        assert_eq!(v_frac_to_num('x'), None);
        assert_eq!(v_frac_to_num('1'), None);
    }

    /// `VULGAR_FRACTIONS` and `is_vulgar` must agree exactly, so the regexes built
    /// from the const recognize precisely the glyphs the parser does. Both
    /// directions: every const glyph is vulgar, and no vulgar glyph (scanning the
    /// range that holds them all) is missing from the const.
    #[test]
    fn vulgar_fractions_match_is_vulgar() {
        use super::{VULGAR_FRACTIONS, is_vulgar};
        assert!(VULGAR_FRACTIONS.chars().all(is_vulgar));
        for c in '\u{0}'..='\u{2200}' {
            assert_eq!(
                is_vulgar(c),
                VULGAR_FRACTIONS.contains(c),
                "is_vulgar and VULGAR_FRACTIONS disagree on {c:?} (U+{:04X})",
                c as u32
            );
        }
    }

    // ============================================================================
    // Fraction Parser Tests - Unicode Fractions
    // ============================================================================

    #[rstest]
    #[case::half("½", 0.5)]
    #[case::quarter("¼", 0.25)]
    #[case::three_quarter("¾", 0.75)]
    #[case::eighth("⅛", 0.125)]
    #[case::three_eighths("⅜", 0.375)]
    #[case::five_eighths("⅝", 0.625)]
    #[case::seven_eighths("⅞", 0.875)]
    #[case::third("⅓", 1.0 / 3.0)]
    #[case::two_thirds("⅔", 2.0 / 3.0)]
    #[case::fifth("⅕", 0.2)]
    #[case::two_fifths("⅖", 0.4)]
    #[case::three_fifths("⅗", 0.6)]
    #[case::four_fifths("⅘", 0.8)]
    #[case::sixth("⅙", 1.0 / 6.0)]
    #[case::five_sixths("⅚", 5.0 / 6.0)]
    #[case::seventh("⅐", 1.0 / 7.0)]
    #[case::ninth("⅑", 1.0 / 9.0)]
    #[case::tenth("⅒", 0.1)]
    fn test_fraction_number_unicode(#[case] input: &str, #[case] expected: f64) {
        assert_eq!(fraction_number(input), Ok(("", expected)));
    }

    // ============================================================================
    // Fraction Parser Tests - Slash Notation
    // ============================================================================

    #[rstest]
    #[case::quarter("1/4", 0.25)]
    #[case::half("1/2", 0.5)]
    #[case::eighth("1/8", 0.125)]
    #[case::third("1/3", 1.0 / 3.0)]
    #[case::three_quarters("3/4", 0.75)]
    fn test_fraction_number_slash(#[case] input: &str, #[case] expected: f64) {
        assert_eq!(fraction_number(input), Ok(("", expected)));
    }

    // ============================================================================
    // Fraction Parser Tests - Mixed Numbers
    // ============================================================================

    #[rstest]
    #[case::one_and_eighth_unicode("1 ⅛", 1.125)]
    #[case::one_and_eighth_slash("1 1/8", 1.125)]
    #[case::one_and_third_no_space("1⅓", 1.0 + 1.0 / 3.0)]
    #[case::one_and_three_quarter("1¾", 1.75)]
    #[case::two_and_third("2 ⅓", 2.0 + 1.0 / 3.0)]
    #[case::three_and_fifth("3⅕", 3.2)]
    #[case::one_and_sixth("1 ⅙", 1.0 + 1.0 / 6.0)]
    #[case::two_and_seventh("2⅐", 2.0 + 1.0 / 7.0)]
    #[case::one_and_half_word("1 and 1/2", 1.5)]
    #[case::two_and_third_word("2 and 1/3", 2.0 + 1.0 / 3.0)]
    // The "and" separator works for vulgar glyphs too, not just slash form.
    #[case::one_and_half_vulgar("1 and ½", 1.5)]
    #[case::two_and_third_vulgar("2 and ⅓", 2.0 + 1.0 / 3.0)]
    fn test_fraction_number_mixed(#[case] input: &str, #[case] expected: f64) {
        assert_eq!(fraction_number(input), Ok(("", expected)));
    }

    // ============================================================================
    // Fraction Parser Tests - Error Cases
    // ============================================================================

    #[rstest]
    #[case::one_over_zero("1/0")]
    #[case::zero_over_zero("0/0")]
    fn test_fraction_zero_denominator(#[case] input: &str) {
        assert!(fraction_number(input).is_err(), "should reject {input}");
    }

    #[test]
    fn test_fraction_zero_numerator() {
        assert_eq!(fraction_number("0/1"), Ok(("", 0.0)));
    }

    /// `nom::double` accepts "inf"/"infinity"/"nan"; the fraction parsers must
    /// reject them (via `finite_double`) so "inf/2" or "nan ½" never parse as a
    /// numeric value. Regression for the finite-guard bypass.
    #[rstest]
    #[case::inf_numerator("inf/2")]
    #[case::infinity_numerator("infinity/2")]
    #[case::nan_numerator("nan/1")]
    #[case::inf_denominator("1/inf")]
    #[case::inf_whole_vulgar("inf ½")]
    #[case::inf_whole_slash("inf 1/2")]
    fn test_fraction_rejects_non_finite(#[case] input: &str) {
        assert!(fraction_number(input).is_err(), "should reject {input}");
    }

    #[test]
    fn test_fraction_number_error() {
        // Just a number without fraction should fail
        assert_eq!(
            fraction_number("1"),
            Err(NomErr::Error(VerboseError {
                errors: vec![
                    ("", VerboseErrorKind::Nom(ErrorKind::Tag)),
                    ("1", VerboseErrorKind::Context("n_fraction")),
                    ("1", VerboseErrorKind::Nom(ErrorKind::Alt)),
                    ("1", VerboseErrorKind::Context("fraction_number")),
                ]
            }))
        );
    }

    /// Every accepted glyph must render back as itself. This is the property
    /// the old two-list arrangement failed: `⅐ ⅑ ⅒` parsed in and came out as
    /// `0.14` / `0.11` / `0.1`, with every test still green because each list
    /// was only ever checked against itself.
    #[test]
    fn fraction_glyphs_round_trip() {
        for glyph in VULGAR_FRACTIONS.chars() {
            let Some(value) = v_frac_to_num(glyph) else {
                unreachable!("VULGAR_FRACTIONS glyph {glyph} must parse")
            };
            assert_eq!(
                glyph_for(value, 1e-6),
                Some(glyph.to_string().as_str()),
                "{glyph} parses to {value} but does not render back as itself"
            );
        }
    }

    /// And the round trip holds through the public formatter a `Measure`'s
    /// `Display` actually uses — the surface where the gap was visible.
    #[test]
    fn every_glyph_survives_format_quantity() {
        for glyph in VULGAR_FRACTIONS.chars() {
            let Some(value) = v_frac_to_num(glyph) else {
                unreachable!("VULGAR_FRACTIONS glyph {glyph} must parse")
            };
            assert_eq!(
                crate::util::format_quantity(value),
                glyph.to_string(),
                "format_quantity lost {glyph}"
            );
        }
    }
}
