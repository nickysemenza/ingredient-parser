//! Diagnostic rendering for `parse-ingredient --explain`.
//!
//! Two modes, both rendered with miette over the *normalized* string the
//! grammar parsed:
//!
//! - **Decomposition** (the common case): when the core grammar carved the line,
//!   [`ingredient::IngredientParser::decompose`] hands us byte spans for each
//!   amount / name / modifier; we label each one. This shows *how the grammar
//!   split the input*.
//! - **Digit caret** (fallback): when a recognizer or the name-only fallback
//!   produced the result there are no grammar spans, so if a digit was present
//!   but produced no amount we underline the digit run(s) instead.
//!
//! miette lives only here — the published `ingredient` crate stays miette-free.

use std::ops::Range;

use ingredient::{Confidence, Decomposition, Field, ParseNotes};
use miette::{
    GraphicalReportHandler, GraphicalTheme, LabeledSpan, MietteDiagnostic, Report, Severity,
};

/// Unicode vulgar-fraction glyphs the parser treats as part of a quantity, so
/// `5½` is reported as one span rather than `5` with the `½` orphaned.
const FRACTION_GLYPHS: &[char] = &[
    '½', '⅓', '⅔', '¼', '¾', '⅕', '⅖', '⅗', '⅘', '⅙', '⅚', '⅛', '⅜', '⅝', '⅞', '⅐', '⅑', '⅒',
];

fn is_quantity_char(c: char) -> bool {
    c.is_ascii_digit() || FRACTION_GLYPHS.contains(&c)
}

/// Byte ranges of maximal quantity runs (ASCII digits + adjacent vulgar
/// fractions) in `input`. These are the spans we underline when the parser saw
/// a digit but produced no amount. Pure and span-only so it can be unit-tested
/// without rendering.
pub fn unparsed_digit_spans(input: &str) -> Vec<Range<usize>> {
    let mut spans = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in input.char_indices() {
        if is_quantity_char(c) {
            start.get_or_insert(i);
        } else if let Some(s) = start.take() {
            spans.push(s..i);
        }
    }
    if let Some(s) = start {
        spans.push(s..input.len());
    }
    spans
}

fn severity_of(confidence: Confidence) -> Severity {
    match confidence {
        // A digit that produced no amount is a likely missed quantity — warn.
        Confidence::Low => Severity::Warning,
        Confidence::Medium | Confidence::High => Severity::Advice,
    }
}

fn field_label(field: Field) -> &'static str {
    match field {
        Field::Amount => "amount",
        Field::Name => "name",
        Field::Modifier => "modifier",
    }
}

fn message_for(diag: &ParseNotes) -> &'static str {
    match diag.confidence {
        Confidence::Low if diag.unparsed_digit => "quantity not parsed into an amount",
        Confidence::Low => "low-confidence parse",
        Confidence::Medium => "name-only parse",
        Confidence::High => "parsed with an amount",
    }
}

fn help_for(diag: &ParseNotes) -> String {
    let mut parts = vec![format!("confidence: {:?}", diag.confidence)];
    if diag.fell_back {
        parts.push("fell back to a name-only ingredient".to_string());
    }
    parts.push("see the stage view below; route the fix via parser/mod.rs".to_string());
    parts.join(" · ")
}

/// The decomposition diagnostic: one label per grammar field span.
fn decomposition_diagnostic(decomp: &Decomposition, diag: &ParseNotes) -> MietteDiagnostic {
    // A digit that produced no amount is informative here, not alarming: the
    // labels already show it landed in the name/modifier, not a missed quantity.
    let (message, severity) = if diag.unparsed_digit {
        (
            "no amount parsed — any digit is part of the name/modifier",
            Severity::Warning,
        )
    } else {
        ("grammar decomposition", Severity::Advice)
    };

    let labels: Vec<LabeledSpan> = decomp
        .spans
        .iter()
        .map(|s| LabeledSpan::at(s.range.clone(), field_label(s.field)))
        .collect();

    MietteDiagnostic::new(message)
        .with_severity(severity)
        .with_labels(labels)
        .with_help(format!(
            "{} · grammar-stage carve (refine may move prep words — see stage view)",
            format_args!("confidence: {:?}", diag.confidence)
        ))
}

/// The fallback diagnostic when the grammar didn't carve the line: a digit caret
/// when a number produced no amount, otherwise just the confidence header.
fn fallback_diagnostic(decomp: &Decomposition, diag: &ParseNotes) -> MietteDiagnostic {
    let mut d = MietteDiagnostic::new(message_for(diag))
        .with_severity(severity_of(diag.confidence))
        .with_help(help_for(diag));

    if diag.unparsed_digit {
        let labels: Vec<LabeledSpan> = unparsed_digit_spans(&decomp.source)
            .into_iter()
            .map(|span| LabeledSpan::at(span, "this number didn't become an amount"))
            .collect();
        d = d.with_labels(labels);
    }
    d
}

/// Render the miette report block for `--explain`. When the grammar carved the
/// line, labels each amount/name/modifier span; otherwise falls back to a digit
/// caret. Rendered over `decomp.source` (the normalized line the grammar saw).
/// `use_color` mirrors the caller's `IsTerminal` gate.
pub fn render(decomp: &Decomposition, diag: &ParseNotes, use_color: bool) -> String {
    let d = if decomp.spans.is_empty() {
        fallback_diagnostic(decomp, diag)
    } else {
        decomposition_diagnostic(decomp, diag)
    };

    let report = Report::new(d).with_source_code(decomp.source.clone());
    let theme = if use_color {
        GraphicalTheme::unicode()
    } else {
        GraphicalTheme::unicode_nocolor()
    };
    let handler = GraphicalReportHandler::new_themed(theme);
    let mut out = String::new();
    // render_report only errors if the writer fails; a String writer cannot.
    let _ = handler.render_report(&mut out, &*report);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    // "5½" is one run: '5' at byte 3, '½' (2 bytes) at 4..6 → 3..6; "1" at 0..1.
    #[case("1 (5½-ounce) piece", vec![0..1, 3..6])]
    #[case("2 cups flour", vec![0..1])]
    #[case("Juice of 1 lemon", vec![9..10])]
    #[case("salt", vec![])]
    // Two separate digit runs: '1' at 0, '2' at byte 7 (after "1 cup, ").
    #[case("1 cup, 2 tbsp", vec![0..1, 7..8])]
    fn finds_quantity_runs(#[case] input: &str, #[case] expected: Vec<Range<usize>>) {
        assert_eq!(unparsed_digit_spans(input), expected);
    }

    fn decomp(source: &str, spans: Vec<ingredient::FieldSpan>) -> Decomposition {
        Decomposition {
            source: source.to_string(),
            spans,
        }
    }

    fn span(field: Field, range: Range<usize>, text: &str) -> ingredient::FieldSpan {
        ingredient::FieldSpan {
            field,
            range,
            text: text.to_string(),
        }
    }

    #[test]
    fn render_underlines_unparsed_digit_when_no_grammar_spans() {
        // Empty spans (recognizer/fallback path) + a digit that produced no
        // amount → the digit-caret fallback fires.
        let diag = ParseNotes {
            confidence: Confidence::Low,
            fell_back: false,
            unparsed_digit: true,
        };
        let out = render(&decomp("1+1 vitamins", vec![]), &diag, false);
        assert!(out.contains("quantity not parsed into an amount"));
        assert!(out.contains("this number didn't become an amount"));
    }

    #[test]
    fn render_labels_grammar_decomposition() {
        // Spans present → each field is labeled, no digit-miss caret.
        let diag = ParseNotes {
            confidence: Confidence::High,
            fell_back: false,
            unparsed_digit: false,
        };
        let spans = vec![
            span(Field::Amount, 0..6, "2 cups"),
            span(Field::Name, 7..12, "flour"),
        ];
        let out = render(&decomp("2 cups flour", spans), &diag, false);
        assert!(out.contains("grammar decomposition"));
        assert!(out.contains("amount"));
        assert!(out.contains("name"));
        assert!(!out.contains("this number didn't become an amount"));
    }

    #[test]
    fn render_digit_in_name_is_informative_not_alarming() {
        // "Pierre Ferrand 1840 Cognac": grammar carves a Name span covering the
        // whole line; the digit is part of the name, so we say so rather than
        // warning about a missed quantity.
        let diag = ParseNotes {
            confidence: Confidence::Low,
            fell_back: false,
            unparsed_digit: true,
        };
        let spans = vec![span(Field::Name, 0..26, "Pierre Ferrand 1840 Cognac")];
        let out = render(&decomp("Pierre Ferrand 1840 Cognac", spans), &diag, false);
        assert!(out.contains("part of the name/modifier"));
        assert!(!out.contains("this number didn't become an amount"));
    }
}
