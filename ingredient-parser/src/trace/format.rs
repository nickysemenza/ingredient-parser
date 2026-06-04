//! Tree formatting for parse traces

use std::fmt::Write as _;

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

// ---------------------------------------------------------------------------
// Compact stage view (`--explain`)
//
// The full tree is great for debugging the grammar but drowns the pipeline
// story in `alt()` backtracking. `format_stages` renders one line per pipeline
// stage — normalize → recognize → grammar → refine → result — and collapses the
// grammar's combinator subtree to a single summary line, so a corpus-fixer can
// see at a glance *which stage* mishandled a line (and therefore where a fix
// belongs). See the routing guide in `parser/mod.rs`.
// ---------------------------------------------------------------------------

/// Recognizer span names, mirroring `parser::recognize::RECOGNIZERS`. Used only
/// to bucket the trace's direct children into stages for this debug view; a
/// stale entry would only mis-label a stage in `--explain`, never affect parsing.
const RECOGNIZER_NAMES: &[&str] = &["optional_wrapped", "trailing_amount", "x_of_construction"];
/// The grammar span name (the `traced_parser!` wrapping `parse_ingredient`).
const GRAMMAR_NAME: &str = "parse_ingredient";

fn is_core_node(name: &str) -> bool {
    name == GRAMMAR_NAME || RECOGNIZER_NAMES.contains(&name)
}

fn success_preview(node: &TraceNode) -> Option<&str> {
    match &node.outcome {
        TraceOutcome::Success { output_preview, .. } => Some(output_preview),
        _ => None,
    }
}

/// Find the grammar node among the core children, whether it's a direct child
/// (no recognizer matched) or nested under a successful recognizer (e.g.
/// `x_of_construction` re-parses its rewritten line through the grammar).
fn find_grammar(core: &[TraceNode]) -> Option<&TraceNode> {
    for c in core {
        if c.name == GRAMMAR_NAME {
            return Some(c);
        }
        if RECOGNIZER_NAMES.contains(&c.name.as_str()) {
            if let Some(g) = c.children.iter().find(|g| g.name == GRAMMAR_NAME) {
                return Some(g);
            }
        }
    }
    None
}

fn stage_label(label: &str, colored: bool) -> String {
    let padded = format!("{label:<11}");
    if colored {
        format!("\x1b[1m{padded}\x1b[0m")
    } else {
        padded
    }
}

fn ok_mark(colored: bool) -> &'static str {
    if colored {
        "\x1b[32m✓\x1b[0m"
    } else {
        "✓"
    }
}

fn fail_mark(colored: bool) -> &'static str {
    if colored {
        "\x1b[31m✗\x1b[0m"
    } else {
        "✗"
    }
}

/// Render a parse trace as a compact, stage-level report for `--explain`.
pub(super) fn format_stages(root: &TraceNode, colored: bool) -> String {
    let mut out = String::new();
    let mut line = |label: &str, body: &str| {
        let _ = writeln!(out, "{}{body}", stage_label(label, colored));
    };

    line("input:", &format!("\"{}\"", root.input));

    let children = &root.children;
    let first_core = children.iter().position(|c| is_core_node(&c.name));
    let last_core = children.iter().rposition(|c| is_core_node(&c.name));

    // normalize — every node before the first core (recognizer/grammar) node.
    let normalize_nodes = match first_core {
        Some(i) => &children[..i],
        None => &children[..],
    };
    if normalize_nodes.is_empty() {
        line("normalize:", "(no rewrites fired)");
    } else {
        for (idx, n) in normalize_nodes.iter().enumerate() {
            let label = if idx == 0 { "normalize:" } else { "" };
            let after = success_preview(n).unwrap_or("");
            line(label, &format!("{}  \"{}\" → \"{after}\"", n.name, n.input));
        }
    }

    // recognize + grammar — the core block.
    if let (Some(i), Some(j)) = (first_core, last_core) {
        let core = &children[i..=j];
        let mut parts = Vec::new();
        let mut matched = false;
        for c in core {
            if RECOGNIZER_NAMES.contains(&c.name.as_str()) {
                if let Some(preview) = success_preview(c) {
                    parts.push(format!("{} {} → {preview}", c.name, ok_mark(colored)));
                    matched = true;
                } else {
                    parts.push(format!("{} {}", c.name, fail_mark(colored)));
                }
            }
        }
        if !parts.is_empty() {
            let suffix = if matched { "" } else { "  → core parse" };
            line("recognize:", &format!("{}{suffix}", parts.join("  ")));
        }
        let grammar_body = match find_grammar(core) {
            Some(g) => match success_preview(g) {
                Some(p) => format!("name=\"{p}\""),
                None => "(no parse — fell back)".to_string(),
            },
            None => "(skipped — recognizer produced the result)".to_string(),
        };
        line("grammar:", &grammar_body);
    }

    // refine — every node after the last core node.
    let refine_nodes = match last_core {
        Some(j) => &children[j + 1..],
        None => &[][..],
    };
    if refine_nodes.is_empty() {
        line("refine:", "(no passes changed it)");
    } else {
        for (idx, n) in refine_nodes.iter().enumerate() {
            let label = if idx == 0 { "refine:" } else { "" };
            let after = success_preview(n).unwrap_or("");
            line(label, &format!("{}  \"{}\" → {after}", n.name, n.input));
        }
    }

    let result_preview = success_preview(root).unwrap_or("(name-only fallback)");
    line("result:", &format!("name=\"{result_preview}\""));

    out
}
