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
//! let parser = IngredientParser::new(false);
//! let result = parser.parse_with_trace("2 cups flour");
//! println!("{}", result.trace.format_tree(true));
//! ```

use std::cell::RefCell;
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
            input: truncate_input(input, 40),
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
                .unwrap()
                .as_micros() as u64,
        }
    }

    /// Format the trace as a tree string for display
    ///
    /// # Arguments
    /// * `colored` - Whether to use ANSI color codes
    pub fn format_tree(&self, colored: bool) -> String {
        let mut output = String::new();
        format_node(&self.root, &mut output, "", true, colored);
        output
    }

    /// Export trace to Jaeger-compatible JSON format
    pub fn to_jaeger_json(&self) -> String {
        use rand::Rng;

        // Generate trace ID (16 bytes as hex = 32 chars)
        let mut rng = rand::rng();
        let trace_id: String = (0..32)
            .map(|_| format!("{:x}", rng.random_range(0..16u8)))
            .collect();

        // Collect spans from tree
        let mut spans = Vec::new();
        let mut span_counter = 0u64;
        self.collect_spans(
            &self.root,
            &trace_id,
            None,
            &mut spans,
            &mut span_counter,
        );

        // Build Jaeger JSON structure
        let json = serde_json::json!({
            "data": [{
                "traceID": trace_id,
                "spans": spans,
                "processes": {
                    "p1": {
                        "serviceName": "ingredient-parser",
                        "tags": []
                    }
                }
            }]
        });

        serde_json::to_string_pretty(&json).unwrap_or_else(|_| "{}".to_string())
    }

    /// Recursively collect spans from the trace tree
    fn collect_spans(
        &self,
        node: &TraceNode,
        trace_id: &str,
        parent_span_id: Option<&str>,
        spans: &mut Vec<serde_json::Value>,
        span_counter: &mut u64,
    ) {
        use rand::Rng;

        // Generate span ID
        let mut rng = rand::rng();
        let span_id: String = (0..16)
            .map(|_| format!("{:x}", rng.random_range(0..16u8)))
            .collect();

        // Calculate start time in unix microseconds
        let start_time = if let (Some(baseline), Some(node_start)) =
            (self.baseline_instant, node.start_time)
        {
            let offset = node_start.duration_since(baseline).as_micros() as u64;
            self.baseline_unix_micros + offset
        } else {
            self.baseline_unix_micros + *span_counter
        };

        // Calculate duration
        let duration = node.duration_micros().unwrap_or(1);

        // Build references (parent relationship)
        let references: Vec<serde_json::Value> = parent_span_id
            .map(|parent_id| {
                vec![serde_json::json!({
                    "refType": "CHILD_OF",
                    "traceID": trace_id,
                    "spanID": parent_id
                })]
            })
            .unwrap_or_default();

        // Build tags
        let mut tags = vec![
            serde_json::json!({"key": "input", "type": "string", "value": node.input}),
        ];

        match &node.outcome {
            TraceOutcome::Success { consumed, output_preview } => {
                tags.push(serde_json::json!({"key": "status", "type": "string", "value": "success"}));
                tags.push(serde_json::json!({"key": "consumed", "type": "int64", "value": *consumed as i64}));
                tags.push(serde_json::json!({"key": "output", "type": "string", "value": output_preview}));
            }
            TraceOutcome::Failure { error } => {
                tags.push(serde_json::json!({"key": "status", "type": "string", "value": "failure"}));
                tags.push(serde_json::json!({"key": "error", "type": "bool", "value": true}));
                tags.push(serde_json::json!({"key": "error.message", "type": "string", "value": error}));
            }
            TraceOutcome::Incomplete => {
                tags.push(serde_json::json!({"key": "status", "type": "string", "value": "incomplete"}));
            }
        }

        // Create span
        let span = serde_json::json!({
            "traceID": trace_id,
            "spanID": span_id,
            "operationName": node.name,
            "references": references,
            "startTime": start_time,
            "duration": duration,
            "tags": tags,
            "logs": [],
            "processID": "p1"
        });

        spans.push(span);
        *span_counter += 1;

        // Process children
        for child in &node.children {
            self.collect_spans(child, trace_id, Some(&span_id), spans, span_counter);
        }
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

// Thread-local storage for trace collection
thread_local! {
    static TRACE_COLLECTOR: RefCell<Option<TraceCollector>> = const { RefCell::new(None) };
}

/// Collects trace information during parsing
#[derive(Debug)]
pub(crate) struct TraceCollector {
    /// Stack of nodes being built (parent -> child relationship)
    stack: Vec<TraceNode>,
    /// Baseline instant for converting to unix time
    baseline_instant: Instant,
    /// Unix timestamp (microseconds) when tracing started
    baseline_unix_micros: u64,
}

impl TraceCollector {
    /// Create a new trace collector
    pub(crate) fn new() -> Self {
        use std::time::{SystemTime, UNIX_EPOCH};
        let baseline_instant = Instant::now();
        let baseline_unix_micros = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64;
        Self {
            stack: Vec::new(),
            baseline_instant,
            baseline_unix_micros,
        }
    }

    /// Enter a new parser context
    pub(crate) fn enter(&mut self, name: &str, input: &str) {
        let node = TraceNode::new(name, input);
        self.stack.push(node);
    }

    /// Exit the current parser context with success
    pub(crate) fn exit_success(&mut self, consumed: usize, output_preview: &str) {
        if let Some(mut node) = self.stack.pop() {
            node.success(consumed, output_preview);
            self.attach_to_parent(node);
        }
    }

    /// Exit the current parser context with failure
    pub(crate) fn exit_failure(&mut self, error: &str) {
        if let Some(mut node) = self.stack.pop() {
            node.failure(error);
            self.attach_to_parent(node);
        }
    }

    /// Attach a completed node to its parent (or keep as root)
    fn attach_to_parent(&mut self, node: TraceNode) {
        if let Some(parent) = self.stack.last_mut() {
            parent.add_child(node);
        } else {
            // This is the root node, push it back
            self.stack.push(node);
        }
    }

    /// Finish tracing and return the root trace
    pub(crate) fn finish(mut self, input: &str) -> ParseTrace {
        let root = if let Some(node) = self.stack.pop() {
            node
        } else {
            TraceNode::new("parse_ingredient", input)
        };

        ParseTrace {
            input: input.to_string(),
            root,
            baseline_instant: Some(self.baseline_instant),
            baseline_unix_micros: self.baseline_unix_micros,
        }
    }
}

impl Default for TraceCollector {
    fn default() -> Self {
        Self::new()
    }
}

// Public API for interacting with thread-local collector

/// Enable tracing for the current thread
pub fn enable_tracing() {
    TRACE_COLLECTOR.with(|tc| {
        *tc.borrow_mut() = Some(TraceCollector::new());
    });
}

/// Disable tracing and retrieve the collected trace
pub fn disable_tracing(input: &str) -> ParseTrace {
    TRACE_COLLECTOR.with(|tc| {
        tc.borrow_mut()
            .take()
            .map(|c| c.finish(input))
            .unwrap_or_else(|| ParseTrace::new(input))
    })
}

/// Enter a parser context (if tracing is enabled)
pub(crate) fn trace_enter(name: &str, input: &str) {
    TRACE_COLLECTOR.with(|tc| {
        if let Some(ref mut collector) = *tc.borrow_mut() {
            collector.enter(name, input);
        }
    });
}

/// Exit parser context with success (if tracing is enabled)
pub(crate) fn trace_exit_success(consumed: usize, output_preview: &str) {
    TRACE_COLLECTOR.with(|tc| {
        if let Some(ref mut collector) = *tc.borrow_mut() {
            collector.exit_success(consumed, output_preview);
        }
    });
}

/// Exit parser context with failure (if tracing is enabled)
pub(crate) fn trace_exit_failure(error: &str) {
    TRACE_COLLECTOR.with(|tc| {
        if let Some(ref mut collector) = *tc.borrow_mut() {
            collector.exit_failure(error);
        }
    });
}

// Helper functions for formatting

fn truncate_input(input: &str, max_len: usize) -> String {
    let char_count = input.chars().count();
    if char_count <= max_len {
        input.to_string()
    } else {
        let truncated: String = input.chars().take(max_len).collect();
        format!("{truncated}...")
    }
}

fn format_node(node: &TraceNode, output: &mut String, prefix: &str, is_last: bool, colored: bool) {
    // Determine the connector
    let connector = if is_last { "└─ " } else { "├─ " };

    // Format the outcome symbol
    let outcome_symbol = match &node.outcome {
        TraceOutcome::Success { output_preview, .. } => {
            if colored {
                format!("\x1b[32m✓\x1b[0m → {output_preview}")
            } else {
                format!("✓ → {output_preview}")
            }
        }
        TraceOutcome::Failure { .. } => {
            if colored {
                "\x1b[31m✗\x1b[0m".to_string()
            } else {
                "✗".to_string()
            }
        }
        TraceOutcome::Incomplete => "...".to_string(),
    };

    // Format the node name
    let name_display = if colored {
        format!("\x1b[1m{}\x1b[0m", node.name)
    } else {
        node.name.clone()
    };

    // Write this node
    output.push_str(&format!(
        "{}{}{} \"{}\" {}\n",
        prefix, connector, name_display, node.input, outcome_symbol
    ));

    // Format children
    let child_prefix = if is_last {
        format!("{prefix}   ")
    } else {
        format!("{prefix}│  ")
    };

    let child_count = node.children.len();
    for (idx, child) in node.children.iter().enumerate() {
        let is_last_child = idx == child_count - 1;
        format_node(child, output, &child_prefix, is_last_child, colored);
    }
}

impl fmt::Display for ParseTrace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.format_tree(false))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_node_creation() {
        let node = TraceNode::new("test_parser", "some input text");
        assert_eq!(node.name, "test_parser");
        assert_eq!(node.input, "some input text");
        assert!(matches!(node.outcome, TraceOutcome::Incomplete));
    }

    #[test]
    fn test_trace_node_success() {
        let mut node = TraceNode::new("test", "input");
        node.success(5, "value: 42");
        assert!(matches!(node.outcome, TraceOutcome::Success { .. }));
    }

    #[test]
    fn test_trace_node_failure() {
        let mut node = TraceNode::new("test", "input");
        node.failure("expected number");
        assert!(matches!(node.outcome, TraceOutcome::Failure { .. }));
    }

    #[test]
    fn test_trace_collector() {
        let mut collector = TraceCollector::new();

        collector.enter("outer", "test input");
        collector.enter("inner", "test input");
        collector.exit_success(4, "parsed");
        collector.exit_success(10, "full result");

        let trace = collector.finish("test input");
        assert_eq!(trace.root.name, "outer");
        assert_eq!(trace.root.children.len(), 1);
    }

    #[test]
    fn test_format_tree() {
        let mut root = TraceNode::new("root", "input text");
        root.success(10, "result");

        let mut child = TraceNode::new("child", "input text");
        child.failure("no match");
        root.add_child(child);

        let trace = ParseTrace {
            input: "input text".to_string(),
            root,
            baseline_instant: None,
            baseline_unix_micros: 0,
        };

        let output = trace.format_tree(false);
        assert!(output.contains("root"));
        assert!(output.contains("child"));
        assert!(output.contains("✓"));
        assert!(output.contains("✗"));
    }

    #[test]
    fn test_thread_local_tracing() {
        enable_tracing();

        trace_enter("test", "input");
        trace_exit_success(5, "done");

        let trace = disable_tracing("input");
        assert_eq!(trace.root.name, "test");
    }
}
