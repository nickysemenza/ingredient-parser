use eframe::egui;
use ingredient::ingredient::Ingredient;
use ingredient::trace::ParseTrace;

use super::debug::{show_trace_tree, TraceTreeContext};

pub fn show_test_tab(
    ui: &mut egui::Ui,
    test_input: &mut String,
    test_trace: &mut Option<ParseTrace>,
    test_result: &mut Option<Ingredient>,
) {
    ui.heading("Test Ingredient Parser");
    ui.separator();

    // Input section
    ui.horizontal(|ui| {
        ui.label("Ingredient:");
        let response = ui.add(
            egui::TextEdit::singleline(test_input)
                .desired_width(400.0)
                .hint_text("e.g., 2 cups flour, sifted"),
        );

        if ui.button("Parse").clicked()
            || response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))
        {
            let parser = ingredient::IngredientParser::new();
            let result = parser.parse_with_trace(test_input);
            *test_trace = Some(result.trace);
            *test_result = result.result.ok();
        }

        // Export to Jaeger button (only enabled when trace exists)
        ui.add_enabled_ui(test_trace.is_some(), |ui| {
            if ui.button("📤 Copy Jaeger JSON").clicked() {
                if let Some(trace) = test_trace {
                    let jaeger_json = trace.to_jaeger_json();
                    ui.ctx().copy_text(jaeger_json);
                }
            }
        });
    });

    ui.separator();

    // Results section - 1/3 left, 2/3 right layout
    let available_width = ui.available_width();
    let available_height = ui.available_height();
    let left_width = available_width / 3.0;
    let right_width = available_width * 2.0 / 3.0 - 10.0; // -10 for spacing

    ui.horizontal_top(|ui| {
        ui.set_min_height(available_height);
        // Left column: Parsed result (1/3 width)
        ui.allocate_ui_with_layout(
            egui::vec2(left_width, available_height),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| {
                show_parsed_result(ui, test_result);
            },
        );

        ui.separator();

        // Right column: Parse trace tree (2/3 width)
        ui.allocate_ui_with_layout(
            egui::vec2(right_width, available_height),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| {
                ui.heading("Parse Trace");
                ui.separator();
                if let Some(trace) = test_trace {
                    egui::ScrollArea::vertical()
                        .id_salt("test_trace_tree")
                        .show(ui, |ui| {
                            show_trace_tree(ui, trace, TraceTreeContext::Test);
                        });
                } else {
                    ui.label("Parse trace will appear here");
                }
            },
        );
    });
}

fn show_parsed_result(ui: &mut egui::Ui, test_result: &Option<Ingredient>) {
    ui.heading("Parsed Result");
    ui.separator();
    if let Some(result) = test_result {
        ui.label(format!("Name: {}", result.name));
        ui.label(format!(
            "Amounts: {}",
            result
                .amounts
                .iter()
                .map(|a| a.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ));
        ui.label(format!(
            "Modifier: {}",
            result.modifier.as_deref().unwrap_or("(none)")
        ));
        ui.separator();
        ui.label("JSON:");
        egui::ScrollArea::vertical()
            .id_salt("result_json")
            .max_height(200.0)
            .show(ui, |ui| {
                let json = serde_json::to_string_pretty(result).unwrap_or_default();
                ui.add(egui::TextEdit::multiline(&mut json.as_str()).code_editor());
            });
    } else {
        ui.label("Enter an ingredient and click Parse");
    }
}
