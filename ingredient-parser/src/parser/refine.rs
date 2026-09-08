//! Name-local vocabulary resolution over the structurally assembled ingredient.
//! Structural ownership (clauses and parentheticals) belongs to `segment`.
//! Every name extraction transfers its source ownership before the next pass;
//! final modifier ordering follows source positions, not extraction order.

mod alternatives;
mod prep;
mod units;

use std::cmp::Reverse;

#[cfg(test)]
use super::ir::ModifierPart;
use super::ir::{ModifierKind, ParsedIngredient};
use crate::IngredientParser;
use crate::unit::{self, Measure};

impl IngredientParser {
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
        if !crate::trace::is_diagnostics_enabled() {
            run(self, parsed);
            return;
        }
        let before = parsed.clone();
        run(self, parsed);
        let changed = *parsed != before;
        crate::trace::trace_on_change(
            crate::trace::Stage::Refine,
            pass.id().as_str(),
            &before.name,
            &format!(
                "{} | {}",
                parsed.name,
                parsed.modifier_string().as_deref().unwrap_or("-")
            ),
            changed,
        );
    }
}

type Pass = fn(&IngredientParser, &mut ParsedIngredient);

crate::define_stage_pipeline! {
    // Every pass is an `Extract*` step (the lone non-`Extract` variant,
    // `CollapseName`, was removed as dead), so the shared prefix is intrinsic to
    // the pipeline rather than a naming smell.
    #[allow(clippy::enum_variant_names)]
    pub(super) enum PassId,
    pub(super) struct RefinePass,
    pub(super) const REFINE_PIPELINE: &[RefinePass],
    type Pass = Pass,
    trace: pub(crate) REFINE_TRACE_NAMES,
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
        ExtractPurposeGerund,
        "extract_purpose_gerund",
        IngredientParser::extract_purpose_gerund
    ),
    (
        ExtractAdjectivesFromName,
        "extract_adjectives_from_name",
        IngredientParser::extract_adjectives_from_name
    ),

}

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

#[cfg(test)]
mod tests;
