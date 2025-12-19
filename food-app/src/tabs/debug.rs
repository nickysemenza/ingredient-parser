use eframe::egui::{self, RichText};
use eframe::epaint::Color32;
use egui_ltreeview::TreeView;
use ingredient::trace::{ParseTrace, TraceNode, TraceOutcome};
use ingredient::util::truncate_str;

/// Context for the trace tree to generate unique IDs
#[derive(Clone, Copy)]
pub enum TraceTreeContext {
    Test,
    Debug,
}

pub fn show_debug_tab(ui: &mut egui::Ui, traces: &[ParseTrace], selected: &mut Option<usize>) {
    // Use columns for better layout
    ui.columns(2, |columns| {
        // Left column: ingredient selector
        columns[0].heading("Ingredients");
        columns[0].separator();
        egui::ScrollArea::vertical()
            .id_salt("ingredient_list")
            .show(&mut columns[0], |ui| {
                for (idx, trace) in traces.iter().enumerate() {
                    let is_selected = *selected == Some(idx);
                    if ui
                        .selectable_label(is_selected, truncate_str(&trace.input, 50))
                        .clicked()
                    {
                        *selected = Some(idx);
                    }
                }
            });

        // Right column: tree view for selected ingredient
        columns[1].heading("Parse Trace");
        columns[1].separator();
        if let Some(idx) = selected {
            if let Some(trace) = traces.get(*idx) {
                columns[1].label(format!("Input: \"{}\"", trace.input));
                columns[1].separator();
                egui::ScrollArea::vertical()
                    .id_salt("trace_tree")
                    .show(&mut columns[1], |ui| {
                        show_trace_tree(ui, trace, TraceTreeContext::Debug);
                    });
            }
        } else {
            columns[1].label("Select an ingredient to view its parse trace");
        }
    });
}

pub fn show_trace_tree(ui: &mut egui::Ui, trace: &ParseTrace, context: TraceTreeContext) {
    let id_salt = match context {
        TraceTreeContext::Test => "test_parse_trace_tree",
        TraceTreeContext::Debug => "debug_parse_trace_tree",
    };
    let id = ui.make_persistent_id(id_salt);
    TreeView::new(id).show(ui, |builder| {
        render_trace_node(builder, &trace.root, 0);
    });
}

fn render_trace_node(
    builder: &mut egui_ltreeview::TreeViewBuilder<usize>,
    node: &TraceNode,
    id: usize,
) -> usize {
    let label = format_node_label_rich(node);

    if node.children.is_empty() {
        builder.leaf(id, label);
        id + 1
    } else {
        builder.dir(id, label);
        let mut next_id = id + 1;
        for child in &node.children {
            next_id = render_trace_node(builder, child, next_id);
        }
        builder.close_dir();
        next_id
    }
}

fn format_node_label_rich(node: &TraceNode) -> RichText {
    let text = format!(
        "{} \"{}\"{}",
        node.name,
        truncate_str(&node.input, 25),
        match &node.outcome {
            TraceOutcome::Success { output_preview, .. } =>
                format!(" -> {}", truncate_str(output_preview, 20)),
            TraceOutcome::Incomplete => " ...".to_string(),
            _ => String::new(),
        }
    );

    match &node.outcome {
        TraceOutcome::Success { .. } => {
            // Bright green for success path
            RichText::new(text)
                .color(Color32::from_rgb(100, 200, 100))
                .strong()
        }
        TraceOutcome::Failure { .. } => {
            // Muted red for failed branches
            RichText::new(text).color(Color32::from_rgb(180, 90, 90))
        }
        TraceOutcome::Incomplete => RichText::new(text).color(Color32::from_rgb(180, 180, 100)),
    }
}
