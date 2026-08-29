//! Structured stage-level view of a parse trace.
//!
//! The full trace tree is great for debugging the grammar but drowns the
//! pipeline story in `alt()` backtracking. [`StageReport`] buckets the root's
//! direct children into the pipeline stages — normalize → recognize → grammar
//! → segment → refine → result — so callers (the CLI's `--explain` renderer,
//! the egui stages view) can show *which stage* shaped a line without
//! re-deriving the bucketing. See the routing guide in `parser/mod.rs`.

use std::cell::RefCell;

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

/// Which bucket a rewrite belongs to. Passed in by the caller — every
/// `trace_on_change` site knows its own stage statically, so the trace module
/// never has to guess a stage from the pass name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Stage {
    Normalize,
    Segment,
    Refine,
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

impl Default for StageReport {
    fn default() -> Self {
        Self::empty("")
    }
}

impl StageReport {
    pub(super) fn empty(input: &str) -> Self {
        Self {
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

pub(crate) fn finish_recording(result: &str) -> StageReport {
    STAGE_REPORT.with(|slot| {
        let mut report = slot
            .borrow_mut()
            .take()
            .unwrap_or_else(|| StageReport::empty(""));
        report.result_preview = Some(result.to_string());
        report
    })
}

pub(crate) fn record_rewrite(stage: Stage, id: &str, before: &str, after: &str, changed: bool) {
    if !changed {
        return;
    }
    STAGE_REPORT.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(report) = slot.as_mut() else { return };
        let rewrite = StageRewrite {
            name: id.to_string(),
            before: before.to_string(),
            after: after.to_string(),
        };
        match stage {
            Stage::Normalize => report.normalize.push(rewrite),
            Stage::Segment => report.segment.push(rewrite),
            Stage::Refine => report.refine.push(rewrite),
        }
    });
}

pub(crate) fn record_recognizer(id: &str, output: Option<&str>) {
    STAGE_REPORT.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(report) = slot.as_mut() else { return };
        report.recognizers.push(RecognizerAttempt {
            name: id.to_string(),
            output: output.map(str::to_string),
        });
    });
}

pub(crate) fn record_grammar(outcome: GrammarOutcome) {
    STAGE_REPORT.with(|slot| {
        let mut slot = slot.borrow_mut();
        let Some(report) = slot.as_mut() else { return };
        report.grammar = Some(outcome);
    });
}
