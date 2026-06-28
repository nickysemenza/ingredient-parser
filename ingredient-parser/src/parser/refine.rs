//! Post-parse refinement passes.
//!
//! After the grammar captures the raw shape, these passes recover misplaced
//! names, pull preparation adjectives and alternatives out of the name into the
//! modifier, and hoist secondary amounts. They run in a fixed, load-bearing
//! order (see `postprocess_ingredient`).

mod alternatives;
mod amounts;
mod prep;
mod recover;
mod units;

use std::cmp::Reverse;

use super::ir::{ModifierPart, ParsedIngredient};
use super::normalize::collapse_whitespace;
use crate::parser::{MeasurementMode, MeasurementParser};
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
        for pass in REFINE_PIPELINE {
            self.run_refine_pass(pass, parsed);
        }
    }

    fn run_refine_pass(&self, pass: &RefinePass, parsed: &mut ParsedIngredient) {
        let RefinePass {
            id,
            phase: _phase,
            run,
        } = *pass;
        if crate::trace::is_tracing_enabled() {
            let before = parsed.clone();
            run(self, parsed);
            if *parsed != before {
                crate::trace::trace_enter(id.as_str(), &before.name);
                crate::trace::trace_exit_success(
                    0,
                    &format!(
                        "{} | {}",
                        parsed.name,
                        parsed.modifier_string().as_deref().unwrap_or("-")
                    ),
                );
            }
        } else {
            run(self, parsed);
        }
    }

    /// Collapse runs of whitespace left in the name by earlier passes. A pass in
    /// its own right so the ordered [`REFINE_PIPELINE`] list stays the single
    /// source of truth for the sequence.
    pub(super) fn collapse_name(&self, parsed: &mut ParsedIngredient) {
        parsed.name = collapse_whitespace(&parsed.name);
    }
}

type Pass = fn(&IngredientParser, &mut ParsedIngredient);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum PassId {
    FixLeadingPrepPhrase,
    FixLeadingMinusClause,
    ExtractPostfixProduceUnit,
    ExtractSizeUnitFromName,
    ExtractLeadingPrepAlternative,
    ExtractTrailingPrepClause,
    RecoverHeadNounFromModifier,
    ExtractAdjectivesFromName,
    CollapseName,
    ExtractPurposeGerund,
    ExtractAlternativeFromName,
    ExtractWordAlternativeFromName,
    ExtractAndOrAlternativeFromName,
    RecoverParentheticalAliasFromModifier,
    RecoverSharedHeadFromAlternatives,
    ExtractSecondaryAmountsFromModifier,
}

impl PassId {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            PassId::FixLeadingPrepPhrase => "fix_leading_prep_phrase",
            PassId::FixLeadingMinusClause => "fix_leading_minus_clause",
            PassId::ExtractPostfixProduceUnit => "extract_postfix_produce_unit",
            PassId::ExtractSizeUnitFromName => "extract_size_unit_from_name",
            PassId::ExtractLeadingPrepAlternative => "extract_leading_prep_alternative",
            PassId::ExtractTrailingPrepClause => "extract_trailing_prep_clause",
            PassId::RecoverHeadNounFromModifier => "recover_head_noun_from_modifier",
            PassId::ExtractAdjectivesFromName => "extract_adjectives_from_name",
            PassId::CollapseName => "collapse_name",
            PassId::ExtractPurposeGerund => "extract_purpose_gerund",
            PassId::ExtractAlternativeFromName => "extract_alternative_from_name",
            PassId::ExtractWordAlternativeFromName => "extract_word_alternative_from_name",
            PassId::ExtractAndOrAlternativeFromName => "extract_and_or_alternative_from_name",
            PassId::RecoverParentheticalAliasFromModifier => {
                "recover_parenthetical_alias_from_modifier"
            }
            PassId::RecoverSharedHeadFromAlternatives => "recover_shared_head_from_alternatives",
            PassId::ExtractSecondaryAmountsFromModifier => {
                "extract_secondary_amounts_from_modifier"
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum RefinePhase {
    Recover,
    Units,
    Prep,
    Alternatives,
    Amounts,
    Cleanup,
}

#[derive(Clone, Copy)]
pub(super) struct RefinePass {
    id: PassId,
    phase: RefinePhase,
    run: Pass,
}

impl RefinePass {
    const fn new(id: PassId, phase: RefinePhase, run: Pass) -> Self {
        Self { id, phase, run }
    }
}

/// The ordered refinement pipeline. The order is load-bearing — e.g. whitespace
/// is collapsed *between* adjective and alternative extraction. The modifier is
/// finalized when the IR is lowered to `Ingredient`. Adding or reordering a step
/// is a one-line edit here.
///
/// Critical ordering dependencies (see `refine_pipeline_order_invariants` test):
/// - `ExtractAdjectivesFromName` before alternative passes — prep words must leave
///   the name before "or …" / "and …" alternative extraction runs.
/// - `CollapseName` between adjective and alternative extraction — whitespace
///   normalization must not run before adjectives are peeled.
/// - `ExtractSecondaryAmountsFromModifier` last — hoists parenthetical amounts
///   after all modifier text shaping is finished.
pub(super) const REFINE_PIPELINE: &[RefinePass] = &[
    RefinePass::new(
        PassId::FixLeadingPrepPhrase,
        RefinePhase::Recover,
        IngredientParser::fix_leading_prep_phrase,
    ),
    RefinePass::new(
        PassId::FixLeadingMinusClause,
        RefinePhase::Recover,
        IngredientParser::fix_leading_minus_clause,
    ),
    RefinePass::new(
        PassId::ExtractPostfixProduceUnit,
        RefinePhase::Units,
        IngredientParser::extract_postfix_produce_unit,
    ),
    RefinePass::new(
        PassId::ExtractSizeUnitFromName,
        RefinePhase::Units,
        IngredientParser::extract_size_unit_from_name,
    ),
    RefinePass::new(
        PassId::ExtractLeadingPrepAlternative,
        RefinePhase::Alternatives,
        IngredientParser::extract_leading_prep_alternative,
    ),
    RefinePass::new(
        PassId::ExtractTrailingPrepClause,
        RefinePhase::Prep,
        IngredientParser::extract_trailing_prep_clause,
    ),
    RefinePass::new(
        PassId::RecoverHeadNounFromModifier,
        RefinePhase::Recover,
        IngredientParser::recover_head_noun_from_modifier,
    ),
    RefinePass::new(
        PassId::ExtractAdjectivesFromName,
        RefinePhase::Prep,
        IngredientParser::extract_adjectives_from_name,
    ),
    RefinePass::new(
        PassId::CollapseName,
        RefinePhase::Cleanup,
        IngredientParser::collapse_name,
    ),
    RefinePass::new(
        PassId::ExtractPurposeGerund,
        RefinePhase::Prep,
        IngredientParser::extract_purpose_gerund,
    ),
    RefinePass::new(
        PassId::ExtractAlternativeFromName,
        RefinePhase::Alternatives,
        IngredientParser::extract_alternative_from_name,
    ),
    RefinePass::new(
        PassId::ExtractWordAlternativeFromName,
        RefinePhase::Alternatives,
        IngredientParser::extract_word_alternative_from_name,
    ),
    RefinePass::new(
        PassId::ExtractAndOrAlternativeFromName,
        RefinePhase::Alternatives,
        IngredientParser::extract_and_or_alternative_from_name,
    ),
    RefinePass::new(
        PassId::RecoverParentheticalAliasFromModifier,
        RefinePhase::Recover,
        IngredientParser::recover_parenthetical_alias_from_modifier,
    ),
    RefinePass::new(
        PassId::RecoverSharedHeadFromAlternatives,
        RefinePhase::Alternatives,
        IngredientParser::recover_shared_head_from_alternatives,
    ),
    RefinePass::new(
        PassId::ExtractSecondaryAmountsFromModifier,
        RefinePhase::Amounts,
        IngredientParser::extract_secondary_amounts_from_modifier,
    ),
];

/// Strip a single pair of parentheses that wraps the *entire* modifier, e.g.
/// "(softened)" -> "softened". Modifiers with internal parentheses or only
/// partial wrapping are left untouched.
pub(super) fn strip_wrapping_parens(modifier: Option<String>) -> Option<String> {
    let modifier = modifier?;
    let trimmed = modifier.trim();
    if let Some(inner) = trimmed.strip_prefix('(').and_then(|s| s.strip_suffix(')'))
        && !inner.contains('(')
        && !inner.contains(')')
    {
        let inner = inner.trim();
        return (!inner.is_empty()).then(|| inner.to_string());
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use rstest::rstest;
    use std::collections::HashSet;

    const EXPECTED_PIPELINE: &[(PassId, RefinePhase)] = &[
        (PassId::FixLeadingPrepPhrase, RefinePhase::Recover),
        (PassId::FixLeadingMinusClause, RefinePhase::Recover),
        (PassId::ExtractPostfixProduceUnit, RefinePhase::Units),
        (PassId::ExtractSizeUnitFromName, RefinePhase::Units),
        (
            PassId::ExtractLeadingPrepAlternative,
            RefinePhase::Alternatives,
        ),
        (PassId::ExtractTrailingPrepClause, RefinePhase::Prep),
        (PassId::RecoverHeadNounFromModifier, RefinePhase::Recover),
        (PassId::ExtractAdjectivesFromName, RefinePhase::Prep),
        (PassId::CollapseName, RefinePhase::Cleanup),
        (PassId::ExtractPurposeGerund, RefinePhase::Prep),
        (
            PassId::ExtractAlternativeFromName,
            RefinePhase::Alternatives,
        ),
        (
            PassId::ExtractWordAlternativeFromName,
            RefinePhase::Alternatives,
        ),
        (
            PassId::ExtractAndOrAlternativeFromName,
            RefinePhase::Alternatives,
        ),
        (
            PassId::RecoverParentheticalAliasFromModifier,
            RefinePhase::Recover,
        ),
        (
            PassId::RecoverSharedHeadFromAlternatives,
            RefinePhase::Alternatives,
        ),
        (
            PassId::ExtractSecondaryAmountsFromModifier,
            RefinePhase::Amounts,
        ),
    ];

    const EXPECTED_TRACE_LABELS: &[&str] = &[
        "fix_leading_prep_phrase",
        "fix_leading_minus_clause",
        "extract_postfix_produce_unit",
        "extract_size_unit_from_name",
        "extract_leading_prep_alternative",
        "extract_trailing_prep_clause",
        "recover_head_noun_from_modifier",
        "extract_adjectives_from_name",
        "collapse_name",
        "extract_purpose_gerund",
        "extract_alternative_from_name",
        "extract_word_alternative_from_name",
        "extract_and_or_alternative_from_name",
        "recover_parenthetical_alias_from_modifier",
        "recover_shared_head_from_alternatives",
        "extract_secondary_amounts_from_modifier",
    ];

    #[test]
    fn refine_pipeline_order_is_locked() {
        let actual: Vec<_> = REFINE_PIPELINE
            .iter()
            .map(|pass| (pass.id, pass.phase))
            .collect();
        assert_eq!(actual, EXPECTED_PIPELINE);
    }

    #[test]
    fn refine_pipeline_pass_ids_are_unique() {
        let ids: HashSet<_> = REFINE_PIPELINE.iter().map(|pass| pass.id).collect();
        assert_eq!(ids.len(), REFINE_PIPELINE.len());
    }

    #[test]
    fn refine_pipeline_trace_labels_are_stable() {
        let labels: Vec<_> = REFINE_PIPELINE
            .iter()
            .map(|pass| pass.id.as_str())
            .collect();
        assert_eq!(labels, EXPECTED_TRACE_LABELS);
    }

    fn pass_index(id: PassId) -> usize {
        REFINE_PIPELINE
            .iter()
            .position(|pass| pass.id == id)
            .expect("REFINE_PIPELINE missing expected pass")
    }

    #[test]
    fn refine_pipeline_order_invariants() {
        // Prep adjectives must leave the name before any alternative extraction.
        assert!(
            pass_index(PassId::ExtractAdjectivesFromName)
                < pass_index(PassId::ExtractAlternativeFromName),
            "adjectives before alternatives"
        );
        assert!(
            pass_index(PassId::ExtractAdjectivesFromName)
                < pass_index(PassId::ExtractWordAlternativeFromName),
            "adjectives before word alternatives"
        );
        assert!(
            pass_index(PassId::ExtractAdjectivesFromName)
                < pass_index(PassId::ExtractAndOrAlternativeFromName),
            "adjectives before and/or alternatives"
        );
        // Whitespace collapse sits between adjective peel and alternative passes.
        assert!(
            pass_index(PassId::ExtractAdjectivesFromName) < pass_index(PassId::CollapseName),
            "adjectives before collapse"
        );
        assert!(
            pass_index(PassId::CollapseName) < pass_index(PassId::ExtractAlternativeFromName),
            "collapse before alternatives"
        );
        // Parenthetical amount hoists run after modifier text is fully shaped.
        assert!(
            pass_index(PassId::ExtractSecondaryAmountsFromModifier) == REFINE_PIPELINE.len() - 1,
            "secondary amounts must be last"
        );
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
    // left in the name (it's really two ingredients — a parse_multi concern)…
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
    #[case::distribute_mustard(
        "grainy or Dijon mustard",
        "grainy mustard",
        Some("or Dijon mustard")
    )]
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

        let mut i = ParsedIngredient {
            name: "medium garlic clove".into(),
            amounts: vec![Measure::new("whole", 1.0)],
            modifier: vec![],
            optional: false,
        };
        parser.extract_postfix_produce_unit(&mut i);
        assert_eq!(i.name, "garlic");
        assert_eq!(i.amounts, vec![Measure::new("clove", 1.0)]);
        assert_eq!(i.modifier_string().as_deref(), Some("medium"));

        // Idiom guard: cinnamon isn't a produce food, so "cinnamon stick" stays.
        let mut i = ParsedIngredient {
            name: "cinnamon stick".into(),
            amounts: vec![Measure::new("whole", 1.0)],
            modifier: vec![],
            optional: false,
        };
        parser.extract_postfix_produce_unit(&mut i);
        assert_eq!(i.name, "cinnamon stick");
        assert_eq!(i.amounts, vec![Measure::new("whole", 1.0)]);

        // A real volume/weight lead (not a plain count) → don't fire.
        let mut i = ParsedIngredient {
            name: "garlic clove".into(),
            amounts: vec![Measure::new("cup", 1.0)],
            modifier: vec![],
            optional: false,
        };
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
            let mut i = ParsedIngredient {
                name: name.into(),
                amounts,
                modifier: vec![],
                optional: false,
            };
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
    fn refine_pipeline_is_idempotent(#[case] line: &str) {
        let parser = IngredientParser::new();
        let (_, parsed) = parser.parse_ingredient(line).unwrap();

        let mut once = parsed.clone();
        parser.refine(&mut once);
        let mut twice = once.clone();
        parser.refine(&mut twice);

        assert_eq!(once, twice, "refine is not idempotent for {line:?}");
    }
}
