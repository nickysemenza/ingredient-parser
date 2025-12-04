//! Tests for parse tracing functionality

#![allow(clippy::unwrap_used)]

use ingredient::trace::{
    disable_tracing, enable_tracing, trace_enter, trace_exit_success, ParseTrace, TraceNode,
    TraceOutcome,
};

#[test]
fn test_trace_node() {
    // Creation
    let node = TraceNode::new("test_parser", "some input text");
    assert_eq!(node.name, "test_parser");
    assert_eq!(node.input, "some input text");
    assert!(matches!(node.outcome, TraceOutcome::Incomplete));

    // Success outcome
    let mut success_node = TraceNode::new("test", "input");
    success_node.success(5, "value: 42");
    assert!(matches!(success_node.outcome, TraceOutcome::Success { .. }));

    // Failure outcome
    let mut failure_node = TraceNode::new("test", "input");
    failure_node.failure("expected number");
    assert!(matches!(failure_node.outcome, TraceOutcome::Failure { .. }));

    // Tree formatting
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

#[test]
fn test_trace_exit_failure() {
    use ingredient::trace::trace_exit_failure;

    enable_tracing();
    trace_enter("failing_parser", "bad input");
    trace_exit_failure("expected digit");

    let trace = disable_tracing("bad input");
    assert_eq!(trace.root.name, "failing_parser");
    assert!(matches!(trace.root.outcome, TraceOutcome::Failure { .. }));
    if let TraceOutcome::Failure { error } = &trace.root.outcome {
        assert_eq!(error, "expected digit");
    }
}

#[test]
fn test_is_tracing_enabled() {
    use ingredient::trace::is_tracing_enabled;

    // Initially tracing should be disabled
    assert!(!is_tracing_enabled());

    enable_tracing();
    assert!(is_tracing_enabled());

    disable_tracing("test");
    assert!(!is_tracing_enabled());
}

#[test]
fn test_trace_duration_micros() {
    let mut node = TraceNode::new("timed_parser", "input");
    // Before completion, no end_time
    assert!(node.end_time.is_none());

    // After success, should have timing
    node.success(5, "result");
    assert!(node.end_time.is_some());

    // Duration should be available (start and end are set)
    // Since start_time and end_time are both Some, duration should be Some
    assert!(node.start_time.is_some());
}

#[test]
fn test_nested_tracing() {
    enable_tracing();

    // Simulate nested parser calls
    trace_enter("outer", "full input");
    trace_enter("inner", "full input");
    trace_exit_success(4, "inner result");
    trace_exit_success(10, "outer result");

    let trace = disable_tracing("full input");

    // The outer parser should have the inner as a child
    assert_eq!(trace.root.name, "outer");
    assert_eq!(trace.root.children.len(), 1);
    assert_eq!(trace.root.children[0].name, "inner");
}

#[test]
fn test_parse_trace_display() {
    let trace = ParseTrace::new("test input");

    // Display trait should produce formatted output
    let output = format!("{}", trace);
    assert!(output.contains("parse_ingredient"));
}

#[test]
fn test_format_tree_colored() {
    let mut root = TraceNode::new("root", "input");
    root.success(5, "done");

    let mut child_success = TraceNode::new("success_child", "input");
    child_success.success(3, "ok");
    root.add_child(child_success);

    let mut child_fail = TraceNode::new("fail_child", "input");
    child_fail.failure("nope");
    root.add_child(child_fail);

    let trace = ParseTrace {
        input: "input".to_string(),
        root,
        baseline_instant: None,
        baseline_unix_micros: 0,
    };

    // Test colored output (contains ANSI escape codes)
    let colored = trace.format_tree(true);
    assert!(colored.contains("\x1b[32m")); // green for success
    assert!(colored.contains("\x1b[31m")); // red for failure
    assert!(colored.contains("\x1b[1m")); // bold for names

    // Test non-colored output (no ANSI codes)
    let plain = trace.format_tree(false);
    assert!(!plain.contains("\x1b["));
}

#[test]
fn test_truncate_input_in_trace_node() {
    // Very long input should be truncated
    let long_input = "a".repeat(100);
    let node = TraceNode::new("test", &long_input);

    // Input should be truncated to 40 chars + "..."
    assert!(node.input.len() <= 43);
    assert!(node.input.ends_with("..."));
}

#[test]
fn test_trace_incomplete_outcome() {
    let node = TraceNode::new("incomplete", "input");

    // Default outcome is Incomplete
    assert!(matches!(node.outcome, TraceOutcome::Incomplete));

    // Format tree should show "..." for incomplete
    let trace = ParseTrace {
        input: "input".to_string(),
        root: node,
        baseline_instant: None,
        baseline_unix_micros: 0,
    };

    let output = trace.format_tree(false);
    assert!(output.contains("..."));
}
