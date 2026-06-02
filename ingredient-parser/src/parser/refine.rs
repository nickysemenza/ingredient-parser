//! Post-parse refinement passes.
//!
//! After the grammar captures the raw shape, these passes recover misplaced
//! names, pull preparation adjectives and alternatives out of the name into the
//! modifier, and hoist secondary amounts. They run in a fixed, load-bearing
//! order (see `postprocess_ingredient`).

use std::cmp::Reverse;

use super::ir::{ModifierPart, ParsedIngredient};
use super::normalize::collapse_whitespace;
use crate::parser::MeasurementParser;
use crate::unit::{self, Measure};
use crate::{Ingredient, IngredientParser};

impl IngredientParser {
    /// Run the ordered refinement passes on the parsed IR, then lower it to the
    /// public [`Ingredient`] (which joins the typed modifier parts back into a
    /// string and finalizes it).
    pub(super) fn postprocess_ingredient(&self, mut parsed: ParsedIngredient) -> Ingredient {
        self.refine(&mut parsed);
        parsed.into()
    }

    /// Run the ordered refinement passes in place, without lowering. Split out so
    /// a caller that needs to append more modifier text *after* refinement (the
    /// inline-descriptive-paren path) can do so through the IR before lowering,
    /// rather than hand-joining the public modifier string.
    pub(super) fn refine(&self, parsed: &mut ParsedIngredient) {
        // When tracing, emit a node for each pass that actually changed the
        // ingredient (a before→after view) so the egui tree shows what each pass
        // did. The clone is gated behind the tracing flag, so the hot path stays
        // allocation-free.
        if crate::trace::is_tracing_enabled() {
            for (name, pass) in POST_PASSES {
                let before = parsed.clone();
                pass(self, parsed);
                if *parsed != before {
                    crate::trace::trace_enter(name, &before.name);
                    crate::trace::trace_exit_success(
                        0,
                        &format!(
                            "{} | {}",
                            parsed.name,
                            parsed.modifier_string().as_deref().unwrap_or("-")
                        ),
                    );
                }
            }
        } else {
            for (_name, pass) in POST_PASSES {
                pass(self, parsed);
            }
        }
    }

    /// Collapse runs of whitespace left in the name by earlier passes. A pass in
    /// its own right so the ordered `POST_PASSES` list stays the single source of
    /// truth for the sequence.
    fn collapse_name(&self, parsed: &mut ParsedIngredient) {
        parsed.name = collapse_whitespace(&parsed.name);
    }

    /// Recover from a leading prep phrase that displaced the ingredient name.
    ///
    /// A line like "2/3 cup finely chopped, raw pistachios" parses with the
    /// text *before* the comma as the name and the text *after* as the modifier,
    /// yielding name="finely chopped" / modifier="raw pistachios" — backwards.
    /// When the whole name is a single known prep phrase and a modifier is
    /// present, swap them so the prep phrase becomes the modifier and the real
    /// name is restored. The exact-match guard keeps descriptive names (e.g.
    /// "raw pistachios, finely chopped", where the name isn't a prep phrase) from
    /// ever being touched.
    fn fix_leading_prep_phrase(&self, parsed: &mut ParsedIngredient) {
        let name = parsed.name.trim();
        if name.is_empty() || !self.adjectives.contains(&name.to_lowercase()) {
            return;
        }
        let Some(modifier) = parsed.modifier_string() else {
            return;
        };
        let prep = name.to_string();
        parsed.name = modifier;
        parsed.modifier = vec![ModifierPart::Prep(prep)];
    }

    /// Recover from a leading subtractive clause that displaced the name, e.g.
    /// "½ cup minus 1 tablespoon flour" parses with "½ cup" as the amount and
    /// "minus 1 tablespoon flour" as the name. When the name begins with "minus"
    /// followed by a parseable measurement, move "minus <measure>" into the
    /// modifier and restore the real name ("flour"). The primary amount is left
    /// as stated (the subtraction isn't applied numerically).
    fn fix_leading_minus_clause(&self, parsed: &mut ParsedIngredient) {
        let name = parsed.name.clone();
        let Some(rest) = name
            .strip_prefix("minus ")
            .or_else(|| name.strip_prefix("Minus "))
        else {
            return;
        };
        let mp = MeasurementParser::new(&self.units, self.is_rich_text);
        let Ok((remaining, measures)) = mp.parse_measurement_list(rest) else {
            return;
        };
        if measures.is_empty() || remaining.trim().is_empty() {
            return;
        }
        let consumed = rest[..rest.len() - remaining.len()].trim();
        let clause = format!("minus {consumed}");
        parsed.name = remaining.trim().to_string();
        // Prepend the subtractive clause so it leads the modifier ("minus …, …").
        parsed.modifier.insert(0, ModifierPart::Raw(clause));
    }

    fn extract_adjectives_from_name(&self, parsed: &mut ParsedIngredient) {
        let mut name = parsed.name.clone();
        let mut name_lower = name.to_lowercase();
        let mut found_adjectives: Vec<&String> = self
            .adjectives
            .iter()
            .filter(|adj| name_lower.contains(adj.as_str()))
            .collect();
        found_adjectives.sort_by_key(|adj| Reverse(adj.len()));

        for adjective in found_adjectives {
            let Some(pos) = name_lower.find(adjective.as_str()) else {
                continue;
            };

            let end = pos + adjective.len();
            // `pos`/`end` are byte offsets into the lowercased name. Lowercasing
            // can change byte lengths for some Unicode (e.g. 'İ' -> "i̇"), so these
            // offsets may not fall on char boundaries in the original `name`.
            // Skip rather than panic when slicing `name` would split a char.
            if !name.is_char_boundary(pos) || !name.is_char_boundary(end) {
                continue;
            }

            // Require a whitespace/string-edge boundary on both sides, so an
            // adjective embedded in a larger token is left alone (e.g. "chopped"
            // inside "well-chopped" must not corrupt the name into "well-").
            let before_boundary = name[..pos]
                .chars()
                .next_back()
                .is_none_or(char::is_whitespace);
            let after_boundary = name[end..].chars().next().is_none_or(char::is_whitespace);
            if !before_boundary || !after_boundary {
                continue;
            }

            parsed.push_modifier(ModifierPart::Prep(adjective.clone()));

            let before = name[..pos].trim();
            let after = name[end..].trim();
            let mut new_name = String::with_capacity(name.len());
            if !before.is_empty() {
                new_name.push_str(before);
                if !after.is_empty() {
                    new_name.push(' ');
                }
            }
            if !after.is_empty() {
                new_name.push_str(after);
            }

            name = new_name.trim().to_string();
            name_lower = name.to_lowercase();
        }

        parsed.name = name;
    }

    /// Recover a leading preparation *alternative* that displaced the name, e.g.
    /// "grated or finely chopped lemon zest" parses with "grated or finely
    /// chopped lemon zest" as the name. When the name begins with
    /// "`<participle> or <known-adjective>`" — a prep word (typically `-ed`),
    /// "or", then a recognized adjective phrase — that whole prefix is a
    /// preparation note. Move it to the modifier and keep the trailing head noun
    /// as the name ("lemon zest", modifier "grated or finely chopped").
    ///
    /// Guarded tightly so genuine two-ingredient alternatives ("basil or chopped
    /// parsley") are left alone: the first word must look like a participle
    /// (`-ed`) or be a known adjective, the word after "or" must be a known
    /// adjective phrase, and a head noun must remain.
    fn extract_leading_prep_alternative(&self, parsed: &mut ParsedIngredient) {
        let name = parsed.name.trim().to_string();
        let words: Vec<&str> = name.split_whitespace().collect();
        if words.len() < 4 || words[1].to_lowercase() != "or" {
            return;
        }
        let first = words[0].to_lowercase();
        let first_is_prep = first.ends_with("ed") || self.adjectives.contains(&first);
        if !first.chars().all(char::is_alphabetic) || !first_is_prep {
            return;
        }
        // A known adjective phrase (two words then one) immediately after "or".
        let two = format!(
            "{} {}",
            words[2].to_lowercase(),
            words.get(3).map(|w| w.to_lowercase()).unwrap_or_default()
        );
        let adj_len = if words.len() >= 5 && self.adjectives.contains(&two) {
            2
        } else if self.adjectives.contains(&words[2].to_lowercase()) {
            1
        } else {
            return;
        };
        let name_start = 2 + adj_len;
        if name_start >= words.len() {
            return;
        }
        let prefix = words[..name_start].join(" ");
        parsed.name = words[name_start..].join(" ");
        parsed.push_modifier(ModifierPart::Prep(prefix));
    }

    fn extract_alternative_from_name(&self, parsed: &mut ParsedIngredient) {
        let (name, alternative) = extract_alternative(&parsed.name);
        parsed.name = name;
        if let Some(alternative) = alternative {
            parsed.push_modifier(ModifierPart::Alternative(alternative));
        }
    }

    fn extract_secondary_amounts_from_modifier(&self, parsed: &mut ParsedIngredient) {
        let Some(modifier) = parsed.modifier_string() else {
            return;
        };

        let (secondary_amounts, cleaned_modifier) =
            extract_secondary_amounts(&modifier, &self.units);
        // Only rewrite the modifier when an amount was actually hoisted; otherwise
        // leave the typed parts untouched (the cleaned string equals the original).
        if secondary_amounts.is_empty() {
            return;
        }
        parsed.amounts.extend(secondary_amounts);
        parsed.modifier = if cleaned_modifier.trim().is_empty() {
            Vec::new()
        } else {
            vec![ModifierPart::Raw(cleaned_modifier)]
        };
    }
}

/// A single post-parse refinement pass: a named mutation of the parsed
/// ingredient. `&IngredientParser` carries the parse context (units, adjectives,
/// rich-text mode) each pass needs.
type Pass = fn(&IngredientParser, &mut ParsedIngredient);

/// The ordered refinement pipeline. The order is load-bearing — e.g. whitespace
/// is collapsed *between* adjective and alternative extraction. The modifier is
/// finalized when the IR is lowered to `Ingredient`. Adding or reordering a step
/// is a one-line edit here.
const POST_PASSES: &[(&str, Pass)] = &[
    (
        "fix_leading_prep_phrase",
        IngredientParser::fix_leading_prep_phrase,
    ),
    (
        "fix_leading_minus_clause",
        IngredientParser::fix_leading_minus_clause,
    ),
    (
        "extract_leading_prep_alternative",
        IngredientParser::extract_leading_prep_alternative,
    ),
    (
        "extract_adjectives_from_name",
        IngredientParser::extract_adjectives_from_name,
    ),
    ("collapse_name", IngredientParser::collapse_name),
    (
        "extract_alternative_from_name",
        IngredientParser::extract_alternative_from_name,
    ),
    (
        "extract_secondary_amounts_from_modifier",
        IngredientParser::extract_secondary_amounts_from_modifier,
    ),
];

/// Strip a single pair of parentheses that wraps the *entire* modifier, e.g.
/// "(softened)" -> "softened". Modifiers with internal parentheses or only
/// partial wrapping are left untouched.
pub(super) fn strip_wrapping_parens(modifier: Option<String>) -> Option<String> {
    let modifier = modifier?;
    let trimmed = modifier.trim();
    if let Some(inner) = trimmed.strip_prefix('(').and_then(|s| s.strip_suffix(')')) {
        if !inner.contains('(') && !inner.contains(')') {
            let inner = inner.trim();
            return (!inner.is_empty()).then(|| inner.to_string());
        }
    }
    Some(modifier)
}

pub(super) fn clean_modifier(modifier: Option<String>) -> Option<String> {
    modifier.and_then(|modifier| {
        let trimmed = modifier.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

/// Extract alternative ingredients from the name (e.g., "garlic or 1 teaspoon garlic powder")
///
/// Returns `(cleaned_name, optional_alternative)` where:
/// - `cleaned_name`: The ingredient name with alternative removed
/// - `optional_alternative`: The alternative portion to be added to modifier
fn extract_alternative(name: &str) -> (String, Option<String>) {
    use regex::Regex;
    use std::sync::LazyLock;

    static ALTERNATIVE_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
        let frac = crate::fraction::VULGAR_FRACTIONS;
        #[allow(clippy::expect_used)]
        Regex::new(&format!(r"(?i)\s+or\s+(\d+|[{frac}]|a\s+|an\s+)"))
            .expect("invalid alternative pattern regex")
    });

    let Some(matched) = ALTERNATIVE_PATTERN.find(name) else {
        return (name.to_string(), None);
    };

    let (ingredient_part, alternative_part) = name.split_at(matched.start());
    let alternative = alternative_part.trim();
    if alternative.is_empty() {
        return (name.to_string(), None);
    }

    (
        ingredient_part.trim().to_string(),
        Some(alternative.to_string()),
    )
}

/// Extract secondary amounts from modifier patterns like "(from about 15 sprigs)".
///
/// Returns `(extracted_amounts, cleaned_modifier)` where:
/// - `extracted_amounts`: `Vec<Measure>` parsed from the pattern
/// - `cleaned_modifier`: The modifier with the pattern removed
fn extract_secondary_amounts(
    modifier: &str,
    units: &std::collections::HashSet<String>,
) -> (Vec<Measure>, String) {
    use regex::Regex;
    use std::sync::LazyLock;

    static SECONDARY_AMOUNT_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
        #[allow(clippy::expect_used)]
        Regex::new(r"\((?:from\s+)?(?:about|approximately|roughly|around)\s+([^)]+)\)")
            .expect("invalid secondary amount regex")
    });

    let Some(caps) = SECONDARY_AMOUNT_PATTERN.captures(modifier) else {
        return (vec![], modifier.to_string());
    };

    let Some(full_match) = caps.get(0) else {
        return (vec![], modifier.to_string());
    };
    let Some(amount_match) = caps.get(1) else {
        return (vec![], modifier.to_string());
    };
    let amount_text = amount_match.as_str().trim();

    let mp = MeasurementParser::new(units, false);
    let Ok((remaining, measures)) = mp.parse_measurement_list(amount_text) else {
        return (vec![], modifier.to_string());
    };

    // A *dimension* aside like "(about 3-inch)" inside a prep phrase ("cut into
    // long (about 3-inch) strips") describes shape, not a secondary quantity.
    // Leave it in the modifier rather than hoisting a spurious inch amount.
    let is_distance = |m: &Measure| match m.unit() {
        unit::Unit::Inch => true,
        unit::Unit::Other(s) => crate::parser::is_distance_unit(s),
        _ => false,
    };
    if measures.iter().any(is_distance) {
        return (vec![], modifier.to_string());
    }

    let remaining_trimmed = remaining.trim();
    let is_simple_remaining = remaining_trimmed.is_empty()
        || (remaining_trimmed.split_whitespace().count() == 1
            && remaining_trimmed.chars().all(char::is_alphabetic));

    if !is_simple_remaining || measures.is_empty() {
        return (vec![], modifier.to_string());
    }

    let cleaned = format!(
        "{}{}",
        &modifier[..full_match.start()],
        &modifier[full_match.end()..]
    )
    .trim()
    .to_string();

    (measures, cleaned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

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

    /// "(about N unit)" in the modifier hoists a secondary amount; a distance
    /// aside ("(about 3-inch)") is a shape descriptor and is left in place.
    #[rstest]
    #[case::hoists("chopped (about 2 cups)", 1)]
    #[case::distance_kept("cut into (about 3-inch) strips", 0)]
    fn test_extract_secondary_amounts_from_modifier(
        #[case] modifier: &str,
        #[case] want_amounts: usize,
    ) {
        let parser = IngredientParser::new();
        let mut i = ing("scallions", Some(modifier));
        parser.extract_secondary_amounts_from_modifier(&mut i);
        assert_eq!(i.amounts.len(), want_amounts, "modifier: {modifier}");
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
}
