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

/// Stage-level summary of a parse trace (the data behind `--explain`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageReport {
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
        input: root.input.clone(),
        normalize,
        recognizers,
        grammar,
        segment,
        refine,
        result_preview: success_preview(root).map(str::to_string),
    }
}
