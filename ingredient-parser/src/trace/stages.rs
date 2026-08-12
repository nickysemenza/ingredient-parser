//! Structured stage-level view of a parse trace.
//!
//! The full trace tree is great for debugging the grammar but drowns the
//! pipeline story in `alt()` backtracking. [`StageReport`] buckets the root's
//! direct children into the pipeline stages — normalize → recognize → grammar
//! → segment → refine → result — so callers (the CLI's `--explain` renderer,
//! the egui stages view) can show *which stage* shaped a line without
//! re-deriving the bucketing. See the routing guide in `parser/mod.rs`.

use super::{TraceNode, TraceOutcome};
use crate::parser::recognize::RECOGNIZER_TRACE_NAMES;
use crate::parser::segment::SEGMENT_TRACE_NAMES;
use std::cell::RefCell;

/// The grammar span name (the `traced_parser!` wrapping `parse_ingredient`).
const GRAMMAR_NAME: &str = "parse_ingredient";

/// A normalize rewrite or refine pass that changed the line/ingredient.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageRewrite {
    /// Name of the rewrite/pass (e.g. `strip_optional_note`).
    pub name: String,
    /// Input before the step (truncated for display).
    pub before: String,
    /// Output preview after the step.
    pub after: String,
}

/// One special-form recognizer attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecognizerAttempt {
    /// Recognizer name (e.g. `x_of_construction`).
    pub name: String,
    /// Output preview when the recognizer matched; `None` when it didn't.
    pub output: Option<String>,
}

/// How the nom grammar stage concluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GrammarOutcome {
    /// The grammar parsed the line; carries the parsed name preview.
    Parsed(String),
    /// The grammar failed and the parse fell back to a name-only ingredient.
    FellBack,
    /// A recognizer produced the result without re-entering the grammar.
    Skipped,
}

/// One of the five Ingredient-line execution stages (plus the final result).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Normalize,
    Recognize,
    Grammar,
    Segment,
    Refine,
    Result,
}

/// Explicit outcome of an authoritative stage event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StageEventOutcome {
    /// A candidate was tried but did not match.
    Attempted,
    /// A rewrite or pass changed the execution state.
    Applied { output: String },
    /// A recognizer or grammar produced a result.
    Matched { output: String },
    /// The grammar failed and the name-only fallback was used.
    Failed { reason: String },
}

/// One directly recorded event, in execution order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageEvent {
    pub stage: Stage,
    pub name: String,
    pub input: String,
    pub outcome: StageEventOutcome,
}

/// Stage-level summary of a parse trace (the data behind `--explain`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageReport {
    /// Authoritative ordered event stream. Compatibility projections below are
    /// derived while recording, never reconstructed from trace-tree nesting.
    pub events: Vec<StageEvent>,
    /// The traced input line (truncated for display).
    pub input: String,
    /// Normalize rewrites that fired, in order.
    pub normalize: Vec<StageRewrite>,
    /// Recognizer attempts, in order (empty when the trace has no core block).
    pub recognizers: Vec<RecognizerAttempt>,
    /// Grammar outcome; `None` only for degenerate traces with no
    /// recognizer/grammar nodes at all (e.g. a trace captured mid-parse).
    pub grammar: Option<GrammarOutcome>,
    /// Segmentation decisions (clause classifications and assembly repairs),
    /// in order. Empty on the legacy path.
    pub segment: Vec<StageRewrite>,
    /// Refine passes that changed the ingredient, in order.
    pub refine: Vec<StageRewrite>,
    /// Final result name preview; `None` means the name-only fallback fired.
    pub result_preview: Option<String>,
}

impl StageReport {
    /// `true` if any recognizer matched.
    pub fn recognizer_matched(&self) -> bool {
        self.recognizers.iter().any(|r| r.output.is_some())
    }
}

impl Default for StageReport {
    fn default() -> Self {
        Self::empty("")
    }
}

impl StageReport {
    fn empty(input: &str) -> Self {
        Self {
            events: Vec::new(),
            input: input.to_string(),
            normalize: Vec::new(),
            recognizers: Vec::new(),
            grammar: None,
            segment: Vec::new(),
            refine: Vec::new(),
            result_preview: None,
        }
    }
}

thread_local! {
    static STAGE_REPORT: RefCell<Option<StageReport>> = const { RefCell::new(None) };
}

pub(crate) fn enable_recording(input: &str) {
    STAGE_REPORT.with(|slot| *slot.borrow_mut() = Some(StageReport::empty(input)));
}

pub(crate) fn is_recording_enabled() -> bool {
    STAGE_REPORT.with(|slot| slot.borrow().is_some())
}

pub(crate) fn finish_recording(result: &str) -> StageReport {
    STAGE_REPORT.with(|slot| {
        let mut report = slot
            .borrow_mut()
            .take()
            .unwrap_or_else(|| StageReport::empty(""));
        report.result_preview = Some(result.to_string());
        report.events.push(StageEvent {
            stage: Stage::Result,
            name: "result".to_string(),
            input: String::new(),
            outcome: StageEventOutcome::Matched {
                output: result.to_string(),
            },
        });
        report
    })
}

pub(crate) fn record_rewrite(id: &str, before: &str, after: &str, changed: bool) {
    if !changed {
        return;
    }
    STAGE_REPORT.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(report) = slot.as_mut() else { return };
        let stage = if crate::parser::normalize::REWRITE_TRACE_NAMES.contains(&id) {
            Stage::Normalize
        } else if crate::parser::segment::SEGMENT_TRACE_NAMES.contains(&id) {
            Stage::Segment
        } else {
            Stage::Refine
        };
        let rewrite = StageRewrite {
            name: id.to_string(),
            before: before.to_string(),
            after: after.to_string(),
        };
        match stage {
            Stage::Normalize => report.normalize.push(rewrite),
            Stage::Segment => report.segment.push(rewrite),
            Stage::Refine => report.refine.push(rewrite),
            _ => {}
        }
        report.events.push(StageEvent {
            stage,
            name: id.to_string(),
            input: before.to_string(),
            outcome: StageEventOutcome::Applied {
                output: after.to_string(),
            },
        });
    });
}

pub(crate) fn record_recognizer(id: &str, input: &str, output: Option<&str>) {
    STAGE_REPORT.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(report) = slot.as_mut() else { return };
        report.recognizers.push(RecognizerAttempt {
            name: id.to_string(),
            output: output.map(str::to_string),
        });
        report.events.push(StageEvent {
            stage: Stage::Recognize,
            name: id.to_string(),
            input: input.to_string(),
            outcome: match output {
                Some(output) => StageEventOutcome::Matched {
                    output: output.to_string(),
                },
                None => StageEventOutcome::Attempted,
            },
        });
    });
}

pub(crate) fn record_grammar(input: &str, outcome: GrammarOutcome) {
    STAGE_REPORT.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(report) = slot.as_mut() else { return };
        let event_outcome = match &outcome {
            GrammarOutcome::Parsed(output) => StageEventOutcome::Matched {
                output: output.clone(),
            },
            GrammarOutcome::FellBack => StageEventOutcome::Failed {
                reason: "fell back to name-only".to_string(),
            },
            GrammarOutcome::Skipped => StageEventOutcome::Attempted,
        };
        report.grammar = Some(outcome);
        report.events.push(StageEvent {
            stage: Stage::Grammar,
            name: GRAMMAR_NAME.to_string(),
            input: input.to_string(),
            outcome: event_outcome,
        });
    });
}

fn is_core_node(name: &str) -> bool {
    name == GRAMMAR_NAME || RECOGNIZER_TRACE_NAMES.contains(&name)
}

fn success_preview(node: &TraceNode) -> Option<&str> {
    match &node.outcome {
        TraceOutcome::Success { output_preview, .. } => Some(output_preview),
        _ => None,
    }
}

/// Find the grammar node among the core children, whether it's a direct child
/// (no recognizer matched) or nested under a successful recognizer (e.g.
/// `x_of_construction` re-parses its rewritten line through the grammar).
fn find_grammar(core: &[TraceNode]) -> Option<&TraceNode> {
    for c in core {
        if c.name == GRAMMAR_NAME {
            return Some(c);
        }
        if RECOGNIZER_TRACE_NAMES.contains(&c.name.as_str())
            && let Some(g) = c.children.iter().find(|g| g.name == GRAMMAR_NAME)
        {
            return Some(g);
        }
    }
    None
}

fn rewrite_from(node: &TraceNode) -> StageRewrite {
    StageRewrite {
        name: node.name.clone(),
        before: node.input.clone(),
        after: success_preview(node).unwrap_or("").to_string(),
    }
}

/// Bucket a trace root's direct children into pipeline stages.
pub(super) fn build_report(root: &TraceNode) -> StageReport {
    let children = &root.children;
    let first_core = children.iter().position(|c| is_core_node(&c.name));
    let last_core = children.iter().rposition(|c| is_core_node(&c.name));

    // normalize — every node before the first core (recognizer/grammar) node.
    let normalize_nodes = match first_core {
        Some(i) => &children[..i],
        None => &children[..],
    };
    let normalize = normalize_nodes.iter().map(rewrite_from).collect();

    // recognize + grammar + segment — the core block. Segment decisions
    // (clause classifications, assembly repairs) nest *inside* the grammar
    // span on the segmented path.
    let (recognizers, grammar, segment) = match (first_core, last_core) {
        (Some(i), Some(j)) => {
            let core = &children[i..=j];
            let recognizers = core
                .iter()
                .filter(|c| RECOGNIZER_TRACE_NAMES.contains(&c.name.as_str()))
                .map(|c| RecognizerAttempt {
                    name: c.name.clone(),
                    output: success_preview(c).map(str::to_string),
                })
                .collect();
            let grammar_node = find_grammar(core);
            let grammar = match grammar_node {
                Some(g) => match success_preview(g) {
                    Some(p) => GrammarOutcome::Parsed(p.to_string()),
                    None => GrammarOutcome::FellBack,
                },
                None => GrammarOutcome::Skipped,
            };
            let segment = grammar_node
                .map(|g| {
                    g.children
                        .iter()
                        .filter(|c| SEGMENT_TRACE_NAMES.contains(&c.name.as_str()))
                        .map(rewrite_from)
                        .collect()
                })
                .unwrap_or_default();
            (recognizers, Some(grammar), segment)
        }
        _ => (Vec::new(), None, Vec::new()),
    };

    // refine — every node after the last core node.
    let refine_nodes = match last_core {
        Some(j) => &children[j + 1..],
        None => &[][..],
    };
    let refine = refine_nodes.iter().map(rewrite_from).collect();

    StageReport {
        events: Vec::new(),
        input: root.input.clone(),
        normalize,
        recognizers,
        grammar,
        segment,
        refine,
        result_preview: success_preview(root).map(str::to_string),
    }
}
