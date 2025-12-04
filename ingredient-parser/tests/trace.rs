//! Tests for parse tracing functionality

#![allow(clippy::unwrap_used)]

mod common;

use ingredient::trace::{
    disable_tracing, enable_tracing, trace_enter, trace_exit_success, ParseTrace, TraceNode,
    TraceOutcome,
};

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
