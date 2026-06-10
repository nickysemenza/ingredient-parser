//! Tree formatting for parse traces

use std::fmt::Write as _;

use super::stages::{GrammarOutcome, StageReport};
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
//
// The bucketing itself lives in `stages.rs` (`StageReport`); this is just the
// text renderer.
// ---------------------------------------------------------------------------

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

/// Render a stage report as a compact, stage-level text view for `--explain`.
pub(super) fn format_stages(report: &StageReport, colored: bool) -> String {
    let mut out = String::new();
    let mut line = |label: &str, body: &str| {
        let _ = writeln!(out, "{}{body}", stage_label(label, colored));
    };

    line("input:", &format!("\"{}\"", report.input));

    if report.normalize.is_empty() {
        line("normalize:", "(no rewrites fired)");
    } else {
        for (idx, n) in report.normalize.iter().enumerate() {
            let label = if idx == 0 { "normalize:" } else { "" };
            line(
                label,
                &format!("{}  \"{}\" → \"{}\"", n.name, n.before, n.after),
            );
        }
    }

    if let Some(grammar) = &report.grammar {
        if !report.recognizers.is_empty() {
            let parts: Vec<String> = report
                .recognizers
                .iter()
                .map(|r| match &r.output {
                    Some(preview) => format!("{} {} → {preview}", r.name, ok_mark(colored)),
                    None => format!("{} {}", r.name, fail_mark(colored)),
                })
                .collect();
            let suffix = if report.recognizer_matched() {
                ""
            } else {
                "  → core parse"
            };
            line("recognize:", &format!("{}{suffix}", parts.join("  ")));
        }
        let grammar_body = match grammar {
            GrammarOutcome::Parsed(p) => format!("name=\"{p}\""),
            GrammarOutcome::FellBack => "(no parse — fell back)".to_string(),
            GrammarOutcome::Skipped => "(skipped — recognizer produced the result)".to_string(),
        };
        line("grammar:", &grammar_body);
    }

    if report.refine.is_empty() {
        line("refine:", "(no passes changed it)");
    } else {
        for (idx, n) in report.refine.iter().enumerate() {
            let label = if idx == 0 { "refine:" } else { "" };
            line(
                label,
                &format!("{}  \"{}\" → {}", n.name, n.before, n.after),
            );
        }
    }

    let result_preview = report
        .result_preview
        .as_deref()
        .unwrap_or("(name-only fallback)");
    line("result:", &format!("name=\"{result_preview}\""));

    out
}
