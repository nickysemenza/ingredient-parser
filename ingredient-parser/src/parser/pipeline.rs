use super::ir::{ModifierPart, ParsedIngredient};
use super::normalize::{lift_inline_descriptive_paren, normalize_input, strip_optional_note};
use crate::parser::Res;
use crate::trace;
use crate::traced_parser;
use crate::usage::classify_usage;
use crate::{Ingredient, IngredientParser};

impl IngredientParser {
    pub(crate) fn parse_ingredient_line(&self, input: &str) -> Ingredient {
        let normalized = normalize_input(input);
        let (mut ingredient, fell_back) =
            self.parse_normalized_ingredient_with_provenance(normalized.as_ref());
        // Attach parse-fidelity notes here at the single funnel, computed from
        // the *raw* input (so the digit scan sees what the author wrote).
        ingredient.parse_notes = crate::ParseNotes::derive(input, &ingredient, fell_back);
        ingredient
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

    /// Parse a normalized line, also reporting whether the parse fell back to a
    /// name-only ingredient (no structured recognizer/core parse succeeded).
    /// Used to derive parse notes.
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
        // Authoritative usage classification: re-run with the whole line in
        // hand, so purpose phrases the modifier extraction missed still count.
        // Construction-time classification (Ingredient::new, the IR lowering)
        // only sees name+modifier; this is the one place with the full text.
        ingredient.usage = classify_usage(
            &ingredient.name,
            ingredient.modifier.as_deref(),
            Some(input),
            None,
        );
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
            let (_, mut parsed) = self.parse_ingredient_ir(&cleaned).ok()?;
            // Refine first, then append the lifted aside as the trailing modifier
            // part — so it lands *after* any prep adjective the refine passes
            // extract (e.g. "sliced, ¼ inch / 6 mm"), and is joined/finalized
            // through the IR's single lowering path.
            self.refine(&mut parsed);
            parsed.push_modifier(ModifierPart::Raw(aside));
            return Some(parsed.into());
        }

        self.parse_ingredient_ir(input)
            .ok()
            .map(|(_, ingredient)| self.postprocess_ingredient(ingredient))
    }

    /// Parse the raw grammar shape: amounts, then clause segmentation and
    /// assembly. Feeds the refine pipeline.
    ///
    /// The `"parse_ingredient"` span name is load-bearing outside this module —
    /// `trace::stages` buckets the grammar stage by it, and a golden snapshot
    /// pins it — so it stays even though the function of that name is gone.
    fn parse_ingredient_ir<'a>(&self, input: &'a str) -> Res<&'a str, ParsedIngredient> {
        traced_parser!(
            "parse_ingredient",
            input,
            self.parse_ingredient_segmented(input),
            |i: &ParsedIngredient| i.name.clone(),
            "parse failed"
        )
    }

    /// Decompose a line into grammar-stage field spans for the `--explain`
    /// decomposition view.
    ///
    /// Returns the normalized string the spans index into, plus one
    /// [`FieldSpan`](crate::FieldSpan) per amount region / name / modifier the
    /// grammar carved. `spans` is empty when a whole-line recognizer or the
    /// name-only fallback produced the result (no core-grammar carving to show).
    ///
    /// # Example
    ///
    /// ```
    /// use ingredient::IngredientParser;
    /// use ingredient::Field;
    ///
    /// let parser = IngredientParser::new();
    /// let decomp = parser.decompose("2 cups flour, sifted");
    ///
    /// assert_eq!(decomp.source, "2 cups flour, sifted");
    /// assert_eq!(decomp.spans.len(), 3);
    /// assert_eq!(decomp.spans[0].field, Field::Amount);
    /// assert_eq!(decomp.spans[0].text, "2 cups");
    /// assert_eq!(decomp.spans[1].field, Field::Name);
    /// assert_eq!(decomp.spans[1].text, "flour");
    /// assert_eq!(decomp.spans[2].field, Field::Modifier);
    /// assert_eq!(decomp.spans[2].text, "sifted");
    /// ```
    pub fn decompose(&self, raw: &str) -> crate::Decomposition {
        let normalized = normalize_input(raw);
        let (cleaned, _optional) = strip_optional_note(normalized.as_ref());
        // Only the core grammar carves fields into spans; a whole-line
        // recognizer produces the result without the field grammar running.
        let spans = if self.run_recognizers(cleaned.as_ref()).is_some() {
            Vec::new()
        } else {
            self.grammar_field_spans(cleaned.as_ref())
        };
        crate::Decomposition {
            source: cleaned.into_owned(),
            spans,
        }
    }

    /// Grammar-stage field spans, derived from the clause byte ranges. Empty
    /// vec if the parse fails.
    fn grammar_field_spans(&self, input: &str) -> Vec<crate::FieldSpan> {
        self.segmented_field_spans(input)
    }
}

/// A name-only ingredient for a line the grammar could not parse.
fn fallback_ingredient(input: &str) -> Ingredient {
    Ingredient::from_parser_parts(input.trim(), vec![], None, false)
}

#[cfg(test)]
mod decompose_tests {
    use crate::{Field, IngredientParser};
    use rstest::rstest;

    /// (field, text) pairs expected from `decompose`, in span order.
    type Expected = &'static [(Field, &'static str)];

    #[rstest]
    #[case("2 cups flour", &[(Field::Amount, "2 cups"), (Field::Name, "flour")])]
    #[case(
        "1 cup / 240ml water",
        &[(Field::Amount, "1 cup / 240ml"), (Field::Name, "water")]
    )]
    #[case(
        "2¼ cups all-purpose flour, sifted",
        &[
            (Field::Amount, "2¼ cups"),
            (Field::Name, "all-purpose flour"),
            (Field::Modifier, "sifted"),
        ]
    )]
    // Grammar-stage carve: prep adjectives stay in the name span here; refine
    // moves them later (the --explain stage view shows that separately).
    #[case(
        "2 chopped fresh basil",
        &[(Field::Amount, "2"), (Field::Name, "chopped fresh basil")]
    )]
    #[case("salt", &[(Field::Name, "salt")])]
    // A multi-clause tail yields one modifier span per clause on the
    // segmented path (the legacy grammar produced a single span here).
    #[case(
        "1 cup flour, sifted, divided",
        &[
            (Field::Amount, "1 cup"),
            (Field::Name, "flour"),
            (Field::Modifier, "sifted"),
            (Field::Modifier, "divided"),
        ]
    )]
    fn decompose_carves_fields(#[case] input: &str, #[case] expected: Expected) {
        let parser = IngredientParser::new();
        let decomp = parser.decompose(input);

        let got: Vec<(Field, &str)> = decomp
            .spans
            .iter()
            .map(|s| (s.field, s.text.as_str()))
            .collect();
        let want: Vec<(Field, &str)> = expected.to_vec();
        assert_eq!(got, want, "decompose({input:?})");

        // Every span must index back into `source` and match its `text`, and
        // spans must not overlap.
        let mut prev_end = 0;
        for s in &decomp.spans {
            assert_eq!(&decomp.source[s.range.clone()], s.text, "span text/range");
            assert!(s.range.start >= prev_end, "spans overlap in {input:?}");
            prev_end = s.range.end;
        }
    }

    #[test]
    fn recognizer_handled_line_has_no_grammar_spans() {
        // "Juice of 1 lemon" is produced by the x_of_construction recognizer,
        // not the field grammar — so there are no grammar-stage spans to show.
        let parser = IngredientParser::new();
        let decomp = parser.decompose("Juice of 1 lemon");
        assert!(
            decomp.spans.is_empty(),
            "recognizer result should yield no spans, got {:?}",
            decomp.spans
        );
    }
}
