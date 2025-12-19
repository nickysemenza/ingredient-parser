//! Thread-local trace collection

use super::{ParseTrace, TraceNode};
use std::cell::RefCell;
use std::time::Instant;

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
            .map(|d| d.as_micros() as u64)
            .unwrap_or(0);
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

// Thread-local storage for trace collection
thread_local! {
    static TRACE_COLLECTOR: RefCell<Option<TraceCollector>> = const { RefCell::new(None) };
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
pub fn trace_enter(name: &str, input: &str) {
    TRACE_COLLECTOR.with(|tc| {
        if let Some(ref mut collector) = *tc.borrow_mut() {
            collector.enter(name, input);
        }
    });
}

/// Exit parser context with success (if tracing is enabled)
pub fn trace_exit_success(consumed: usize, output_preview: &str) {
    TRACE_COLLECTOR.with(|tc| {
        if let Some(ref mut collector) = *tc.borrow_mut() {
            collector.exit_success(consumed, output_preview);
        }
    });
}

/// Exit parser context with failure (if tracing is enabled)
pub fn trace_exit_failure(error: &str) {
    TRACE_COLLECTOR.with(|tc| {
        if let Some(ref mut collector) = *tc.borrow_mut() {
            collector.exit_failure(error);
        }
    });
}

/// Check if tracing is currently enabled for this thread
///
/// Use this to avoid expensive formatting operations when tracing is disabled.
pub fn is_tracing_enabled() -> bool {
    TRACE_COLLECTOR.with(|tc| tc.borrow().is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trace_collector_default_impl() {
        // Test the Default trait impl for TraceCollector
        let collector: TraceCollector = Default::default();

        // Should have an empty stack initially
        assert!(collector.stack.is_empty());

        // Should have valid timestamps
        assert!(collector.baseline_unix_micros > 0);
    }

    #[test]
    fn test_trace_collector_lifecycle() {
        let mut collector = TraceCollector::new();

        // Enter a parser
        collector.enter("test_parser", "input text");

        // Exit with success
        collector.exit_success(5, "output");

        // Finish and get the trace
        let trace = collector.finish("input text");

        assert_eq!(trace.root.name, "test_parser");
    }

    #[test]
    fn test_trace_collector_nested() {
        let mut collector = TraceCollector::new();

        // Enter outer parser
        collector.enter("outer", "full input");

        // Enter inner parser
        collector.enter("inner", "full input");

        // Exit inner with failure
        collector.exit_failure("no match");

        // Exit outer with success
        collector.exit_success(10, "result");

        let trace = collector.finish("full input");

        assert_eq!(trace.root.name, "outer");
        assert_eq!(trace.root.children.len(), 1);
        assert_eq!(trace.root.children[0].name, "inner");
    }

    #[test]
    fn test_trace_collector_empty_finish() {
        // Test finishing with no entries
        let collector = TraceCollector::new();
        let trace = collector.finish("input");

        // Should create a default root node
        assert_eq!(trace.root.name, "parse_ingredient");
    }
}
