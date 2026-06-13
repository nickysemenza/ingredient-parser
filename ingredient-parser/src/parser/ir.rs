//! Internal parse IR.
//!
//! The grammar and refine passes work on a `ParsedIngredient` rather than the
//! public [`Ingredient`]. Its modifier is a typed, ordered list of
//! [`ModifierPart`]s — preparation words, alternatives, and raw grammar text —
//! instead of an opaque string the passes pack into and re-parse. At the public
//! boundary it is lowered to [`Ingredient`] via `From`, which joins the parts
//! back into the modifier string exactly as the old `append_modifier` did (parts
//! joined by ", "), so the flattening is faithful and the public type is
//! unchanged.

use crate::unit::Measure;
use crate::usage::classify_usage;
use crate::Ingredient;

/// A single piece of an ingredient's modifier, tagged by what it represents.
/// The tag gives a structured view (see [`ParsedIngredient::prep`] etc.); the
/// order is preserved so lowering reproduces the original modifier string.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum ModifierPart {
    /// Free-form modifier text captured by the grammar (post-comma text,
    /// lifted asides, subtractive clauses).
    Raw(String),
    /// A preparation word/phrase extracted from the name ("chopped", "sifted").
    Prep(String),
    /// An alternative ingredient/measure ("or 1 tsp garlic powder").
    Alternative(String),
}

impl ModifierPart {
    pub(crate) fn text(&self) -> &str {
        match self {
            ModifierPart::Raw(s) | ModifierPart::Prep(s) | ModifierPart::Alternative(s) => s,
        }
    }
}

/// The parser's internal working representation, lowered to [`Ingredient`] at
/// the public boundary.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct ParsedIngredient {
    pub name: String,
    pub amounts: Vec<Measure>,
    pub modifier: Vec<ModifierPart>,
    pub optional: bool,
}

impl ParsedIngredient {
    /// Join the modifier parts into the single string form, trimming empties.
    /// Parts are comma-joined ("sifted, fresh"), except a parenthetical aside is
    /// space-joined to what precedes it ("minced (not too lean)") — the join the
    /// grammar's old `raw_modifier` did inline, relocated here now that prep
    /// extraction is a single owner producing separate parts.
    pub(crate) fn modifier_string(&self) -> Option<String> {
        // Parts are joined with ", " below, so a leading comma on any individual
        // part is always a stray grammar artifact — strip it defensively.
        let parts: Vec<&str> = self
            .modifier
            .iter()
            .map(|part| part.text().trim().trim_start_matches(',').trim())
            .filter(|text| !text.is_empty())
            .collect();
        if parts.is_empty() {
            return None;
        }
        let mut out = String::new();
        for (i, part) in parts.iter().enumerate() {
            if i > 0 {
                out.push_str(if part.starts_with('(') { " " } else { ", " });
            }
            out.push_str(part);
        }
        Some(out)
    }

    /// Append a part to the modifier (skips empty additions, like the old
    /// `append_modifier`).
    pub(crate) fn push_modifier(&mut self, part: ModifierPart) {
        if !part.text().trim().is_empty() {
            self.modifier.push(part);
        }
    }

    /// The preparation words, in order (structured view of the modifier).
    #[cfg(test)]
    pub(crate) fn prep(&self) -> Vec<&str> {
        self.modifier
            .iter()
            .filter_map(|p| match p {
                ModifierPart::Prep(s) => Some(s.as_str()),
                _ => None,
            })
            .collect()
    }

    /// The alternatives, in order (structured view of the modifier).
    #[cfg(test)]
    pub(crate) fn alternatives(&self) -> Vec<&str> {
        self.modifier
            .iter()
            .filter_map(|p| match p {
                ModifierPart::Alternative(s) => Some(s.as_str()),
                _ => None,
            })
            .collect()
    }
}

impl From<ParsedIngredient> for Ingredient {
    fn from(parsed: ParsedIngredient) -> Ingredient {
        let modifier = super::refine::strip_wrapping_parens(parsed.modifier_string());
        // Classified from name+modifier here; the pipeline re-classifies once
        // more with the raw line in hand (see `parse_pipeline_after_normalize`).
        let usage = classify_usage(&parsed.name, modifier.as_deref(), None, None);
        Ingredient {
            name: parsed.name,
            amounts: parsed.amounts,
            modifier,
            optional: parsed.optional,
            usage,
        }
    }
}
