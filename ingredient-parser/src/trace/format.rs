//! Tree formatting for parse traces

use super::{TraceNode, TraceOutcome};

/// Format a trace node as a tree string for display
pub(super) fn format_tree(node: &TraceNode, colored: bool) -> String {
    let mut output = String::new();
    format_node(node, &mut output, "", true, colored);
    output
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

    // Write this node
    if colored {
        output.push_str(&format!(
            "{}{}\x1b[1m{}\x1b[0m \"{}\" {}\n",
            prefix, connector, node.name, node.input, outcome_symbol
        ));
    } else {
        output.push_str(&format!(
            "{}{}{} \"{}\" {}\n",
            prefix, connector, node.name, node.input, outcome_symbol
        ));
    }

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
