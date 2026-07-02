//! Tests for parse tracing functionality

#![allow(clippy::unwrap_used)]

use ingredient::IngredientParser;
use ingredient::trace::{GrammarOutcome, ParseTrace, TraceNode, TraceOutcome};
use rstest::{fixture, rstest};

// ============================================================================
// Fixtures
// ============================================================================

#[fixture]
fn parser() -> IngredientParser {
    IngredientParser::new()
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

// NOTE: the thread-local tracing hooks (enable_tracing/trace_enter/…) are
// crate-internal (`pub(crate)`); their unit tests live alongside them in
// `src/trace/collector.rs`. The public entry point is `parse_with_trace`.

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

// NOTE: the parse_with_trace happy path (result == from_str + non-empty tree) is
// smoke-tested across the WHOLE corpus by `accuracy.rs::trace_path_matches_from_str`,
// and from_str accuracy (adjective extraction etc.) belongs in
// tests/corpus/corpus.jsonl. Tests below cover trace-specific behavior the
// corpus can't express: custom-parser config through the traced path and
// permissive edge cases.

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

/// Each outcome emits its own tag set on the Jaeger span. `incomplete` is the
/// default (no `success`/`failure` call); its status value is asserted too.
#[rstest]
#[case::success("success", &["status", "consumed", "output"])]
#[case::failure("failure", &["status", "error", "error.message"])]
#[case::incomplete("incomplete", &["status"])]
fn test_jaeger_json_tags(#[case] outcome: &str, #[case] expected_keys: &[&str]) {
    let mut root = TraceNode::new("parser", "input");
    match outcome {
        "success" => root.success(5, "output"),
        "failure" => root.failure("parse error"),
        _ => {} // incomplete: leave the node untouched
    }

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

    for key in expected_keys {
        assert!(tag_keys.contains(key), "Expected tag '{key}' for {outcome}");
    }

    if outcome == "incomplete" {
        let status_tag = tags.iter().find(|t| t["key"] == "status").unwrap();
        assert_eq!(status_tag["value"], "incomplete");
    }
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
// Edge Case Tests - Parser Robustness
// ============================================================================

/// Test that parse_with_trace handles various edge cases gracefully.
/// Note: The parser is permissive. A bare quantity may have an empty name
/// ("12345"), but when leftover text would otherwise be orphaned in the
/// modifier, the parse falls back to the input as the name ("@#$%").
#[rstest]
#[case::empty("", "")]
#[case::whitespace("   ", "")]
#[case::newline_only("\n", "")]
#[case::numbers_only("12345", "")] // Bare quantity: amount parsed, name legitimately empty
#[case::special_chars("@#$%", "@#$%")] // Junk would orphan in the modifier -> fall back to input
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

// ============================================================================
// StageReport Tests (ParseTrace::stages)
// ============================================================================

/// Plain line: no normalize rewrites, all recognizers fail, grammar parses,
/// no refine passes — the trivial path through every bucket.
#[rstest]
fn test_stages_plain_line(parser: IngredientParser) {
    let report = parser.parse_with_trace("2 cups flour").trace.stages();
    assert!(report.normalize.is_empty());
    assert_eq!(report.recognizers.len(), 3);
    assert!(!report.recognizer_matched());
    assert_eq!(
        report.grammar,
        Some(GrammarOutcome::Parsed("flour".to_string()))
    );
    assert!(report.refine.is_empty());
    assert_eq!(report.result_preview.as_deref(), Some("flour"));
}

/// Normalize rewrite: the leading determiner strip lands in the normalize
/// bucket with its before → after texts.
#[rstest]
fn test_stages_normalize_rewrite(parser: IngredientParser) {
    let report = parser.parse_with_trace("the 1 cup flour").trace.stages();
    assert_eq!(report.normalize.len(), 1);
    let rewrite = &report.normalize[0];
    assert_eq!(rewrite.name, "strip_leading_determiner");
    assert_eq!(rewrite.before, "the 1 cup flour");
    assert_eq!(rewrite.after, "1 cup flour");
    assert_eq!(
        report.grammar,
        Some(GrammarOutcome::Parsed("flour".to_string()))
    );
}

/// Special-form recognizer match: x_of_construction matches and the grammar
/// outcome comes from the nested re-parse of the rewritten line.
#[rstest]
fn test_stages_recognizer_match(parser: IngredientParser) {
    let report = parser.parse_with_trace("Juice of 1 lemon").trace.stages();
    assert!(report.recognizer_matched());
    let matched = report
        .recognizers
        .iter()
        .find(|r| r.output.is_some())
        .unwrap();
    assert_eq!(matched.name, "x_of_construction");
    assert_eq!(matched.output.as_deref(), Some("lemon"));
    assert_eq!(
        report.grammar,
        Some(GrammarOutcome::Parsed("lemon".to_string()))
    );
    assert_eq!(report.result_preview.as_deref(), Some("lemon"));
}

/// Refine pass: the merged alternatives extraction (here the no-quantity
/// word-alternative split) shows up in the refine bucket.
#[rstest]
fn test_stages_refine_pass(parser: IngredientParser) {
    let report = parser.parse_with_trace("red or white onion").trace.stages();
    assert_eq!(report.refine.len(), 1);
    assert_eq!(report.refine[0].name, "extract_alternatives_from_name");
    assert_eq!(report.result_preview.as_deref(), Some("red onion"));
}

/// The segment bucket carries the clause decisions and the assembly repairs
/// that fired, in emit order (nested inside the grammar span on the segmented
/// default path).
#[rstest]
fn test_stages_segment_bucket(parser: IngredientParser) {
    let report = parser
        .parse_with_trace("1 cup flour, sifted, divided")
        .trace
        .stages();
    let names: Vec<&str> = report.segment.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        vec!["head_candidate", "prep_chain", "prep_chain"],
        "clause decisions in source order"
    );

    // An assembly repair (the minus-clause split) also lands in the bucket.
    let report = parser
        .parse_with_trace("½ cup minus 1 tablespoon flour")
        .trace
        .stages();
    assert!(
        report
            .segment
            .iter()
            .any(|s| s.name == "fix_leading_minus_clause"),
        "assembly repair missing from segment bucket: {:?}",
        report.segment
    );
}

/// Synthetic trees pin the bucketing variants real parses can't reach:
/// a failed grammar node → FellBack, a recognizer-only success → Skipped,
/// and a trace with no core nodes at all → grammar None / everything in
/// normalize / no result preview.
#[test]
fn test_stages_synthetic_variants() {
    fn trace_with_children(children: Vec<TraceNode>) -> ParseTrace {
        let mut root = TraceNode::new("parse_line", "input");
        for c in children {
            root.add_child(c);
        }
        ParseTrace {
            input: "input".to_string(),
            root,
            baseline_instant: None,
            baseline_unix_micros: 0,
        }
    }

    // Grammar node failed → FellBack.
    let mut grammar = TraceNode::new("parse_ingredient", "input");
    grammar.failure("no parse");
    let report = trace_with_children(vec![grammar]).stages();
    assert_eq!(report.grammar, Some(GrammarOutcome::FellBack));
    assert_eq!(report.result_preview, None);

    // Recognizer succeeded with no nested grammar → Skipped.
    let mut recognizer = TraceNode::new("x_of_construction", "input");
    recognizer.success(5, "lemon");
    let report = trace_with_children(vec![recognizer]).stages();
    assert_eq!(report.grammar, Some(GrammarOutcome::Skipped));
    assert!(report.recognizer_matched());

    // No core nodes at all → grammar None, children bucketed as normalize.
    let mut rewrite = TraceNode::new("some_rewrite", "input");
    rewrite.success(0, "rewritten");
    let report = trace_with_children(vec![rewrite]).stages();
    assert_eq!(report.grammar, None);
    assert!(report.recognizers.is_empty());
    assert_eq!(report.normalize.len(), 1);
    assert_eq!(report.normalize[0].after, "rewritten");
}

/// Regression guard: `format_stages` (the `--explain` renderer, now backed by
/// `stages()`) still emits the stage labels the CLI output is known by.
#[rstest]
fn test_format_stages_labels(parser: IngredientParser) {
    let out = parser
        .parse_with_trace("2 cups flour")
        .trace
        .format_stages(false);
    for label in [
        "input:",
        "normalize:",
        "recognize:",
        "grammar:",
        "refine:",
        "result:",
    ] {
        assert!(out.contains(label), "missing label {label} in:\n{out}");
    }
    assert!(out.contains("(no rewrites fired)"));
    assert!(out.contains("name=\"flour\""));
}

/// Unit mismatch in ranges is handled gracefully with tracing enabled, and
/// the formatted tree actually carries the range parser's mismatch branch.
/// (The previous version asserted only unconditional invariants of
/// parse_with_trace — it could not fail.)
#[rstest]
fn test_parse_with_trace_range_unit_mismatch(parser: IngredientParser) {
    let result = parser.parse_with_trace("1g-2tbsp flour");
    let tree = result.trace.format_tree(false);
    assert!(
        tree.contains("range_with_units") || tree.contains("cross_unit_range"),
        "trace tree should show the range parser attempt:\n{tree}"
    );
}
