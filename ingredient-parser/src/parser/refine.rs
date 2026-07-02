//! Post-parse refinement passes.
//!
//! After the grammar captures the raw shape, these passes recover misplaced
//! names, pull preparation adjectives and alternatives out of the name into the
//! modifier, and hoist secondary amounts. They run in a fixed, load-bearing
//! order (see `postprocess_ingredient`).
//!
//! That order is a *tested contract*, not a comment: [`ORDER_CONSTRAINTS`] lists
//! each load-bearing edge (`before` must precede `after`) together with a
//! `witness` — a real line that parses correctly in declared order and *wrong*
//! when the two passes are swapped. The `declared_order_matches_pipeline` and
//! `constraints_are_load_bearing` tests (in `refine/tests.rs`) enforce both
//! halves: the pipeline honours every edge, and every edge earns its place.

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
        let RefinePass { run, .. } = *pass;
        if !crate::trace::is_tracing_enabled() {
            run(self, parsed);
            return;
        }
        let before = parsed.clone();
        run(self, parsed);
        crate::trace::trace_on_change(
            pass.id().as_str(),
            &before.name,
            &format!(
                "{} | {}",
                parsed.name,
                parsed.modifier_string().as_deref().unwrap_or("-")
            ),
            *parsed != before,
        );
    }

    /// Collapse runs of whitespace left in the name by earlier passes. A pass in
    /// its own right so the ordered [`REFINE_PIPELINE`] list stays the single
    /// source of truth for the sequence.
    pub(super) fn collapse_name(&self, parsed: &mut ParsedIngredient) {
        parsed.name = collapse_whitespace(&parsed.name);
    }

    /// Run the refine passes in an arbitrary caller-supplied order. Test-only:
    /// [`ORDER_CONSTRAINTS`] uses this to run a witness once in declared order and
    /// once with two passes swapped, proving the edge changes the result.
    #[cfg(test)]
    pub(super) fn refine_with_order(&self, order: &[&RefinePass], parsed: &mut ParsedIngredient) {
        for pass in order {
            self.run_refine_pass(pass, parsed);
        }
    }
}

type Pass = fn(&IngredientParser, &mut ParsedIngredient);

crate::define_stage_pipeline! {
    pub(super) enum PassId,
    pub(super) struct RefinePass,
    pub(super) const REFINE_PIPELINE: &[RefinePass],
    type Pass = Pass,
    trace: pub(crate) REFINE_TRACE_NAMES,
    (
        FixLeadingPrepPhrase,
        "fix_leading_prep_phrase",
        IngredientParser::fix_leading_prep_phrase
    ),
    (
        FixLeadingMinusClause,
        "fix_leading_minus_clause",
        IngredientParser::fix_leading_minus_clause
    ),
    (
        ExtractPostfixProduceUnit,
        "extract_postfix_produce_unit",
        IngredientParser::extract_postfix_produce_unit
    ),
    (
        ExtractSizeUnitFromName,
        "extract_size_unit_from_name",
        IngredientParser::extract_size_unit_from_name
    ),
    (
        ExtractLeadingPrepAlternative,
        "extract_leading_prep_alternative",
        IngredientParser::extract_leading_prep_alternative
    ),
    (
        ExtractTrailingPrepClause,
        "extract_trailing_prep_clause",
        IngredientParser::extract_trailing_prep_clause
    ),
    (
        RecoverHeadNounFromModifier,
        "recover_head_noun_from_modifier",
        IngredientParser::recover_head_noun_from_modifier
    ),
    (
        ExtractAdjectivesFromName,
        "extract_adjectives_from_name",
        IngredientParser::extract_adjectives_from_name
    ),
    (CollapseName, "collapse_name", IngredientParser::collapse_name),
    (
        ExtractPurposeGerund,
        "extract_purpose_gerund",
        IngredientParser::extract_purpose_gerund
    ),
    (
        ExtractAlternativesFromName,
        "extract_alternatives_from_name",
        IngredientParser::extract_alternatives_from_name
    ),
    (
        RecoverParentheticalAliasFromModifier,
        "recover_parenthetical_alias_from_modifier",
        IngredientParser::recover_parenthetical_alias_from_modifier
    ),
    (
        RecoverSharedHeadFromAlternatives,
        "recover_shared_head_from_alternatives",
        IngredientParser::recover_shared_head_from_alternatives
    ),
    (
        ExtractSecondaryAmountsFromModifier,
        "extract_secondary_amounts_from_modifier",
        IngredientParser::extract_secondary_amounts_from_modifier
    ),
}

/// A load-bearing ordering edge in [`REFINE_PIPELINE`]: `before` must run before
/// `after`, for the reason given, and `witness` is a line that proves it — it
/// parses correctly in declared order and differently (wrong) when the two passes
/// are swapped. The pairing is verified from both sides by the tests in
/// `refine/tests.rs`; it exists only for those tests.
#[cfg(test)]
pub(super) struct OrderConstraint {
    pub before: PassId,
    pub after: PassId,
    pub reason: &'static str,
    /// A line that parses correctly in declared order and WRONG when the two
    /// passes are swapped — proves the edge is load-bearing.
    pub witness: &'static str,
}

/// The ordering edges the refine pipeline depends on. Each is enforced positionally
/// (`declared_order_matches_pipeline`) and behaviourally, via its witness
/// (`constraints_are_load_bearing`). Keep witnesses as real ingredient lines.
#[cfg(test)]
pub(super) const ORDER_CONSTRAINTS: &[OrderConstraint] = &[
    OrderConstraint {
        before: PassId::FixLeadingPrepPhrase,
        after: PassId::RecoverHeadNounFromModifier,
        reason: "resolve the vocab-adjective leading-prep case first, so a name \
                 that is a single known prep phrase is swapped with its modifier \
                 before the pure-prep-chain recovery can misfire on it",
        witness: "2 cups chopped, toasted walnuts",
    },
    OrderConstraint {
        before: PassId::ExtractTrailingPrepClause,
        after: PassId::ExtractAdjectivesFromName,
        reason: "move a trailing \"<participle> with/into …\" clause as one span \
                 before the adjective scan; the participle is a known adjective, so \
                 the scan would otherwise pull only that word and strand its \
                 prepositional tail in the name",
        witness: "2 cups spinach chopped into ribbons",
    },
    OrderConstraint {
        before: PassId::RecoverHeadNounFromModifier,
        after: PassId::ExtractAdjectivesFromName,
        reason: "recover the real head noun out of the modifier first, so the \
                 adjective scan runs over the restored name rather than a bare \
                 leading participle",
        witness: "1/2 cup deribbed, seeded, and roughly chopped fresh hot green chiles, such as serrano",
    },
    OrderConstraint {
        before: PassId::ExtractLeadingPrepAlternative,
        after: PassId::ExtractAdjectivesFromName,
        reason: "peel a leading \"<participle> or <adj>\" prep alternative before \
                 the adjective scan, so the shared prep words leave as one clause \
                 instead of being split across name and modifier",
        witness: "1 teaspoon grated or finely chopped lemon zest",
    },
    OrderConstraint {
        before: PassId::ExtractAdjectivesFromName,
        after: PassId::ExtractAlternativesFromName,
        reason: "a leading prep adjective must leave the name before alternative \
                 extraction, so the alternatives pass sees the bare \
                 \"<premod> or <premod> <head>\" and can reconstruct the shared head \
                 (\"red onion\") instead of splitting on the adjective-laden left",
        witness: "chopped red or white onion",
    },
    OrderConstraint {
        before: PassId::ExtractAdjectivesFromName,
        after: PassId::ExtractPurposeGerund,
        reason: "strip fixed vocab purpose phrases (\"for garnish\") from the name \
                 first, so the gerund pass's leftmost-\" for \" match lands on the \
                 real \"for <gerund>\" tail instead of stalling on the vocab phrase",
        witness: "chopped parsley for garnish for brushing the bread",
    },
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
mod tests;
