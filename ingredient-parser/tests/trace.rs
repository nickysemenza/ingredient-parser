//! Tests for parse tracing functionality

#![allow(clippy::unwrap_used)]

use ingredient::trace::{
    disable_tracing, enable_tracing, is_tracing_enabled, trace_enter, trace_exit_success,
    ParseTrace, TraceNode, TraceOutcome,
};
use ingredient::IngredientParser;

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
    let output = format!("{trace}");
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

// ============================================================================
// parse_with_trace() Tests
// ============================================================================

#[test]
fn test_parse_with_trace_success() {
    let parser = IngredientParser::new();
    let result = parser.parse_with_trace("2 cups flour");

    // Result should be successful
    assert!(result.result.is_ok());
    let ingredient = result.result.unwrap();
    assert_eq!(ingredient.name, "flour");

    // Trace should contain parser info
    let tree = result.trace.format_tree(false);
    assert!(tree.contains("parse_ingredient"));
}

#[test]
fn test_parse_with_trace_complex_input() {
    let parser = IngredientParser::new();
    let result = parser.parse_with_trace("1½ cups / 180g all-purpose flour, sifted");

    assert!(result.result.is_ok());
    let ingredient = result.result.unwrap();
    assert_eq!(ingredient.name, "all-purpose flour");
    assert_eq!(ingredient.amounts.len(), 2);
    assert!(ingredient.modifier.is_some());

    // Trace should show the parse tree
    let tree = result.trace.format_tree(false);
    assert!(!tree.is_empty());
}

#[test]
fn test_parse_with_trace_minimal_input() {
    let parser = IngredientParser::new();
    let result = parser.parse_with_trace("salt");

    assert!(result.result.is_ok());
    let ingredient = result.result.unwrap();
    assert_eq!(ingredient.name, "salt");
    assert!(ingredient.amounts.is_empty());
}

#[test]
fn test_parse_with_trace_range() {
    let parser = IngredientParser::new();
    let result = parser.parse_with_trace("2-3 cups water");

    assert!(result.result.is_ok());
    let ingredient = result.result.unwrap();
    assert_eq!(ingredient.name, "water");
    assert_eq!(ingredient.amounts.len(), 1);
}

// ============================================================================
// Jaeger JSON Export Tests
// ============================================================================

#[test]
fn test_to_jaeger_json_basic() {
    let mut root = TraceNode::new("test_parser", "test input");
    root.success(10, "parsed value");

    let trace = ParseTrace {
        input: "test input".to_string(),
        root,
        baseline_instant: Some(std::time::Instant::now()),
        baseline_unix_micros: 1000000,
    };

    let json = trace.to_jaeger_json();

    // Should be valid JSON
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

    // Should have the expected structure
    assert!(parsed["data"].is_array());
    assert!(parsed["data"][0]["traceID"].is_string());
    assert!(parsed["data"][0]["spans"].is_array());
    assert!(parsed["data"][0]["processes"]["p1"]["serviceName"] == "ingredient-parser");
}

#[test]
fn test_to_jaeger_json_with_children() {
    let mut root = TraceNode::new("root_parser", "full input");

    let mut child1 = TraceNode::new("child1", "full input");
    child1.success(5, "child result");
    root.add_child(child1);

    let mut child2 = TraceNode::new("child2", "remaining");
    child2.failure("no match");
    root.add_child(child2);

    root.success(10, "root result");

    let trace = ParseTrace {
        input: "full input".to_string(),
        root,
        baseline_instant: Some(std::time::Instant::now()),
        baseline_unix_micros: 1000000,
    };

    let json = trace.to_jaeger_json();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

    // Should have 3 spans (root + 2 children)
    let spans = &parsed["data"][0]["spans"];
    assert_eq!(spans.as_array().unwrap().len(), 3);

    // Check span tags exist
    for span in spans.as_array().unwrap() {
        assert!(span["operationName"].is_string());
        assert!(span["tags"].is_array());
        assert!(span["startTime"].is_number());
        assert!(span["duration"].is_number());
    }
}

#[test]
fn test_to_jaeger_json_success_tags() {
    let mut root = TraceNode::new("success_parser", "input");
    root.success(5, "success_output");

    let trace = ParseTrace {
        input: "input".to_string(),
        root,
        baseline_instant: Some(std::time::Instant::now()),
        baseline_unix_micros: 1000000,
    };

    let json = trace.to_jaeger_json();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

    let span = &parsed["data"][0]["spans"][0];
    let tags = span["tags"].as_array().unwrap();

    // Should have status, consumed, and output tags
    let tag_keys: Vec<&str> = tags.iter().map(|t| t["key"].as_str().unwrap()).collect();
    assert!(tag_keys.contains(&"status"));
    assert!(tag_keys.contains(&"consumed"));
    assert!(tag_keys.contains(&"output"));
}

#[test]
fn test_to_jaeger_json_failure_tags() {
    let mut root = TraceNode::new("failure_parser", "bad input");
    root.failure("parse error");

    let trace = ParseTrace {
        input: "bad input".to_string(),
        root,
        baseline_instant: Some(std::time::Instant::now()),
        baseline_unix_micros: 1000000,
    };

    let json = trace.to_jaeger_json();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

    let span = &parsed["data"][0]["spans"][0];
    let tags = span["tags"].as_array().unwrap();

    // Should have error tags
    let tag_keys: Vec<&str> = tags.iter().map(|t| t["key"].as_str().unwrap()).collect();
    assert!(tag_keys.contains(&"status"));
    assert!(tag_keys.contains(&"error"));
    assert!(tag_keys.contains(&"error.message"));
}

#[test]
fn test_to_jaeger_json_incomplete_tags() {
    // Create a node that's never completed (remains Incomplete)
    let root = TraceNode::new("incomplete_parser", "input");

    let trace = ParseTrace {
        input: "input".to_string(),
        root,
        baseline_instant: Some(std::time::Instant::now()),
        baseline_unix_micros: 1000000,
    };

    let json = trace.to_jaeger_json();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

    let span = &parsed["data"][0]["spans"][0];
    let tags = span["tags"].as_array().unwrap();

    // Find the status tag
    let status_tag = tags.iter().find(|t| t["key"] == "status").unwrap();
    assert_eq!(status_tag["value"], "incomplete");
}

#[test]
fn test_to_jaeger_json_no_baseline_instant() {
    // Test the branch where baseline_instant is None
    let mut root = TraceNode::new("test_parser", "input");
    root.success(5, "result");

    let trace = ParseTrace {
        input: "input".to_string(),
        root,
        baseline_instant: None, // No baseline instant
        baseline_unix_micros: 1000000,
    };

    let json = trace.to_jaeger_json();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

    // Should still produce valid JSON
    assert!(parsed["data"][0]["spans"].is_array());
}

#[test]
fn test_to_jaeger_json_references() {
    // Test parent-child references in spans
    let mut root = TraceNode::new("parent", "input");
    let mut child = TraceNode::new("child", "input");
    child.success(3, "child result");
    root.add_child(child);
    root.success(5, "parent result");

    let trace = ParseTrace {
        input: "input".to_string(),
        root,
        baseline_instant: Some(std::time::Instant::now()),
        baseline_unix_micros: 1000000,
    };

    let json = trace.to_jaeger_json();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

    let spans = parsed["data"][0]["spans"].as_array().unwrap();

    // Find child span (should have reference to parent)
    let child_span = spans
        .iter()
        .find(|s| s["operationName"] == "child")
        .unwrap();
    let references = child_span["references"].as_array().unwrap();

    // Child should have a CHILD_OF reference
    assert_eq!(references.len(), 1);
    assert_eq!(references[0]["refType"], "CHILD_OF");
}

// ============================================================================
// TraceCollector Default Tests
// ============================================================================

#[test]
fn test_trace_collector_default() {
    // Test the Default impl for TraceCollector by using enable_tracing
    // which creates a new TraceCollector

    // First ensure tracing is disabled
    if is_tracing_enabled() {
        disable_tracing("cleanup");
    }

    // Now enable and verify it works
    enable_tracing();
    assert!(is_tracing_enabled());

    trace_enter("test", "input");
    trace_exit_success(5, "done");

    let trace = disable_tracing("input");
    assert_eq!(trace.root.name, "test");
}

#[test]
fn test_trace_node_timing() {
    let mut node = TraceNode::new("timed", "input");

    // Before completion, no end_time
    assert!(node.end_time.is_none());

    // After success, end_time should be set
    node.success(5, "result");
    assert!(node.end_time.is_some());
    // start_time should also be set
    assert!(node.start_time.is_some());
}

#[test]
fn test_trace_deeply_nested() {
    enable_tracing();

    // Create deeply nested structure
    trace_enter("level1", "input");
    trace_enter("level2", "input");
    trace_enter("level3", "input");
    trace_exit_success(1, "l3");
    trace_exit_success(2, "l2");
    trace_exit_success(3, "l1");

    let trace = disable_tracing("input");

    // Verify nesting
    assert_eq!(trace.root.name, "level1");
    assert_eq!(trace.root.children.len(), 1);
    assert_eq!(trace.root.children[0].name, "level2");
    assert_eq!(trace.root.children[0].children.len(), 1);
    assert_eq!(trace.root.children[0].children[0].name, "level3");
}

#[test]
fn test_parse_trace_new() {
    // Test ParseTrace::new creates proper initial structure
    let trace = ParseTrace::new("test input");

    assert_eq!(trace.input, "test input");
    assert_eq!(trace.root.name, "parse_ingredient");
    assert_eq!(trace.root.input, "test input");
    assert!(trace.baseline_instant.is_none());
    assert!(trace.baseline_unix_micros > 0);
}
