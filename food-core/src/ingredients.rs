//! Ingredient lines: parsed rows with their source decomposition, and the full
//! inspection (stages and grammar trace) behind one line.
use ingredient::trace::{GrammarOutcome, StageReport, StageRewrite, TraceOutcome};
use ingredient::{Confidence, Field, Ingredient, IngredientParser, ParseOptions, TraceDetail};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One parsed ingredient line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct IngredientRow {
    /// 1-based position in the submitted text.
    pub line_number: usize,
    /// The line as authored.
    pub input: String,
    pub ingredient: Ingredient,
    /// `ingredient.amounts`, formatted by the parser.
    pub amounts: Vec<String>,
    pub confidence: Confidence,
    pub review_reasons: Vec<ReviewReason>,
    /// The authored line cut into the regions that became each field;
    /// concatenated, the texts reproduce `input`.
    pub segments: Vec<Segment>,
}

/// Why a parse needs a human look: a stable tag plus the parser's message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct ReviewReason {
    pub tag: String,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum SegmentField {
    Amount,
    Name,
    Modifier,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct Segment {
    pub text: String,
    pub field: Option<SegmentField>,
}

/// Everything recorded while parsing one line.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Inspection {
    pub row: IngredientRow,
    pub stages: Option<Stages>,
    /// The compact stage view, as `food-cli ingredient parse --explain` prints it.
    pub stages_text: String,
    pub trace: Option<TraceNode>,
    pub trace_text: String,
    pub jaeger_json: String,
}

/// The pipeline stages that shaped a line, in order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct Stages {
    pub normalize: Vec<Rewrite>,
    pub recognizers: Vec<Recognizer>,
    pub grammar: Grammar,
    pub segment: Vec<Rewrite>,
    pub refine: Vec<Rewrite>,
    /// Final name preview; `None` means the name-only fallback fired.
    pub result: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct Rewrite {
    pub name: String,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct Recognizer {
    pub name: String,
    /// Output preview when the recognizer matched.
    pub output: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Grammar {
    Parsed { name: String },
    FellBack,
    Skipped,
    Missing,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct TraceNode {
    pub name: String,
    pub input: String,
    pub outcome: TraceStatus,
    pub detail: String,
    pub children: Vec<TraceNode>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum TraceStatus {
    Success,
    Failure,
    Incomplete,
}

pub(crate) fn row(
    input: &str,
    line_number: usize,
    ingredient: Ingredient,
    decomposition: Option<&ingredient::Decomposition>,
) -> IngredientRow {
    let mut review_reasons: Vec<_> = ingredient
        .parse_notes
        .review_reasons()
        .into_iter()
        .map(|reason| ReviewReason {
            tag: reason.tag().to_owned(),
            message: reason.to_string(),
        })
        .collect();
    if input.trim().is_empty() {
        review_reasons.push(ReviewReason {
            tag: "empty".into(),
            message: "Empty ingredient line".into(),
        });
    }
    let segments = match decomposition {
        Some(d) => d
            .segments()
            .into_iter()
            .map(|s| Segment {
                text: s.text,
                field: s.field.map(|f| match f {
                    Field::Amount => SegmentField::Amount,
                    Field::Name => SegmentField::Name,
                    Field::Modifier => SegmentField::Modifier,
                }),
            })
            .collect(),
        None if input.is_empty() => vec![],
        None => vec![Segment {
            text: input.to_owned(),
            field: None,
        }],
    };
    IngredientRow {
        line_number,
        input: input.to_owned(),
        amounts: ingredient.amounts.iter().map(ToString::to_string).collect(),
        confidence: ingredient.parse_notes.confidence,
        review_reasons,
        segments,
        ingredient,
    }
}

const ROW: ParseOptions = ParseOptions {
    decomposition: true,
    trace: TraceDetail::None,
};

pub(crate) fn parse_row(
    parser: &IngredientParser,
    input: &str,
    line_number: usize,
) -> IngredientRow {
    let execution = parser.parse_line(input, ROW);
    row(
        input,
        line_number,
        execution.ingredient,
        execution.decomposition.as_ref(),
    )
}

/// One row per line of `input`, blank lines included so line numbers match.
pub fn parse_lines(input: &str) -> Vec<IngredientRow> {
    let parser = IngredientParser::new();
    input
        .lines()
        .enumerate()
        .map(|(index, line)| parse_row(&parser, line, index + 1))
        .collect()
}

fn rewrites(steps: &[StageRewrite]) -> Vec<Rewrite> {
    steps
        .iter()
        .map(|s| Rewrite {
            name: s.name.clone(),
            before: s.before.clone(),
            after: s.after.clone(),
        })
        .collect()
}

fn stages(report: &StageReport) -> Stages {
    Stages {
        normalize: rewrites(&report.normalize),
        recognizers: report
            .recognizers
            .iter()
            .map(|r| Recognizer {
                name: r.name.clone(),
                output: r.output.clone(),
            })
            .collect(),
        grammar: match &report.grammar {
            Some(GrammarOutcome::Parsed(name)) => Grammar::Parsed { name: name.clone() },
            Some(GrammarOutcome::FellBack) => Grammar::FellBack,
            Some(GrammarOutcome::Skipped) => Grammar::Skipped,
            None => Grammar::Missing,
        },
        segment: rewrites(&report.segment),
        refine: rewrites(&report.refine),
        result: report.result_preview.clone(),
    }
}

fn trace_node(source: &ingredient::trace::TraceNode) -> TraceNode {
    let (outcome, detail) = match &source.outcome {
        TraceOutcome::Success {
            consumed,
            output_preview,
        } => (
            TraceStatus::Success,
            format!("{consumed} characters · {output_preview}"),
        ),
        TraceOutcome::Failure { error } => (TraceStatus::Failure, error.clone()),
        TraceOutcome::Incomplete => (TraceStatus::Incomplete, String::new()),
    };
    TraceNode {
        name: source.name.clone(),
        input: source.input.clone(),
        outcome,
        detail,
        children: source.children.iter().map(trace_node).collect(),
    }
}

/// Parse one line with every observation turned on.
pub fn inspect(input: &str) -> Inspection {
    let execution = IngredientParser::new().parse_line(
        input,
        ParseOptions {
            decomposition: true,
            trace: TraceDetail::Full,
        },
    );
    let trace = execution.trace.as_ref();
    Inspection {
        stages: execution.stages.as_ref().map(stages),
        stages_text: execution
            .stages
            .as_ref()
            .map(|s| s.format(false))
            .unwrap_or_default(),
        trace: trace.map(|t| trace_node(&t.root)),
        trace_text: trace.map(|t| t.format_tree(false)).unwrap_or_default(),
        jaeger_json: trace.map(|t| t.to_jaeger_json()).unwrap_or_default(),
        row: row(
            input,
            1,
            execution.ingredient,
            execution.decomposition.as_ref(),
        ),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    #[test]
    fn rows_keep_blank_lines_and_reproduce_their_source() {
        let rows = parse_lines("2¼ cups all-purpose flour, sifted\n\n1+1 vitamins");
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[1].line_number, 2);
        assert_eq!(rows[1].review_reasons[0].tag, "empty");
        for row in &rows {
            let joined: String = row.segments.iter().map(|s| s.text.as_str()).collect();
            assert_eq!(joined, row.input);
        }
        assert_eq!(rows[0].ingredient.name, "all-purpose flour");
        assert_eq!(rows[0].amounts, vec!["2¼ cups"]);
        assert!(
            rows[0]
                .segments
                .iter()
                .any(|s| s.field == Some(SegmentField::Modifier) && s.text == "sifted")
        );
    }

    #[test]
    fn inspection_matches_the_batch_row() {
        for row in parse_lines("1 cup flour\nsalt and pepper to taste\n???") {
            let inspection = inspect(&row.input);
            assert_eq!(inspection.row, row.clone().with_line(1));
            assert!(inspection.trace.is_some());
            assert!(inspection.stages.is_some());
            assert!(serde_json::from_str::<serde_json::Value>(&inspection.jaeger_json).is_ok());
        }
    }

    impl IngredientRow {
        fn with_line(mut self, line_number: usize) -> Self {
            self.line_number = line_number;
            self
        }
    }
}
