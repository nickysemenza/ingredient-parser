//! Debug tracing for ingredient parsing
//!
//! This module provides infrastructure to trace which parser functions are called
//! during ingredient parsing, including which `alt()` branches are tried.
//!
//! # Example
//!
//! ```
//! use ingredient::IngredientParser;
//!
//! let parser = IngredientParser::new();
//! let result = parser.parse_with_trace("2 cups flour");
//! println!("{}", result.trace.format_tree(true));
//! ```

mod collector;
mod format;
mod jaeger;
mod stages;

pub use collector::is_tracing_enabled;
pub use stages::{GrammarOutcome, RecognizerAttempt, StageReport, StageRewrite};
// Thread-local span-stack mutators: in-crate only (the `traced_parser!` macro and
// the pipeline/recognize/refine phases). Not part of the public API — the public
// entry point is `IngredientParser::parse_with_trace` → `ParseTrace`.
pub(crate) use collector::{disable_tracing, enable_tracing};
pub(crate) use collector::{trace_enter, trace_exit_failure, trace_exit_success};

/// The full label universe of each pipeline stage — every normalize rewrite,
/// recognizer, and refine pass that *could* fire, in declared order.
///
/// This is a tooling/introspection surface (used by `food-cli corpus lint
/// --report-stages` to detect rules with zero corpus coverage). It is *not* a
/// per-parse result — for that, parse with [`ParseTrace::stages`], which reports
/// only the stages that actually fired on a given line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PipelineStageNames {
    /// Every normalize rewrite label, in pipeline order.
    pub normalize: &'static [&'static str],
    /// Every whole-line recognizer label, in attempt order.
    pub recognizers: &'static [&'static str],
    /// Every segment-stage label (clause kinds in classifier order, then the
    /// assembly repairs), in emit order.
    pub segment: &'static [&'static str],
    /// Every refine pass label, in pipeline order.
    pub refine: &'static [&'static str],
}

/// The full label universe of the parser's three ordered stage pipelines.
///
/// A tooling/introspection API: it exposes the *static* set of rules the parser
/// could apply, so external tooling (e.g. dead-rule detection over the accuracy
/// corpus) can compare it against the rules that actually fire. It carries no
/// per-line state; use [`IngredientParser::parse_with_trace`] for that.
///
/// [`IngredientParser::parse_with_trace`]: crate::IngredientParser::parse_with_trace
pub fn pipeline_stage_names() -> PipelineStageNames {
    PipelineStageNames {
        normalize: crate::parser::normalize::REWRITE_TRACE_NAMES,
        recognizers: crate::parser::recognize::RECOGNIZER_TRACE_NAMES,
        segment: crate::parser::segment::SEGMENT_TRACE_NAMES,
        refine: crate::parser::refine::REFINE_TRACE_NAMES,
    }
}

/// Trace a stage step that may or may not change its input (normalize rewrites,
/// refine passes). Emits a before→after node only when `changed` is true.
pub(crate) fn trace_on_change(id: &str, before: &str, after: &str, changed: bool) {
    if changed && is_tracing_enabled() {
        trace_enter(id, before);
        trace_exit_success(0, after);
    }
}

/// Trace one recognizer attempt. Returns `result` unchanged after optionally
/// recording success or "no match" in the trace tree.
pub(crate) fn trace_attempt<T>(
    id: &str,
    input: &str,
    result: Option<T>,
    format_success: impl FnOnce(&T) -> String,
) -> Option<T> {
    if is_tracing_enabled() {
        trace_enter(id, input);
        match &result {
            Some(value) => trace_exit_success(0, &format_success(value)),
            None => trace_exit_failure("no match"),
        }
    }
    result
}

use std::fmt;
use std::time::Instant;

/// A node in the parse trace tree
#[derive(Debug, Clone)]
pub struct TraceNode {
    /// Name of the parser/combinator
    pub name: String,
    /// Input at this parse point (truncated for display)
    pub input: String,
    /// Child parser attempts
    pub children: Vec<TraceNode>,
    /// Outcome of this parser
    pub outcome: TraceOutcome,
    /// When this parser started (for timing)
    pub start_time: Option<Instant>,
    /// When this parser finished (for timing)
    pub end_time: Option<Instant>,
}

impl TraceNode {
    /// Create a new trace node (always captures timing)
    pub fn new(name: impl Into<String>, input: &str) -> Self {
        Self {
            name: name.into(),
            input: crate::util::truncate_str(input, 40),
            children: Vec::new(),
            outcome: TraceOutcome::Incomplete,
            start_time: Some(Instant::now()),
            end_time: None,
        }
    }

    /// Mark this node as successful
    pub fn success(&mut self, consumed: usize, output_preview: impl Into<String>) {
        self.outcome = TraceOutcome::Success {
            consumed,
            output_preview: output_preview.into(),
        };
        self.end_time = Some(Instant::now());
    }

    /// Mark this node as failed
    pub fn failure(&mut self, error: impl Into<String>) {
        self.outcome = TraceOutcome::Failure {
            error: error.into(),
        };
        self.end_time = Some(Instant::now());
    }

    /// Get duration in microseconds if timing is available
    pub(crate) fn duration_micros(&self) -> Option<u64> {
        match (self.start_time, self.end_time) {
            (Some(start), Some(end)) => Some(end.duration_since(start).as_micros() as u64),
            _ => None,
        }
    }

    /// Add a child node
    pub fn add_child(&mut self, child: TraceNode) {
        self.children.push(child);
    }
}

/// Outcome of a parse attempt
#[derive(Debug, Clone)]
pub enum TraceOutcome {
    /// Parser succeeded
    Success {
        /// Number of characters consumed
        consumed: usize,
        /// Preview of what was produced
        output_preview: String,
    },
    /// Parser failed
    Failure {
        /// Error description
        error: String,
    },
    /// Parse still in progress (incomplete)
    Incomplete,
}

/// Full parse trace for an ingredient
#[derive(Debug, Clone)]
pub struct ParseTrace {
    /// Original input string
    pub input: String,
    /// Root of the trace tree
    pub root: TraceNode,
    /// Baseline instant for converting node times to unix time
    pub baseline_instant: Option<Instant>,
    /// Unix timestamp (microseconds) when tracing started
    pub baseline_unix_micros: u64,
}

impl ParseTrace {
    /// Create a new parse trace
    pub fn new(input: &str) -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        Self {
            input: input.to_string(),
            root: TraceNode::new("parse_ingredient", input),
            baseline_instant: None,
            baseline_unix_micros: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_micros() as u64)
                .unwrap_or(0),
        }
    }

    /// Format the trace as a tree string for display
    ///
    /// # Arguments
    /// * `colored` - Whether to use ANSI color codes
    pub fn format_tree(&self, colored: bool) -> String {
        format::format_tree(&self.root, colored)
    }

    /// Format the trace as a compact, stage-level report (normalize → recognize
    /// → grammar → refine → result), collapsing the grammar's combinator subtree.
    ///
    /// Where [`format_tree`](Self::format_tree) shows *how* the grammar parsed,
    /// this shows *which pipeline stage* shaped the result — the view for
    /// deciding where a corpus fix belongs.
    ///
    /// # Arguments
    /// * `colored` - Whether to use ANSI color codes
    pub fn format_stages(&self, colored: bool) -> String {
        format::format_stages(&self.stages(), colored)
    }

    /// Bucket the trace into a structured stage-level report (the data behind
    /// [`format_stages`](Self::format_stages)) — normalize rewrites, recognizer
    /// attempts, grammar outcome, refine passes, and the result preview.
    pub fn stages(&self) -> StageReport {
        stages::build_report(&self.root)
    }

    /// Export trace to Jaeger-compatible JSON format
    pub fn to_jaeger_json(&self) -> String {
        jaeger::to_jaeger_json(self)
    }
}

/// Result of parsing with trace
#[derive(Debug, Clone)]
pub struct ParseWithTrace<T> {
    /// The parse result
    pub result: Result<T, String>,
    /// The trace of parser execution
    pub trace: ParseTrace,
}

impl fmt::Display for ParseTrace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.format_tree(false))
    }
}

/// Macro to wrap a parser with tracing, reducing boilerplate.
///
/// # Arguments
/// * `$name` - The name of the parser for trace output
/// * `$input` - The input string being parsed
/// * `$parser` - The parser expression to execute
/// * `$format` - A closure to format successful output for the trace
/// * `$error` - The error message for failures
///
/// # Example
/// ```ignore
/// fn parse_number(&self, input: &str) -> Res<&str, f64> {
///     traced_parser!(
///         "parse_number",
///         input,
///         context("number", double).parse(input),
///         |v: &f64| format!("{v}"),
///         "no number"
///     )
/// }
/// ```
#[macro_export]
macro_rules! traced_parser {
    ($name:expr, $input:expr, $parser:expr, $format:expr, $error:expr) => {{
        use $crate::trace::{
            is_tracing_enabled, trace_enter, trace_exit_failure, trace_exit_success,
        };
        let tracing = is_tracing_enabled();
        if tracing {
            trace_enter($name, $input);
        }
        let result = $parser;
        if tracing {
            match &result {
                Ok((remaining, value)) => {
                    let consumed = $input.len() - remaining.len();
                    trace_exit_success(consumed, &$format(value));
                }
                Err(_) => trace_exit_failure($error),
            }
        }
        result
    }};
}
