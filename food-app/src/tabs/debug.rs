use crate::theme;
use eframe::egui::{self, RichText};
use egui_ltreeview::TreeView;
use ingredient::trace::{ParseTrace, TraceNode, TraceOutcome};
use ingredient::util::truncate_str;

/// Context for the trace tree to generate unique IDs
#[derive(Clone, Copy)]
pub enum TraceTreeContext {
    Test,
}

pub fn show_debug_tab(ui: &mut egui::Ui, traces: &[ParseTrace], selected: &mut Option<usize>) {
    let nav_changed = super::arrow_nav(ui, selected, traces.len());
    egui::Panel::left("recipe_ingredient_list")
        .resizable(true)
        .default_size(260.0)
        .show(ui, |ui| {
            ui.strong("Ingredients");
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("ingredient_list")
                .show(ui, |ui| {
                    for (idx, trace) in traces.iter().enumerate() {
                        let response = ui.selectable_label(*selected == Some(idx), &trace.input);
                        if response.clicked() {
                            *selected = Some(idx);
                        }
                        if nav_changed && *selected == Some(idx) {
                            response.scroll_to_me(Some(egui::Align::Center));
                        }
                    }
                });
        });
    if let Some(trace) = selected.and_then(|idx| traces.get(idx)) {
        super::inspector::show_trace_inspector(ui, trace);
    } else {
        ui.label("Select an ingredient to inspect its fields and parser execution.");
    }
}

pub fn show_trace_tree(ui: &mut egui::Ui, trace: &ParseTrace, context: TraceTreeContext) {
    let id_salt = match context {
        TraceTreeContext::Test => "test_parse_trace_tree",
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
        // Success path
        TraceOutcome::Success { .. } => RichText::new(text)
            .color(theme::palette().trace_ok())
            .strong(),
        // Failed branches
        TraceOutcome::Failure { .. } => RichText::new(text).color(theme::palette().trace_fail()),
        TraceOutcome::Incomplete => RichText::new(text).color(theme::palette().trace_incomplete()),
    }
}
