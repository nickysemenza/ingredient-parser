//! Diagnostic rendering for `parse-ingredient --explain`.
//!
//! One mode, rendered with miette over the *authored* line.
//! [`ingredient::IngredientParser::decompose`] hands us a byte span for every
//! run of the source that ended up in the parsed amount / name / modifier, and
//! we label each one. Because the spans describe the *final* fields, every parse
//! path produces them — recognizers and the name-only fallback included — so
//! there is no span-less case to fall back from.
//!
//! A digit that produced no amount needs no caret of its own: the labels already
//! show which field swallowed it, so the header says that instead.
//!
//! miette lives only here — the published `ingredient` crate stays miette-free.

use ingredient::{Decomposition, Field, ParseNotes};
use miette::{
    GraphicalReportHandler, GraphicalTheme, LabeledSpan, MietteDiagnostic, Report, Severity,
};

fn field_label(field: Field) -> &'static str {
    match field {
        Field::Amount => "amount",
        Field::Name => "name",
        Field::Modifier => "modifier",
    }
}

/// The help line: confidence, then any reason this parse wants a human look,
/// then where to go next. The review reasons and their wording belong to the
/// parser; this surface only decides the miette severity.
fn help_for(diag: &ParseNotes) -> String {
    let mut s = format!("confidence: {:?}", diag.confidence);
    for reason in diag.review_reasons() {
        s.push_str(" · ");
        s.push_str(&reason.to_string());
    }
    s.push_str(" · see the stage view below; route the fix via parser/mod.rs");
    s
}

/// The decomposition diagnostic: one label per final-field span.
fn decomposition_diagnostic(decomp: &Decomposition, diag: &ParseNotes) -> MietteDiagnostic {
    // A digit that produced no amount is informative here, not alarming: the
    // labels already show it landed in the name/modifier, not a missed quantity.
    let (message, severity) = if diag.unparsed_digit {
        (
            "no amount parsed — any digit is part of the name/modifier",
            Severity::Warning,
        )
    } else {
        ("final field decomposition", Severity::Advice)
    };

    let labels: Vec<LabeledSpan> = decomp
        .spans
        .iter()
        .map(|s| LabeledSpan::at(s.range.clone(), field_label(s.field)))
        .collect();

    MietteDiagnostic::new(message)
        .with_severity(severity)
        .with_labels(labels)
        .with_help(help_for(diag))
}

/// Render the miette report block for `--explain`: one label per
/// amount/name/modifier span, over `decomp.source` (the authored line).
/// `use_color` mirrors the caller's `IsTerminal` gate.
pub fn render(decomp: &Decomposition, diag: &ParseNotes, use_color: bool) -> String {
    let d = decomposition_diagnostic(decomp, diag);

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
    use std::ops::Range;

    use super::*;
    use ingredient::Confidence;

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

    /// The help line carries the parser's review reasons, so a parse worth a
    /// human look says why right in the header block.
    #[test]
    fn render_help_carries_review_reasons() {
        let parsed = ingredient::from_str("1+1 vitamins");
        let out = render(
            &ingredient::decompose("1+1 vitamins"),
            &parsed.parse_notes,
            false,
        );
        assert!(out.contains("confidence: Low"), "{out}");
        for reason in parsed.parse_notes.review_reasons() {
            assert!(out.contains(&reason.to_string()), "missing {reason}: {out}");
        }
        assert!(out.contains("route the fix via parser/mod.rs"), "{out}");
    }

    #[test]
    fn render_labels_final_field_decomposition() {
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
        assert!(out.contains("final field decomposition"));
        assert!(out.contains("amount"));
        assert!(out.contains("name"));
        assert!(!out.contains("this number didn't become an amount"));
    }

    #[test]
    fn render_digit_in_name_is_informative_not_alarming() {
        // "Pierre Ferrand 1840 Cognac": the Name span covers the whole line, so
        // the digit is visibly part of the name — say so rather than warning
        // about a missed quantity.
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
