//! Test tab: batch-parse ingredient lines and inspect each line's result,
//! diagnostics, pipeline stages (the `--explain` view), and full trace tree.

use crate::theme;
use eframe::egui::{self, RichText};
use egui_extras::{Column, TableBuilder};
use ingredient::ingredient::Ingredient;
use ingredient::trace::{GrammarOutcome, ParseTrace, StageReport};
use ingredient::util::truncate_str;
use ingredient::{Confidence, ParseNotes};

use super::debug::{TraceTreeContext, show_trace_tree};

/// One parsed input line with everything the table and detail views need.
struct LineResult {
    input: String,
    ingredient: Option<Ingredient>,
    diagnostics: ParseNotes,
    stages: StageReport,
    trace: ParseTrace,
}

impl LineResult {
    fn name(&self) -> &str {
        self.ingredient.as_ref().map_or("", |i| &i.name)
    }

    fn amounts_text(&self) -> String {
        self.ingredient.as_ref().map_or_else(String::new, |i| {
            i.amounts
                .iter()
                .map(|a| a.to_string())
                .collect::<Vec<_>>()
                .join(" / ")
        })
    }

    fn modifier(&self) -> &str {
        self.ingredient
            .as_ref()
            .and_then(|i| i.modifier.as_deref())
            .unwrap_or("")
    }
}

#[derive(Clone, Copy, PartialEq)]
enum DetailView {
    Stages,
    Tree,
    Json,
}

#[derive(Clone, Copy, PartialEq)]
enum SortColumn {
    Input,
    Name,
    Amounts,
    Modifier,
    Confidence,
}

impl SortColumn {
    fn compare(self, a: &LineResult, b: &LineResult) -> std::cmp::Ordering {
        match self {
            Self::Input => a.input.cmp(&b.input),
            Self::Name => a.name().cmp(b.name()),
            Self::Amounts => a.amounts_text().cmp(&b.amounts_text()),
            Self::Modifier => a.modifier().cmp(b.modifier()),
            Self::Confidence => confidence_rank(a.diagnostics.confidence)
                .cmp(&confidence_rank(b.diagnostics.confidence)),
        }
    }
}

/// Sortable rank for a confidence level (Low < Medium < High).
fn confidence_rank(c: Confidence) -> u8 {
    match c {
        Confidence::Low => 0,
        Confidence::Medium => 1,
        Confidence::High => 2,
    }
}

/// State for the Test Parser tab.
pub struct TestTab {
    /// Multiline input, one ingredient line per line. Persisted across runs.
    pub(crate) input: String,
    results: Vec<LineResult>,
    selected: Option<usize>,
    detail: DetailView,
    /// Active sort: column + ascending. `None` keeps input order.
    sort: Option<(SortColumn, bool)>,
    /// Sorted view into `results` — rebuilt on parse or header click, never
    /// per frame.
    order: Vec<usize>,
}

impl Default for TestTab {
    fn default() -> Self {
        Self {
            input: "2 cups all-purpose flour, sifted".to_string(),
            results: Vec::new(),
            selected: None,
            detail: DetailView::Stages,
            sort: None,
            order: Vec::new(),
        }
    }
}

impl TestTab {
    pub fn show(&mut self, ui: &mut egui::Ui) {
        ui.heading("Test Ingredient Parser");
        ui.separator();

        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::multiline(&mut self.input)
                    .desired_rows(4)
                    .desired_width(500.0)
                    .font(egui::TextStyle::Monospace)
                    .hint_text("one ingredient line per line"),
            );
            ui.vertical(|ui| {
                // Cmd/Ctrl+Enter parses; plain Enter keeps inserting newlines
                // in the multiline editor.
                let hotkey = ui.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Enter));
                if ui.button("Parse").clicked() || hotkey {
                    self.parse();
                }
                ui.label(RichText::new("⌘⏎ to parse").weak().small());
            });
        });
        ui.separator();

        if self.results.is_empty() {
            ui.label("Enter ingredient lines and click Parse");
            return;
        }

        // Detail panel for the selected row — added before the table so the
        // table fills the remaining central space.
        if let Some(idx) = self.selected
            && let Some(row) = self.results.get(idx)
        {
            let detail = &mut self.detail;
            egui::Panel::bottom("test_detail")
                .resizable(true)
                .default_size(280.0)
                .show_inside(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.selectable_value(detail, DetailView::Stages, "Stages");
                        ui.selectable_value(detail, DetailView::Tree, "Trace tree");
                        ui.selectable_value(detail, DetailView::Json, "JSON");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("📤 Copy Jaeger JSON").clicked() {
                                ui.ctx().copy_text(row.trace.to_jaeger_json());
                            }
                            ui.label(RichText::new(truncate_str(&row.input, 60)).weak());
                        });
                    });
                    ui.separator();
                    egui::ScrollArea::both()
                        .id_salt("test_detail_scroll")
                        .show(ui, |ui| match detail {
                            DetailView::Stages => show_stages(ui, &row.stages),
                            DetailView::Tree => {
                                show_trace_tree(ui, &row.trace, TraceTreeContext::Test);
                            }
                            DetailView::Json => show_json(ui, row.ingredient.as_ref()),
                        });
                });
        }

        self.show_table(ui);
    }

    /// Parse every non-empty input line. Each line is parsed twice (traced +
    /// diagnostics) — fine for an interactive tool, and the traced parse is
    /// the expensive one anyway.
    fn parse(&mut self) {
        let parser = ingredient::IngredientParser::new();
        self.results = self
            .input
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(|line| {
                let traced = parser.parse_with_trace(line);
                let diagnostics = parser.from_str(line).parse_notes;
                LineResult {
                    input: line.to_string(),
                    stages: traced.trace.stages(),
                    ingredient: traced.result.ok(),
                    diagnostics,
                    trace: traced.trace,
                }
            })
            .collect();
        self.selected = (!self.results.is_empty()).then_some(0);
        self.rebuild_order();
    }

    fn rebuild_order(&mut self) {
        let mut order: Vec<usize> = (0..self.results.len()).collect();
        if let Some((col, ascending)) = self.sort {
            order.sort_by(|&a, &b| {
                let ord = col.compare(&self.results[a], &self.results[b]);
                if ascending { ord } else { ord.reverse() }
            });
        }
        self.order = order;
    }

    fn show_table(&mut self, ui: &mut egui::Ui) {
        // Locals so the table closures don't need `&mut self`; changes are
        // applied after the table renders.
        let mut sort = self.sort;
        let mut clicked = None;
        let selected = self.selected;
        let results = &self.results;
        let order = &self.order;

        let row_height = egui::TextStyle::Body.resolve(ui.style()).size + 8.0;
        TableBuilder::new(ui)
            .striped(true)
            .sense(egui::Sense::click())
            .column(Column::auto().at_least(200.0).resizable(true).clip(true))
            .column(Column::auto().at_least(140.0).resizable(true).clip(true))
            .column(Column::auto().at_least(110.0).resizable(true).clip(true))
            .column(Column::remainder().clip(true))
            .column(Column::auto().at_least(70.0))
            .header(22.0, |mut header| {
                for (label, col) in [
                    ("Input", SortColumn::Input),
                    ("Name", SortColumn::Name),
                    ("Amounts", SortColumn::Amounts),
                    ("Modifier", SortColumn::Modifier),
                    ("Confidence", SortColumn::Confidence),
                ] {
                    header.col(|ui| {
                        sort_header(ui, label, col, &mut sort);
                    });
                }
            })
            .body(|body| {
                body.rows(row_height, order.len(), |mut table_row| {
                    let idx = order[table_row.index()];
                    let r = &results[idx];
                    table_row.set_selected(selected == Some(idx));
                    table_row.col(|ui| {
                        ui.label(truncate_str(&r.input, 60)).on_hover_text(&r.input);
                    });
                    table_row.col(|ui| {
                        ui.label(RichText::new(r.name()).color(theme::palette().name()));
                    });
                    table_row.col(|ui| {
                        ui.label(RichText::new(r.amounts_text()).color(theme::palette().amount()));
                    });
                    table_row.col(|ui| {
                        ui.label(RichText::new(r.modifier()).color(theme::palette().modifier()));
                    });
                    table_row.col(|ui| {
                        confidence_badge(ui, &r.diagnostics);
                    });
                    if table_row.response().clicked() {
                        clicked = Some(idx);
                    }
                });
            });

        if let Some(idx) = clicked {
            self.selected = Some(idx);
        }
        if sort != self.sort {
            self.sort = sort;
            self.rebuild_order();
        }
    }
}

/// A clickable column header cycling none → ascending → descending → none.
fn sort_header(
    ui: &mut egui::Ui,
    label: &str,
    col: SortColumn,
    sort: &mut Option<(SortColumn, bool)>,
) {
    let marker = match sort {
        Some((c, true)) if *c == col => " ▲",
        Some((c, false)) if *c == col => " ▼",
        _ => "",
    };
    if ui
        .button(RichText::new(format!("{label}{marker}")).strong())
        .clicked()
    {
        *sort = match sort {
            Some((c, true)) if *c == col => Some((col, false)),
            Some((c, false)) if *c == col => None,
            _ => Some((col, true)),
        };
    }
}

/// Colored confidence label; hover explains the diagnostic flags.
fn confidence_badge(ui: &mut egui::Ui, diagnostics: &ParseNotes) {
    let (text, color) = match diagnostics.confidence {
        Confidence::High => ("High", theme::palette().trace_ok()),
        Confidence::Medium => ("Medium", theme::palette().trace_incomplete()),
        Confidence::Low => ("Low", theme::palette().trace_fail()),
    };
    let mut notes = Vec::new();
    if diagnostics.fell_back {
        notes.push("fell back to a name-only ingredient");
    }
    if diagnostics.unparsed_digit {
        notes.push("contains a digit that produced no amount (likely missed quantity)");
    }
    if notes.is_empty() {
        match diagnostics.confidence {
            Confidence::High => notes.push("structured parse with at least one amount"),
            Confidence::Medium => notes.push("clean name-only parse (no digit present)"),
            Confidence::Low => {}
        }
    }
    ui.label(RichText::new(text).color(color).strong())
        .on_hover_text(notes.join("\n"));
}

/// Render a [`StageReport`] as one card per pipeline stage, mirroring the
/// CLI's `--explain` view: normalize → recognize → grammar → refine → result.
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
        theme::card_compact(ui, |ui| {
            ui.vertical(add_contents);
        });
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
