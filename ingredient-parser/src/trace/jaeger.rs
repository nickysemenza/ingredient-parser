//! Jaeger JSON export for parse traces

use super::{ParseTrace, TraceNode, TraceOutcome};

/// Export trace to Jaeger-compatible JSON format
pub(super) fn to_jaeger_json(trace: &ParseTrace) -> String {
    use rand::Rng;

    // Generate trace ID (16 bytes as hex = 32 chars)
    let mut rng = rand::rng();
    let trace_id: String = (0..32)
        .map(|_| format!("{:x}", rng.random_range(0..16u8)))
        .collect();

    // Collect spans from tree
    let mut spans = Vec::new();
    let mut span_counter = 0u64;
    collect_spans(
        trace,
        &trace.root,
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
    trace: &ParseTrace,
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
    let start_time =
        if let (Some(baseline), Some(node_start)) = (trace.baseline_instant, node.start_time) {
            let offset = node_start.duration_since(baseline).as_micros() as u64;
            trace.baseline_unix_micros + offset
        } else {
            trace.baseline_unix_micros + *span_counter
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
    let mut tags = vec![serde_json::json!({"key": "input", "type": "string", "value": node.input})];

    match &node.outcome {
        TraceOutcome::Success {
            consumed,
            output_preview,
        } => {
            tags.push(serde_json::json!({"key": "status", "type": "string", "value": "success"}));
            tags.push(
                serde_json::json!({"key": "consumed", "type": "int64", "value": *consumed as i64}),
            );
            tags.push(
                serde_json::json!({"key": "output", "type": "string", "value": output_preview}),
            );
        }
        TraceOutcome::Failure { error } => {
            tags.push(serde_json::json!({"key": "status", "type": "string", "value": "failure"}));
            tags.push(serde_json::json!({"key": "error", "type": "bool", "value": true}));
            tags.push(
                serde_json::json!({"key": "error.message", "type": "string", "value": error}),
            );
        }
        TraceOutcome::Incomplete => {
            tags.push(
                serde_json::json!({"key": "status", "type": "string", "value": "incomplete"}),
            );
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
        collect_spans(trace, child, trace_id, Some(&span_id), spans, span_counter);
    }
}
