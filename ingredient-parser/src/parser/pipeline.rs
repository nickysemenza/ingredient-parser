use super::ir::ParsedIngredient;
use super::normalize::{NormalizedSource, normalize_input, normalize_source};
use crate::parser::Res;
use crate::trace;
use crate::traced_parser;
use crate::usage::classify_usage;
use crate::{
    Decomposition, Field, FieldSpan, Ingredient, IngredientParser, ParseExecution, ParseOptions,
    TraceDetail,
};

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

        let mapped = options.decomposition.then(|| normalize_source(input));
        let normalized = mapped.as_ref().map_or_else(
            || normalize_input(input),
            |source| std::borrow::Cow::Borrowed(source.text.as_ref()),
        );
        let cleaned = normalized.as_ref();
        let (mut ingredient, fell_back, ownership) =
            self.parse_normalized_ingredient_inner(cleaned);
        ingredient.usage = classify_usage(
            &ingredient.name,
            ingredient.modifier.as_deref(),
            Some(cleaned),
            None,
        );
        ingredient.parse_notes =
            crate::ParseNotes::derive(&ingredient, fell_back, ownership.unresolved_quantity);

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
        let decomposition = mapped
            .as_ref()
            .map(|source| final_decomposition(input, source, &ownership));
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

    /// Returns the parsed ingredient and `true` if it came from the name-only
    /// fallback (no recognizer or core parse succeeded).
    fn parse_normalized_ingredient_inner(&self, input: &str) -> (Ingredient, bool, Ownership) {
        // First try the whole-line special-form recognizers (first match wins),
        // then fall back to the general core parse, then to a name-only ingredient.
        if let Some(mut parsed) = self.parse_shape(input) {
            self.refine(&mut parsed);
            if !parsed.name.trim().is_empty() || parsed.modifier.is_empty() {
                parsed.order_modifiers();
                let ownership = Ownership {
                    spans: parsed.ownership(),
                    unresolved_quantity: parsed.unresolved_quantity,
                };
                let ingredient: Ingredient = parsed.into();
                if trace::is_diagnostics_enabled() {
                    trace::record_grammar(trace::GrammarOutcome::Parsed(ingredient.name.clone()));
                }
                return (ingredient, false, ownership);
            }
        }
        if trace::is_diagnostics_enabled() {
            trace::record_grammar(trace::GrammarOutcome::FellBack);
        }
        (
            fallback_ingredient(input),
            true,
            Ownership {
                spans: vec![(0..input.len(), Field::Name)],
                unresolved_quantity: false,
            },
        )
    }

    pub(super) fn parse_ingredient_ir<'a>(&self, input: &'a str) -> Res<&'a str, ParsedIngredient> {
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
}

struct Ownership {
    unresolved_quantity: bool,
    spans: Vec<(std::ops::Range<usize>, Field)>,
}

/// Project the parser's owned regions back through text normalization. Tokens
/// discarded by normalization are never guessed to be measures.
fn final_decomposition(
    raw: &str,
    source: &NormalizedSource<'_>,
    ownership: &Ownership,
) -> Decomposition {
    let mut byte_fields = vec![None; raw.len()];
    for (range, field) in &ownership.spans {
        for origin in source.project(range.clone()) {
            byte_fields[origin].fill(Some(*field));
        }
    }
    // Character granularity also handles adjacent fields with no delimiter,
    // without assigning an entire mixed token to whichever field matched first.
    let tokens: Vec<_> = raw
        .char_indices()
        .filter_map(|(start, ch)| {
            (ch.is_alphanumeric() || crate::fraction::is_vulgar(ch)).then_some(SourceToken {
                range: start..start + ch.len_utf8(),
            })
        })
        .collect();
    let labels: Vec<_> = tokens
        .iter()
        .map(|token| byte_fields[token.range.start])
        .collect();
    Decomposition {
        source: raw.to_string(),
        spans: spans_from_labels(raw, &tokens, &labels),
    }
}

struct SourceToken {
    range: std::ops::Range<usize>,
}

fn spans_from_labels(
    source: &str,
    tokens: &[SourceToken],
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
            text: source[token.range.clone()].to_string(),
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
    // The structural batch count owns its authored unit; the connector "of"
    // belongs to neither the count nor the ingredient name.
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

    #[rstest]
    #[case("flour (see page 123)", &[(Field::Name, "flour")])]
    #[case("2 cups flour (see page 2), sifted", &[
        (Field::Amount, "2 cups"), (Field::Name, "flour"), (Field::Modifier, "sifted")])]
    #[case("• ½ cup café (optional), chopped", &[
        (Field::Amount, "½ cup"), (Field::Name, "café"), (Field::Modifier, "chopped")])]
    #[case("(Juice of 1 lemon)", &[
        (Field::Modifier, "Juice of"), (Field::Amount, "1"), (Field::Name, "lemon")])]
    #[case("(chopped walnuts — 1 cup)", &[
        (Field::Modifier, "chopped"), (Field::Name, "walnuts"), (Field::Amount, "1 cup")])]
    #[case("1 cup flour, flour for dusting", &[
        (Field::Amount, "1 cup"), (Field::Name, "flour"), (Field::Modifier, "flour for dusting")])]
    #[case("4 (4- to 6-ounce) halibut fillets", &[
        (Field::Amount, "4 (4- to 6-ounce"), (Field::Name, "halibut"), (Field::Amount, "fillets")])]
    #[case("2 medium (8-ounce) cones piloncillo", &[
        (Field::Amount, "2"), (Field::Modifier, "medium"),
        (Field::Amount, "8-ounce) cones"), (Field::Name, "piloncillo")])]
    #[case("Chicharrónes (recipe follows; optional)", &[
        (Field::Name, "Chicharrónes"), (Field::Modifier, "recipe follows")])]
    #[case("¼ cup plus 1 tablespoon flour", &[
        (Field::Amount, "¼ cup plus 1 tablespoon"), (Field::Name, "flour")])]
    fn decomposition_tracks_consumed_occurrences(#[case] input: &str, #[case] expected: Expected) {
        let result = IngredientParser::new().decompose(input);
        let fields: Vec<_> = result
            .spans
            .iter()
            .map(|span| (span.field, span.text.as_str()))
            .collect();
        assert_eq!(fields, expected);
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
