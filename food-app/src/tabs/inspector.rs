//! Ingredient inspection shared by pasted input, recipe sources, and cookbook review.
use super::debug::{TraceTreeContext, show_trace_tree};
use crate::theme;
use eframe::egui::{self, RichText};
use ingredient::ingredient::Ingredient;
use ingredient::trace::{GrammarOutcome, ParseTrace, StageReport};

pub struct Inspection<'a> {
    pub input: &'a str,
    pub ingredient: Option<&'a Ingredient>,
    pub stages: &'a StageReport,
    pub trace: &'a ParseTrace,
}

#[derive(Clone, Copy, Default, PartialEq)]
enum View {
    #[default]
    Fields,
    Stages,
    Tree,
    Json,
}

#[derive(Default)]
pub struct IngredientInspector {
    view: View,
}
impl IngredientInspector {
    pub fn show(&mut self, ui: &mut egui::Ui, data: Inspection<'_>) {
        self.show_inner(ui, data, false);
    }

    fn show_inner(&mut self, ui: &mut egui::Ui, data: Inspection<'_>, closable: bool) -> bool {
        let mut close = false;
        ui.horizontal(|ui| {
            ui.label(RichText::new("Ingredient").size(17.0).strong());
            if closable {
                close = ui
                    .small_button(theme::icon::CLOSE)
                    .on_hover_text("Close inspector")
                    .clicked();
            }
            ui.menu_button("Actions", |ui| {
                if ui.button("Copy Jaeger JSON").clicked() {
                    ui.ctx().copy_text(data.trace.to_jaeger_json());
                    ui.close();
                }
                if ui.button("Copy input").clicked() {
                    ui.ctx().copy_text(data.input.to_owned());
                    ui.close();
                }
            });
        });
        ui.add_space(8.0);
        ui.add(egui::Label::new(RichText::new(data.input).monospace()).wrap());
        ui.add_space(12.0);
        ui.horizontal_wrapped(|ui| {
            for (view, label) in [
                (View::Fields, "Fields"),
                (View::Stages, "Stages"),
                (View::Tree, "Trace tree"),
                (View::Json, "JSON"),
            ] {
                ui.selectable_value(&mut self.view, view, label);
            }
        });
        ui.separator();
        egui::ScrollArea::both()
            .id_salt("ingredient_inspector_scroll")
            .show(ui, |ui| match self.view {
                View::Fields => {
                    if let Some(i) = data.ingredient {
                        egui::Grid::new("ingredient_fields")
                            .num_columns(2)
                            .spacing([18.0, 6.0])
                            .show(ui, |ui| {
                                ui.weak("Name");
                                ui.label(&i.name);
                                ui.end_row();
                                ui.weak("Amounts");
                                ui.label(
                                    i.amounts
                                        .iter()
                                        .map(ToString::to_string)
                                        .collect::<Vec<_>>()
                                        .join(" / "),
                                );
                                ui.end_row();
                                ui.weak("Modifier");
                                ui.label(i.modifier.as_deref().unwrap_or("—"));
                                ui.end_row();
                                ui.weak("Optional");
                                ui.label(if i.optional { "Yes" } else { "No" });
                                ui.end_row();
                                ui.weak("Usage");
                                ui.label(format!("{:?}", i.usage));
                                ui.end_row();
                                ui.weak("Confidence");
                                ui.label(format!("{:?}", i.parse_notes.confidence));
                                ui.end_row();
                            });
                        for reason in i.parse_notes.review_reasons() {
                            ui.colored_label(
                                theme::palette().trace_incomplete(),
                                reason.to_string(),
                            );
                        }
                    } else {
                        ui.label("No ingredient result.");
                    }
                }
                View::Stages => show_stages(ui, data.stages),
                View::Tree => show_trace_tree(ui, data.trace, TraceTreeContext::Test),
                View::Json => show_json(ui, data.ingredient),
            });
        close
    }
}

/// Render a [`StageReport`] as one card per pipeline stage, mirroring the
/// CLI's `--explain` view: normalize → recognize → grammar → segment → refine
/// → result.
fn show_stages(ui: &mut egui::Ui, report: &StageReport) {
    stage_card(ui, "input", |ui| {
        ui.monospace(format!("\"{}\"", report.input));
    });

    stage_card(ui, "normalize", |ui| {
        if report.normalize.is_empty() {
            ui.label(RichText::new("(no rewrites fired)").weak());
        } else {
            for r in &report.normalize {
                ui.monospace(format!("{}  \"{}\" → \"{}\"", r.name, r.before, r.after));
            }
        }
    });

    if !report.recognizers.is_empty() {
        stage_card(ui, "recognize", |ui| {
            for r in &report.recognizers {
                match &r.output {
                    Some(out) => {
                        ui.label(
                            RichText::new(format!("{} ✓ → {out}", r.name))
                                .color(theme::palette().trace_ok()),
                        );
                    }
                    None => {
                        ui.label(
                            RichText::new(format!("{} ✗", r.name))
                                .color(theme::palette().trace_fail()),
                        );
                    }
                }
            }
        });
    }

    if let Some(grammar) = &report.grammar {
        stage_card(ui, "grammar", |ui| match grammar {
            GrammarOutcome::Parsed(name) => {
                ui.label(
                    RichText::new(format!("name=\"{name}\"")).color(theme::palette().trace_ok()),
                );
            }
            GrammarOutcome::FellBack => {
                ui.label(
                    RichText::new("(no parse — fell back)").color(theme::palette().trace_fail()),
                );
            }
            GrammarOutcome::Skipped => {
                ui.label(RichText::new("(skipped — recognizer produced the result)").weak());
            }
        });
    }

    if !report.segment.is_empty() {
        stage_card(ui, "segment", |ui| {
            for r in &report.segment {
                ui.monospace(format!("{}  \"{}\" → {}", r.name, r.before, r.after));
            }
        });
    }

    stage_card(ui, "refine", |ui| {
        if report.refine.is_empty() {
            ui.label(RichText::new("(no passes changed it)").weak());
        } else {
            for r in &report.refine {
                ui.monospace(format!("{}  \"{}\" → {}", r.name, r.before, r.after));
            }
        }
    });

    stage_card(ui, "result", |ui| match &report.result_preview {
        Some(name) => {
            ui.label(RichText::new(format!("name=\"{name}\"")).color(theme::palette().trace_ok()));
        }
        None => {
            ui.label(RichText::new("(name-only fallback)").color(theme::palette().trace_fail()));
        }
    });
}

/// One pipeline stage as a labeled card row.
fn stage_card(ui: &mut egui::Ui, label: &str, add_contents: impl FnOnce(&mut egui::Ui)) {
    ui.horizontal_top(|ui| {
        // Fixed-width label gutter so the stage cards align.
        ui.allocate_ui(egui::vec2(72.0, 0.0), |ui| {
            ui.label(RichText::new(label).strong().monospace());
        });
        ui.vertical(add_contents);
    });
}

/// The selected row's full ingredient JSON (read-only, selectable).
fn show_json(ui: &mut egui::Ui, ingredient: Option<&Ingredient>) {
    match ingredient {
        Some(i) => {
            let json = serde_json::to_string_pretty(i).unwrap_or_default();
            ui.add(egui::TextEdit::multiline(&mut json.as_str()).code_editor());
        }
        None => {
            ui.label(RichText::new("(no parsed ingredient)").weak());
        }
    }
}

/// Cached inspection for source viewers that only hold raw ingredient lines.
/// The cache belongs to the containing UI, so selection changes parse once.
pub fn show_line_inspector(ui: &mut egui::Ui, input: &str) -> bool {
    show_source_inspector(ui, input, None, true)
}

pub fn show_trace_inspector(ui: &mut egui::Ui, trace: &ParseTrace) {
    show_source_inspector(ui, &trace.input, Some(trace), false);
}

fn show_source_inspector(
    ui: &mut egui::Ui,
    input: &str,
    trace: Option<&ParseTrace>,
    closable: bool,
) -> bool {
    let id = ui.make_persistent_id("source_ingredient_inspection");
    let cached = ui.data_mut(|data| data.get_temp::<std::sync::Arc<CachedInspection>>(id));
    let cached = cached_inspection(cached, input);
    let view_id = id.with("view");
    let mut inspector = IngredientInspector {
        view: ui
            .data_mut(|data| data.get_temp::<View>(view_id))
            .unwrap_or_default(),
    };
    let close = inspector.show_inner(
        ui,
        Inspection {
            input,
            ingredient: Some(&cached.ingredient),
            stages: &cached.stages,
            trace: trace.unwrap_or(&cached.trace),
        },
        closable,
    );
    ui.data_mut(|data| data.insert_temp(view_id, inspector.view));
    ui.data_mut(|data| data.insert_temp(id, cached));
    close
}

#[derive(Clone)]
struct CachedInspection {
    input: String,
    ingredient: Ingredient,
    stages: StageReport,
    trace: ParseTrace,
}

fn cached_inspection(
    cached: Option<std::sync::Arc<CachedInspection>>,
    input: &str,
) -> std::sync::Arc<CachedInspection> {
    cached
        .filter(|cached| cached.input == input)
        .unwrap_or_else(|| {
            let execution = ingredient::IngredientParser::new().parse_line(
                input,
                ingredient::ParseOptions {
                    decomposition: false,
                    trace: ingredient::TraceDetail::Full,
                },
            );
            std::sync::Arc::new(CachedInspection {
                input: input.to_owned(),
                ingredient: execution.ingredient,
                stages: execution.stages.unwrap_or_default(),
                trace: execution.trace.unwrap_or_else(|| ParseTrace::new(input)),
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inspection_cache_reuses_source_and_replaces_changed_source() {
        let first = cached_inspection(None, "1 cup flour");
        let same = cached_inspection(Some(first.clone()), "1 cup flour");
        assert!(std::sync::Arc::ptr_eq(&first, &same));
        let changed = cached_inspection(Some(same), "(2 tbsp oil)");
        assert!(!std::sync::Arc::ptr_eq(&first, &changed));
        assert_eq!(changed.input, "(2 tbsp oil)");
        assert_eq!(changed.trace.input, changed.input);
        let expected = ingredient::IngredientParser::new().parse_line(
            &changed.input,
            ingredient::ParseOptions {
                decomposition: false,
                trace: ingredient::TraceDetail::Full,
            },
        );
        assert_eq!(changed.ingredient, expected.ingredient);
        assert!(changed.ingredient.optional);
    }
}
