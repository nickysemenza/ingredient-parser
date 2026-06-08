#[allow(deprecated)]
use nom::{
    bytes::complete::tag,
    character::complete::{not_line_ending, space0},
    combinator::opt,
    error::context,
    multi::many1,
    Parser,
};

use super::ir::{ModifierPart, ParsedIngredient};
use super::normalize::{lift_inline_descriptive_paren, normalize_input, strip_optional_note};
use super::refine::clean_modifier;
use crate::parser::{parse_ingredient_text, MeasurementParser, Res};
use crate::trace;
use crate::traced_parser;
use crate::unit::Measure;
use crate::{Ingredient, IngredientParser};

impl IngredientParser {
    pub(crate) fn parse_ingredient_line(&self, input: &str) -> Ingredient {
        let normalized = normalize_input(input);
        self.parse_normalized_ingredient(normalized.as_ref())
    }

    /// Parse a line and report whether it fell back to a name-only ingredient.
    pub(crate) fn parse_ingredient_line_with_provenance(&self, input: &str) -> (Ingredient, bool) {
        let normalized = normalize_input(input);
        self.parse_normalized_ingredient_with_provenance(normalized.as_ref())
    }

    pub(crate) fn parse_ingredient_line_with_trace(
        &self,
        input: &str,
    ) -> trace::ParseWithTrace<Ingredient> {
        trace::enable_tracing();
        // Open the root span with the *raw* input so the normalize rewrites nest
        // under it as the first stage (the non-trace paths normalize before the
        // span, where tracing is off and it's a no-op). The rest of the pipeline
        // (recognizers, grammar, refine passes) then attaches as later children.
        trace::trace_enter("parse_line", input);
        let normalized = normalize_input(input);
        let normalized = normalized.as_ref();
        let (result, _fell_back) = self.parse_pipeline_after_normalize(normalized);
        trace::trace_exit_success(0, &result.name);
        let trace = trace::disable_tracing(normalized);

        trace::ParseWithTrace {
            result: Ok(result),
            trace,
        }
    }

    fn parse_normalized_ingredient(&self, input: &str) -> Ingredient {
        self.parse_normalized_ingredient_with_provenance(input).0
    }

    /// Like `parse_normalized_ingredient`, but also reports whether the parse
    /// fell back to a name-only ingredient (no structured recognizer/core parse
    /// succeeded). Used to derive parse diagnostics.
    pub(crate) fn parse_normalized_ingredient_with_provenance(
        &self,
        input: &str,
    ) -> (Ingredient, bool) {
        // Wrap the whole parse in one root span so the phase spans
        // (recognizers, the grammar, refine passes) nest under it and the trace
        // tree has a single root. No-op when tracing is disabled. The traced
        // entry point (`parse_ingredient_line_with_trace`) opens this span itself
        // — around normalize — and calls `parse_pipeline_after_normalize`
        // directly, so the span is never entered twice.
        trace::trace_enter("parse_line", input);
        let result = self.parse_pipeline_after_normalize(input);
        trace::trace_exit_success(0, &result.0.name);
        result
    }

    /// The post-normalize pipeline body: strip a whole-ingredient "(optional)"
    /// note, run the recognizers/grammar/refine, and set the optional flag.
    fn parse_pipeline_after_normalize(&self, input: &str) -> (Ingredient, bool) {
        // An "(optional)" note marks the whole ingredient optional, e.g.
        // "Grated zest of 1 lemon (optional)" or, mid-line, "almonds (optional),
        // coarsely chopped". Strip it before parsing and set the flag, so it
        // neither pollutes the name/modifier nor blocks a trailing weight from
        // being hoisted. (A *whole-line* parenthesized ingredient is handled
        // separately below.)
        let (cleaned, is_optional) = strip_optional_note(input);
        let (mut ingredient, fell_back) = self.parse_normalized_ingredient_inner(&cleaned);
        if is_optional {
            ingredient.optional = true;
        }
        (ingredient, fell_back)
    }

    /// Returns the parsed ingredient and `true` if it came from the name-only
    /// fallback (no recognizer or core parse succeeded).
    fn parse_normalized_ingredient_inner(&self, input: &str) -> (Ingredient, bool) {
        // First try the whole-line special-form recognizers (first match wins),
        // then fall back to the general core parse, then to a name-only ingredient.
        self.run_recognizers(input)
            .or_else(|| {
                self.parse_core_ingredient(input)
                    // Reject a "successful" parse that lost the ingredient name
                    // into the modifier (seen on real recipes: a decimal comma in
                    // "1,000 grams ... nectarines", a leading prep word, etc.) —
                    // the graceful fallback is better than a name-less ingredient
                    // with garbled text. A bare quantity like "1/2-1 cup"
                    // legitimately has no name, so only fall back when the empty
                    // name coincides with leftover modifier text.
                    .filter(|ingredient| {
                        let name_empty = ingredient.name.trim().is_empty();
                        let has_modifier = ingredient
                            .modifier
                            .as_deref()
                            .is_some_and(|m| !m.trim().is_empty());
                        !(name_empty && has_modifier)
                    })
            })
            .map(|ingredient| (ingredient, false))
            .unwrap_or_else(|| (fallback_ingredient(input), true))
    }

    pub(super) fn parse_core_ingredient(&self, input: &str) -> Option<Ingredient> {
        // A descriptive parenthetical sitting *between* name words — e.g. the
        // "(70° to 80°F)" in "room-temperature (70° to 80°F) water" or the
        // "(¼ inch / 6 mm)" in "sliced (¼ inch / 6 mm) green onions" — breaks the
        // name grammar. Lift it out to the modifier and parse the cleaned line,
        // so the real name and amounts survive. Scoped to temperature/distance
        // asides flanked by name text, so mass/volume parentheticals like
        // "(190 grams)" stay hoisted as amounts and "4 (½-inch) slices" (count +
        // size) is untouched.
        if let Some((cleaned, aside)) = lift_inline_descriptive_paren(input) {
            let (_, mut parsed) = self.parse_ingredient(&cleaned).ok()?;
            // Refine first, then append the lifted aside as the trailing modifier
            // part — so it lands *after* any prep adjective the refine passes
            // extract (e.g. "sliced, ¼ inch / 6 mm"), and is joined/finalized
            // through the IR's single lowering path.
            self.refine(&mut parsed);
            parsed.push_modifier(ModifierPart::Raw(aside));
            return Some(parsed.into());
        }

        self.parse_ingredient(input)
            .ok()
            .map(|(_, ingredient)| self.postprocess_ingredient(ingredient))
    }

    /// Parse a complete ingredient line including amounts, name, and modifiers.
    ///
    /// This method only captures the raw grammar shape. Cleanup such as adjective
    /// extraction, alternative extraction, and secondary amount extraction happens
    /// in the higher-level ingredient pipeline.
    #[tracing::instrument(name = "parse_ingredient")]
    pub(crate) fn parse_ingredient<'a>(&self, input: &'a str) -> Res<&'a str, ParsedIngredient> {
        let mp = MeasurementParser::new(&self.units, false);

        // NOTE: a leading preparation adjective ("1 cup chopped onion") is NOT
        // consumed here — it stays in the name chunks and is extracted into the
        // modifier by the single owner of prep extraction, `refine`'s
        // `extract_adjectives_from_name`/`fix_leading_prep_phrase`. The grammar
        // used to peel a leading adjective separately, which double-handled prep
        // words with refine's name scan; collapsing to one owner removed that.
        let ingredient_format = (
            opt(|a| mp.parse_measurement_list(a)),
            space0,
            opt(|a| mp.parse_bracketed_amounts(a)),
            space0,
            opt(many1(parse_ingredient_text)),
            opt(|a| mp.parse_parenthesized_amounts(a)),
            opt(tag(", ")),
            not_line_ending,
        );

        traced_parser!(
            "parse_ingredient",
            input,
            context("ingredient", ingredient_format).parse(input).map(
                |(
                    next_input,
                    (
                        primary_amounts,
                        _,
                        bracketed_amounts,
                        _,
                        name_chunks,
                        paren_amounts,
                        _,
                        modifier_text,
                    ),
                )| {
                    (
                        next_input,
                        ParsedIngredient {
                            name: raw_name(name_chunks),
                            amounts: merge_amounts(
                                primary_amounts,
                                bracketed_amounts,
                                paren_amounts,
                            ),
                            modifier: raw_modifier(modifier_text)
                                .map(|m| vec![ModifierPart::Raw(m)])
                                .unwrap_or_default(),
                            optional: false,
                        },
                    )
                },
            ),
            |i: &ParsedIngredient| i.name.clone(),
            "parse failed"
        )
    }
}

fn fallback_ingredient(input: &str) -> Ingredient {
    Ingredient {
        name: input.trim().to_string(),
        amounts: vec![],
        modifier: None,
        optional: false,
    }
}

fn raw_name(name_chunks: Option<Vec<&str>>) -> String {
    name_chunks.unwrap_or_default().join("").trim().to_string()
}

fn raw_modifier(modifier_text: &str) -> Option<String> {
    // The grammar captures only the trailing post-name text (after the first
    // ", "). Leading prep adjectives are extracted later by `refine`.
    clean_modifier(Some(modifier_text.trim().to_owned()))
}

fn merge_amounts(
    primary_amounts: Option<Vec<Measure>>,
    bracketed_amounts: Option<Vec<Measure>>,
    paren_amounts: Option<Vec<Measure>>,
) -> Vec<Measure> {
    // Concatenate the three optional groups in order: outer `flatten` drops the
    // `None`s, inner `flatten` unwraps each `Vec`'s elements.
    [primary_amounts, bracketed_amounts, paren_amounts]
        .into_iter()
        .flatten()
        .flatten()
        .collect()
}
