use super::ir::{ModifierPart, ParsedIngredient};
use super::normalize::{lift_inline_descriptive_paren, normalize_input, strip_optional_note};
use crate::parser::Res;
use crate::trace;
use crate::traced_parser;
use crate::unit::singular;
use crate::usage::classify_usage;
use crate::{
    Decomposition, Field, FieldSpan, Ingredient, IngredientParser, ParseExecution, ParseOptions,
    TraceDetail,
};
use std::str::FromStr;

impl IngredientParser {
    /// Execute the Ingredient-line pipeline once and derive every requested
    /// observation from that same run.
    pub fn parse_line(&self, input: &str, options: ParseOptions) -> ParseExecution {
        let record_stages = options.trace != TraceDetail::None;
        let record_trace = options.trace == TraceDetail::Full;
        if record_stages {
            trace::enable_diagnostics();
            trace::enable_stage_recording(input);
        }
        if record_trace {
            trace::enable_tracing();
            trace::trace_enter("parse_line", input);
        }

        let normalized = normalize_input(input);
        let (mut ingredient, fell_back) = self.parse_pipeline_after_normalize(normalized.as_ref());
        ingredient.parse_notes = crate::ParseNotes::derive(input, &ingredient, fell_back);

        if record_trace {
            trace::trace_exit_success(0, &ingredient.name);
        }
        let stages = record_stages.then(|| trace::finish_stage_recording(&ingredient.name));
        let trace = record_trace.then(|| {
            let mut parsed_trace = trace::disable_tracing(input);
            if let Some(report) = stages.clone() {
                parsed_trace.attach_stage_report(report);
            }
            parsed_trace
        });
        let decomposition = options
            .decomposition
            .then(|| self.final_decomposition(input, &ingredient));
        if record_stages {
            trace::disable_diagnostics();
        }

        ParseExecution {
            ingredient,
            decomposition,
            stages,
            trace,
        }
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
        if let Some(ingredient) = self.run_recognizers(input) {
            if trace::is_diagnostics_enabled() {
                trace::record_grammar(input, trace::GrammarOutcome::Skipped);
            }
            return (ingredient, false);
        }
        if let Some(ingredient) = self.parse_core_ingredient(input).filter(|ingredient| {
            // Reject a "successful" parse that lost the ingredient name into
            // the modifier; the graceful fallback preserves the authored text.
            let name_empty = ingredient.name.trim().is_empty();
            let has_modifier = ingredient
                .modifier
                .as_deref()
                .is_some_and(|modifier| !modifier.trim().is_empty());
            !(name_empty && has_modifier)
        }) {
            if trace::is_diagnostics_enabled() {
                trace::record_grammar(
                    input,
                    trace::GrammarOutcome::Parsed(ingredient.name.clone()),
                );
            }
            (ingredient, false)
        } else {
            if trace::is_diagnostics_enabled() {
                trace::record_grammar(input, trace::GrammarOutcome::FellBack);
            }
            (fallback_ingredient(input), true)
        }
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

    /// Decompose a line into final-field spans for the `--explain`
    /// decomposition view.
    ///
    /// Returns the **authored** line, unmodified, plus one
    /// [`FieldSpan`](crate::FieldSpan) per contiguous run of it that ended up in
    /// the parsed amount / name / modifier. The spans describe where each *final*
    /// field came from, after every stage has run — so a prep word refine moved
    /// out of the name is labeled Modifier, where the earlier grammar-stage carve
    /// would still have shown it inside the name.
    ///
    /// Every parse path produces spans, recognizers and the name-only fallback
    /// included; `spans` is empty only for a line with no alphanumeric text to
    /// attribute. Spans are ordered by position, never overlap, and need not
    /// cover the whole line — punctuation and any word no field kept (a dropped
    /// cross-reference, say) are left unlabeled.
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
        self.parse_line(
            raw,
            ParseOptions {
                decomposition: true,
                trace: TraceDetail::None,
            },
        )
        .decomposition
        // Always `Some` for `decomposition: true`; defaulting keeps the workspace
        // `expect_used = "deny"` lint satisfied without a panic path.
        .unwrap_or_default()
    }

    fn final_decomposition(&self, raw: &str, ingredient: &Ingredient) -> Decomposition {
        let tokens = source_tokens(raw);
        let mut labels = vec![None; tokens.len()];
        claim_text(&tokens, &mut labels, &ingredient.name, Field::Name);
        if let Some(modifier) = ingredient.modifier.as_deref() {
            claim_text(&tokens, &mut labels, modifier, Field::Modifier);
        }

        for index in 0..tokens.len() {
            if labels[index].is_some() {
                continue;
            }
            let token = tokens[index].text.to_lowercase();
            let number = token
                .chars()
                .any(|ch| ch.is_ascii_digit() || crate::fraction::is_vulgar(ch))
                || crate::parser::vocab::SPELLED_COUNTS.contains(&token.as_str());
            let unit = self.units.contains(&token)
                || !matches!(
                    crate::unit::Unit::from_str(&token),
                    Ok(crate::unit::Unit::Other(_)) | Err(_)
                )
                || token_is_amount_unit(&token, &ingredient.amounts)
                // `rewrite_batch_of_to_recipe` turns an authored "N batch(es) of"
                // into "N recipe", so the amount records "recipe" and the word
                // actually on the line matches nothing above.
                || matches!(token.as_str(), "batch" | "batches");
            if number || unit {
                labels[index] = Some(Field::Amount);
            }
        }

        // Join an unclaimed measurement qualifier to adjacent amount tokens.
        for index in 0..tokens.len() {
            if labels[index].is_none()
                && matches!(
                    tokens[index].text.to_lowercase().as_str(),
                    "about" | "approximately" | "roughly" | "around" | "scant" | "heaping"
                )
                && ((index > 0 && labels[index - 1] == Some(Field::Amount))
                    || labels.get(index + 1) == Some(&Some(Field::Amount)))
            {
                labels[index] = Some(Field::Amount);
            }
        }

        let mut spans = spans_from_labels(raw, &tokens, &labels);
        spans.sort_by_key(|span| span.range.start);
        Decomposition {
            source: raw.to_string(),
            spans,
        }
    }
}

struct SourceToken<'a> {
    text: &'a str,
    range: std::ops::Range<usize>,
}

fn source_tokens(source: &str) -> Vec<SourceToken<'_>> {
    let mut tokens = Vec::new();
    let mut start = None;
    for (index, ch) in source.char_indices() {
        if ch.is_alphanumeric() || crate::fraction::is_vulgar(ch) {
            start.get_or_insert(index);
        } else if let Some(token_start) = start.take() {
            tokens.push(SourceToken {
                text: &source[token_start..index],
                range: token_start..index,
            });
        }
    }
    if let Some(token_start) = start {
        tokens.push(SourceToken {
            text: &source[token_start..],
            range: token_start..source.len(),
        });
    }
    tokens
}

/// Whether an unclaimed source token spells the unit of one of the parsed
/// amounts.
///
/// The unit vocabulary and [`Unit::from_str`](crate::unit::Unit) only recognize
/// *measurement* units, but a count unit can be any word the grammar accepted:
/// a size word ("1 medium onion" parses to `{medium: 1}`), a "batch", a
/// "sprig". Asking the parse output — rather than re-deriving a unit set —
/// keeps the label in step with whatever the grammar actually produced.
/// Compared through [`singular`] so an authored "cups"/"batches" still matches a
/// stored "cup"/"batch".
fn token_is_amount_unit(token: &str, amounts: &[crate::unit::Measure]) -> bool {
    let token = singular(token);
    amounts
        .iter()
        .any(|amount| singular(&amount.unit().to_str()) == token)
}

fn value_tokens(value: &str) -> impl Iterator<Item = String> + '_ {
    value
        .split(|ch: char| !ch.is_alphanumeric() && !crate::fraction::is_vulgar(ch))
        .filter(|token| !token.is_empty())
        .map(str::to_lowercase)
}

fn claim_text(tokens: &[SourceToken<'_>], labels: &mut [Option<Field>], value: &str, field: Field) {
    let mut cursor = 0;
    for wanted in value_tokens(value) {
        let found = (cursor..tokens.len())
            .chain(0..cursor)
            .find(|index| labels[*index].is_none() && tokens[*index].text.to_lowercase() == wanted);
        if let Some(index) = found {
            labels[index] = Some(field);
            cursor = index + 1;
        }
    }
}

fn spans_from_labels(
    source: &str,
    tokens: &[SourceToken<'_>],
    labels: &[Option<Field>],
) -> Vec<FieldSpan> {
    let mut spans: Vec<FieldSpan> = Vec::new();
    for (token, field) in tokens.iter().zip(labels) {
        let Some(field) = *field else { continue };
        if let Some(previous) = spans.last_mut()
            && previous.field == field
            && source[previous.range.end..token.range.start]
                .chars()
                .all(|ch| !ch.is_alphanumeric())
        {
            previous.range.end = token.range.end;
            previous.text = source[previous.range.clone()].to_string();
            continue;
        }
        spans.push(FieldSpan {
            field,
            range: token.range.clone(),
            text: token.text.to_string(),
        });
    }
    spans
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
    // Final-field provenance: refine moved the prep phrase to Modifier.
    #[case(
        "2 chopped fresh basil",
        &[
            (Field::Amount, "2"),
            (Field::Modifier, "chopped fresh"),
            (Field::Name, "basil")
        ]
    )]
    #[case("salt", &[(Field::Name, "salt")])]
    // A size word is the count unit ("1 medium onion" parses to `{medium: 1}`),
    // so it belongs to the Amount span rather than reading as an unlabeled hole.
    #[case(
        "1 medium onion, diced",
        &[
            (Field::Amount, "1 medium"),
            (Field::Name, "onion"),
            (Field::Modifier, "diced"),
        ]
    )]
    #[case("2 large eggs", &[(Field::Amount, "2 large"), (Field::Name, "eggs")])]
    #[case("3 small potatoes", &[(Field::Amount, "3 small"), (Field::Name, "potatoes")])]
    // "N batch(es) of X" is normalized to "N recipe X", so the authored unit word
    // never appears in the parsed amount — it is labeled from the rewrite instead.
    #[case(
        "1 batch of Marshmallow Meringue",
        &[(Field::Amount, "1 batch"), (Field::Name, "Marshmallow Meringue")]
    )]
    // Contiguous authored text assigned to one final field is one span.
    #[case(
        "1 cup flour, sifted, divided",
        &[
            (Field::Amount, "1 cup"),
            (Field::Name, "flour"),
            (Field::Modifier, "sifted, divided"),
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
    fn recognizer_produces_final_field_spans() {
        let parser = IngredientParser::new();
        let decomp = parser.decompose("Juice of 1 lemon");
        let fields: Vec<(Field, &str)> = decomp
            .spans
            .iter()
            .map(|span| (span.field, span.text.as_str()))
            .collect();
        assert_eq!(
            fields,
            vec![
                (Field::Modifier, "Juice of"),
                (Field::Amount, "1"),
                (Field::Name, "lemon")
            ]
        );
    }

    #[rstest]
    #[case::bullet("• 2 cups flour", "• 2 cups flour")]
    #[case::optional("2 cups flour (optional)", "2 cups flour (optional)")]
    #[case::utf8("½ cup jalapeño", "½ cup jalapeño")]
    #[case::fallback("mystery ✨ ingredient", "mystery ✨ ingredient")]
    fn decomposition_indexes_the_authored_line(#[case] input: &str, #[case] source: &str) {
        let decomp = IngredientParser::new().decompose(input);
        assert_eq!(decomp.source, source);
        for span in decomp.spans {
            assert_eq!(&decomp.source[span.range.clone()], span.text);
        }
    }
}
