//! Tests for parse tracing functionality

#![allow(clippy::unwrap_used)]

use ingredient::trace::{
    disable_tracing, enable_tracing, is_tracing_enabled, trace_enter, trace_exit_failure,
    trace_exit_success, ParseTrace, TraceNode, TraceOutcome,
};
use ingredient::IngredientParser;
use rstest::{fixture, rstest};

// ============================================================================
// Fixtures
// ============================================================================

#[fixture]
fn parser() -> IngredientParser {
    IngredientParser::new()
}

#[fixture]
fn parser_with_adjectives() -> IngredientParser {
    IngredientParser::new().with_adjectives(&["sliced", "thinly sliced", "freshly ground"])
}

#[fixture]
fn parser_with_units() -> IngredientParser {
    IngredientParser::new().with_units(&["dash", "pinch", "handful"])
}

// ============================================================================
// TraceNode Tests
// ============================================================================

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
fn test_truncate_input_in_trace_node() {
    let long_input = "a".repeat(100);
    let node = TraceNode::new("test", &long_input);
    assert!(node.input.len() <= 43);
    assert!(node.input.ends_with("..."));
}

#[rstest]
#[case::before_completion(false)]
#[case::after_success(true)]
fn test_trace_node_timing(#[case] complete: bool) {
    let mut node = TraceNode::new("timed", "input");
    assert!(node.end_time.is_none());

    if complete {
        node.success(5, "result");
        assert!(node.end_time.is_some());
        assert!(node.start_time.is_some());
    }
}

// ============================================================================
// Thread-Local Tracing Tests
// ============================================================================

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
    assert!(!is_tracing_enabled());
    enable_tracing();
    assert!(is_tracing_enabled());
    disable_tracing("test");
    assert!(!is_tracing_enabled());
}

#[test]
fn test_nested_tracing() {
    enable_tracing();
    trace_enter("outer", "full input");
    trace_enter("inner", "full input");
    trace_exit_success(4, "inner result");
    trace_exit_success(10, "outer result");

    let trace = disable_tracing("full input");
    assert_eq!(trace.root.name, "outer");
    assert_eq!(trace.root.children.len(), 1);
    assert_eq!(trace.root.children[0].name, "inner");
}

#[test]
fn test_trace_deeply_nested() {
    enable_tracing();
    trace_enter("level1", "input");
    trace_enter("level2", "input");
    trace_enter("level3", "input");
    trace_exit_success(1, "l3");
    trace_exit_success(2, "l2");
    trace_exit_success(3, "l1");

    let trace = disable_tracing("input");
    assert_eq!(trace.root.name, "level1");
    assert_eq!(trace.root.children[0].name, "level2");
    assert_eq!(trace.root.children[0].children[0].name, "level3");
}

// ============================================================================
// ParseTrace Tests
// ============================================================================

#[test]
fn test_parse_trace_new() {
    let trace = ParseTrace::new("test input");
    assert_eq!(trace.input, "test input");
    assert_eq!(trace.root.name, "parse_ingredient");
    assert!(trace.baseline_instant.is_none());
    assert!(trace.baseline_unix_micros > 0);
}

#[test]
fn test_parse_trace_display() {
    let trace = ParseTrace::new("test input");
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

    let colored = trace.format_tree(true);
    assert!(colored.contains("\x1b[32m")); // green
    assert!(colored.contains("\x1b[31m")); // red
    assert!(colored.contains("\x1b[1m")); // bold

    let plain = trace.format_tree(false);
    assert!(!plain.contains("\x1b["));
}

#[test]
fn test_trace_incomplete_outcome() {
    let node = TraceNode::new("incomplete", "input");
    assert!(matches!(node.outcome, TraceOutcome::Incomplete));

    let trace = ParseTrace {
        input: "input".to_string(),
        root: node,
        baseline_instant: None,
        baseline_unix_micros: 0,
    };
    assert!(trace.format_tree(false).contains("..."));
}

// ============================================================================
// parse_with_trace() - Basic Tests
// ============================================================================

#[rstest]
fn test_parse_with_trace_success(parser: IngredientParser) {
    let result = parser.parse_with_trace("2 cups flour");
    assert!(result.result.is_ok());
    let ingredient = result.result.unwrap();
    assert_eq!(ingredient.name, "flour");
    assert!(result.trace.format_tree(false).contains("parse_ingredient"));
}

#[rstest]
fn test_parse_with_trace_minimal(parser: IngredientParser) {
    let result = parser.parse_with_trace("salt");
    assert!(result.result.is_ok());
    let ingredient = result.result.unwrap();
    assert_eq!(ingredient.name, "salt");
    assert!(ingredient.amounts.is_empty());
}

// ============================================================================
// parse_with_trace() - Parameterized Tests
// ============================================================================

/// Test various range formats
#[rstest]
#[case::hyphen("2-3 cups flour", "flour")]
#[case::em_dash("2–3 cups flour", "flour")]
#[case::to("2 to 3 cups water", "water")]
#[case::or("2 or 3 cups sugar", "sugar")]
#[case::through("2 through 3 cups flour", "flour")]
fn test_range_formats(parser: IngredientParser, #[case] input: &str, #[case] expected: &str) {
    let result = parser.parse_with_trace(input);
    assert!(result.result.is_ok(), "Failed to parse: {input}");
    assert_eq!(result.result.unwrap().name, expected);
}

/// Test various separator formats
#[rstest]
#[case::semicolon("2 cups; 1 tablespoon flour", 2)]
#[case::slash_spaces("1 cup / 240 ml flour", 2)]
#[case::slash_bare("1 cup/240 ml flour", 2)]
#[case::comma("1 cup, 2 tablespoons flour", 2)]
fn test_separator_formats(
    parser: IngredientParser,
    #[case] input: &str,
    #[case] expected_amounts: usize,
) {
    let result = parser.parse_with_trace(input);
    assert!(result.result.is_ok(), "Failed to parse: {input}");
    assert_eq!(result.result.unwrap().amounts.len(), expected_amounts);
}

/// Test fraction formats
#[rstest]
#[case::unicode_half("½ cup flour", 0.5)]
#[case::mixed_unicode("1½ cups flour", 1.5)]
#[case::slash_fraction("1/2 cup flour", 0.5)]
#[case::decimal("2.5 cups flour", 2.5)]
fn test_fraction_formats(parser: IngredientParser, #[case] input: &str, #[case] expected: f64) {
    let result = parser.parse_with_trace(input);
    assert!(result.result.is_ok(), "Failed to parse: {input}");
    let amount = result.result.unwrap().amounts[0].value();
    assert!(
        (amount - expected).abs() < 0.001,
        "Expected {expected}, got {amount}"
    );
}

/// Test special prefixes and suffixes
#[rstest]
#[case::about("about 2 cups flour", "flour")]
#[case::up_to("up to 5 cups flour", "flour")]
#[case::at_most("at most 5 cups flour", "flour")]
#[case::period_suffix("1 tsp. salt", "salt")]
#[case::of_suffix("1 cup of flour", "flour")]
#[case::text_number_one("one cup flour", "flour")]
fn test_special_formats(parser: IngredientParser, #[case] input: &str, #[case] expected: &str) {
    let result = parser.parse_with_trace(input);
    assert!(result.result.is_ok(), "Failed to parse: {input}");
    assert_eq!(result.result.unwrap().name, expected);
}

/// Test upper bound expressions return proper ranges
#[rstest]
#[case::up_to("up to 5 cups flour")]
#[case::at_most("at most 10 cups water")]
fn test_upper_bound_has_range(parser: IngredientParser, #[case] input: &str) {
    let result = parser.parse_with_trace(input);
    assert!(result.result.is_ok());
    let amount = &result.result.unwrap().amounts[0];
    assert!(
        amount.upper_value().is_some(),
        "Expected upper value for: {input}"
    );
}

/// Test complex inputs
#[rstest]
#[case::multi_unit("1½ cups / 180g all-purpose flour, sifted", "all-purpose flour", 2)]
#[case::multiplier("2 x 3 cups flour", "flour", 1)]
#[case::parenthesized("flour (2 cups)", "flour", 1)]
fn test_complex_inputs(
    parser: IngredientParser,
    #[case] input: &str,
    #[case] expected_name: &str,
    #[case] expected_amounts: usize,
) {
    let result = parser.parse_with_trace(input);
    assert!(result.result.is_ok(), "Failed to parse: {input}");
    let ingredient = result.result.unwrap();
    assert_eq!(ingredient.name, expected_name);
    assert_eq!(ingredient.amounts.len(), expected_amounts);
}

// ============================================================================
// Adjective Tests
// ============================================================================

#[rstest]
fn test_adjective_extraction(parser: IngredientParser) {
    let result = parser.parse_with_trace("1 cup chopped onion");
    assert!(result.result.is_ok());
    let ingredient = result.result.unwrap();
    assert_eq!(ingredient.name, "onion");
    assert!(ingredient.modifier.as_ref().unwrap().contains("chopped"));
}

#[rstest]
fn test_longer_adjective_matches_first(parser_with_adjectives: IngredientParser) {
    let result = parser_with_adjectives.parse_with_trace("1 cup thinly sliced onion");
    assert!(result.result.is_ok());
    let ingredient = result.result.unwrap();
    assert_eq!(ingredient.name, "onion");
    assert!(ingredient
        .modifier
        .as_ref()
        .unwrap()
        .contains("thinly sliced"));
}

#[rstest]
fn test_custom_adjectives(parser_with_adjectives: IngredientParser) {
    let result = parser_with_adjectives.parse_with_trace("1 cup freshly ground pepper");
    assert!(result.result.is_ok());
    let ingredient = result.result.unwrap();
    assert_eq!(ingredient.name, "pepper");
    assert!(ingredient
        .modifier
        .as_ref()
        .unwrap()
        .contains("freshly ground"));
}

// ============================================================================
// Custom Unit Tests
// ============================================================================

#[rstest]
fn test_custom_units(parser_with_units: IngredientParser) {
    let result = parser_with_units.parse_with_trace("pinch salt");
    assert!(result.result.is_ok());
    assert_eq!(result.result.unwrap().name, "salt");
}

// ============================================================================
// Jaeger JSON Export Tests
// ============================================================================

fn create_test_trace(with_children: bool, with_baseline: bool) -> ParseTrace {
    let mut root = TraceNode::new("test_parser", "test input");

    if with_children {
        let mut child1 = TraceNode::new("child1", "test input");
        child1.success(5, "child result");
        root.add_child(child1);

        let mut child2 = TraceNode::new("child2", "remaining");
        child2.failure("no match");
        root.add_child(child2);
    }

    root.success(10, "parsed value");

    ParseTrace {
        input: "test input".to_string(),
        root,
        baseline_instant: if with_baseline {
            Some(std::time::Instant::now())
        } else {
            None
        },
        baseline_unix_micros: 1000000,
    }
}

#[rstest]
#[case::basic(false, true)]
#[case::with_children(true, true)]
#[case::no_baseline(false, false)]
fn test_jaeger_json_structure(#[case] with_children: bool, #[case] with_baseline: bool) {
    let trace = create_test_trace(with_children, with_baseline);
    let json = trace.to_jaeger_json();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert!(parsed["data"].is_array());
    assert!(parsed["data"][0]["traceID"].is_string());
    assert!(parsed["data"][0]["spans"].is_array());
    assert_eq!(
        parsed["data"][0]["processes"]["p1"]["serviceName"],
        "ingredient-parser"
    );

    if with_children {
        assert_eq!(parsed["data"][0]["spans"].as_array().unwrap().len(), 3);
    }
}

#[test]
fn test_jaeger_json_success_tags() {
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
    let tags = parsed["data"][0]["spans"][0]["tags"].as_array().unwrap();
    let tag_keys: Vec<&str> = tags.iter().map(|t| t["key"].as_str().unwrap()).collect();

    assert!(tag_keys.contains(&"status"));
    assert!(tag_keys.contains(&"consumed"));
    assert!(tag_keys.contains(&"output"));
}

#[test]
fn test_jaeger_json_failure_tags() {
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
    let tags = parsed["data"][0]["spans"][0]["tags"].as_array().unwrap();
    let tag_keys: Vec<&str> = tags.iter().map(|t| t["key"].as_str().unwrap()).collect();

    assert!(tag_keys.contains(&"status"));
    assert!(tag_keys.contains(&"error"));
    assert!(tag_keys.contains(&"error.message"));
}

#[test]
fn test_jaeger_json_incomplete_tags() {
    let root = TraceNode::new("incomplete_parser", "input");
    let trace = ParseTrace {
        input: "input".to_string(),
        root,
        baseline_instant: Some(std::time::Instant::now()),
        baseline_unix_micros: 1000000,
    };

    let json = trace.to_jaeger_json();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let tags = parsed["data"][0]["spans"][0]["tags"].as_array().unwrap();
    let status_tag = tags.iter().find(|t| t["key"] == "status").unwrap();
    assert_eq!(status_tag["value"], "incomplete");
}

#[test]
fn test_jaeger_json_references() {
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

    let child_span = spans
        .iter()
        .find(|s| s["operationName"] == "child")
        .unwrap();
    let references = child_span["references"].as_array().unwrap();

    assert_eq!(references.len(), 1);
    assert_eq!(references[0]["refType"], "CHILD_OF");
}

// ============================================================================
// TraceCollector Default Test
// ============================================================================

#[test]
fn test_trace_collector_default() {
    if is_tracing_enabled() {
        disable_tracing("cleanup");
    }

    enable_tracing();
    assert!(is_tracing_enabled());

    trace_enter("test", "input");
    trace_exit_success(5, "done");

    let trace = disable_tracing("input");
    assert_eq!(trace.root.name, "test");
}

// ============================================================================
// Edge Case Tests - Parser Robustness
// ============================================================================

/// Test that parse_with_trace handles various edge cases gracefully.
/// Note: The parser is designed to be very permissive - most inputs parse
/// successfully (possibly with empty name). These tests document this behavior.
#[rstest]
#[case::empty("", "")]
#[case::whitespace("   ", "")]
#[case::newline_only("\n", "")]
#[case::numbers_only("12345", "")] // Numbers parsed as measurement, name is empty
#[case::special_chars("@#$%", "")]
fn test_parse_with_trace_edge_cases(
    parser: IngredientParser,
    #[case] input: &str,
    #[case] expected_name: &str,
) {
    let result = parser.parse_with_trace(input);
    // The parser is permissive - it should succeed even with unusual input
    assert!(
        result.result.is_ok(),
        "Unexpected parse failure for: {input:?}"
    );
    assert_eq!(result.result.unwrap().name, expected_name);
}

/// Test that unit mismatch in ranges is handled gracefully with tracing enabled.
/// This exercises the trace formatting code path for range unit mismatch.
#[rstest]
fn test_parse_with_trace_range_unit_mismatch(parser: IngredientParser) {
    // "1g-2tbsp" has mismatched units which should be detected
    let result = parser.parse_with_trace("1g-2tbsp flour");
    assert!(result.result.is_ok());
    // The trace should have captured timing info
    assert!(result.trace.baseline_instant.is_some());
    // The trace should have the input
    assert_eq!(result.trace.input, "1g-2tbsp flour");
}
