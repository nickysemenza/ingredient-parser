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

pub use collector::{disable_tracing, enable_tracing, is_tracing_enabled};
pub use collector::{trace_enter, trace_exit_failure, trace_exit_success};

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
