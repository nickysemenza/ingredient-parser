//! Source-first inspection of the same durable runs used by food-cli.
use eframe::egui;
use recipe_epub::review::ReviewRun;
use recipe_epub::review::{ReviewDecision, ReviewDecisions};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Default)]
pub(super) struct ReviewPanel {
    path: Option<PathBuf>,
    show_stats: bool,
    stats: super::ingredient_stats::StatsPanel,
    run: Option<ReviewRun>,
    selected: usize,
    filter: String,
    missing_only: bool,
    discrepancies_only: bool,
    status_filter: String,
    decisions: BTreeMap<String, ReviewDecision>,
    error: Option<String>,
    evaluation: Option<serde_json::Value>,
    images: Vec<(String, Vec<u8>)>,
    images_registered: bool,
    discrepancies: BTreeMap<String, Vec<serde_json::Value>>,
}

impl ReviewPanel {
    pub fn open(&mut self, path: PathBuf) {
        let result = (|| -> Result<_, Box<dyn std::error::Error>> {
            let run = ReviewRun::read(&path)?;
            let notes = path.with_extension("review.json");
            let decisions = ReviewDecisions::read(&notes, &run)?.documents;
            Ok((run, decisions))
        })();
        match result {
            Ok((run, decisions)) => {
                self.path = Some(path);
                self.stats = super::ingredient_stats::StatsPanel::default();
                match recipe_epub::review::stats::ingredient_stats(&run) {
                    Ok(stats) => self.stats.stats = Some(stats),
                    Err(error) => self.stats.error = Some(error.to_string()),
                }
                self.show_stats = false;
                self.run = Some(run);
                self.decisions = decisions;
                self.selected = 0;
                self.filter.clear();
                self.missing_only = false;
                self.discrepancies_only = false;
                self.status_filter.clear();
                self.evaluation = None;
                self.discrepancies.clear();
                self.images.clear();
                self.images_registered = false;
                self.error = None;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui.button("Open review run…").clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("Review run", &["json"])
                    .pick_file()
            {
                self.open(path);
            }
            if ui
                .add_enabled(
                    self.run.is_some(),
                    egui::Button::new("Load source EPUB images…"),
                )
                .clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("Source EPUB", &["epub"])
                    .pick_file()
                && let Some(run) = &self.run
            {
                let result = std::fs::read(path)
                    .map_err(|e| e.to_string())
                    .and_then(|bytes| {
                        recipe_epub::review::source_images(run, &bytes).map_err(|e| e.to_string())
                    });
                match result {
                    Ok(images) => {
                        self.images = images;
                        self.images_registered = false;
                    }
                    Err(error) => self.error = Some(error),
                }
            }
            if ui
                .add_enabled(self.run.is_some(), egui::Button::new("Load expectations…"))
                .clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("Expectations", &["json"])
                    .pick_file()
                && let Some(run) = &self.run
            {
                let result = (|| -> Result<_, Box<dyn std::error::Error>> {
                    let expected: serde_json::Value =
                        serde_json::from_slice(&std::fs::read(path)?)?;
                    Ok(recipe_epub::review::evaluate_document(run, expected)?)
                })();
                match result {
                    Ok(value) => {
                        self.discrepancies =
                            recipe_epub::review::discrepancies_by_source(run, &value);
                        self.evaluation = Some(value);
                    }
                    Err(error) => self.error = Some(error.to_string()),
                }
            }
        });
        if self.run.is_some() {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.show_stats, false, "Source review");
                ui.selectable_value(&mut self.show_stats, true, "Ingredient names");
            });
        }
        if self.show_stats {
            if let Some(doc) = self.stats.show(ui)
                && let Some(index) = self
                    .run
                    .as_ref()
                    .and_then(|run| run.documents.iter().position(|d| d.path == doc))
            {
                self.selected = index;
                self.filter.clear();
                self.status_filter.clear();
                self.missing_only = false;
                self.discrepancies_only = false;
                self.show_stats = false;
            }
            return;
        }
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut self.missing_only, "Missing extraction only");
            ui.add_enabled(
                self.evaluation.is_some(),
                egui::Checkbox::new(&mut self.discrepancies_only, "Discrepancies only"),
            );
            egui::ComboBox::from_id_salt("review-status-filter")
                .selected_text(if self.status_filter.is_empty() {
                    "All review states"
                } else {
                    &self.status_filter
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.status_filter,
                        String::new(),
                        "All review states",
                    );
                    for status in ["Unreviewed", "Accepted", "Incorrect", "Uncertain"] {
                        ui.selectable_value(&mut self.status_filter, status.into(), status);
                    }
                });
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .hint_text("Find source text or document"),
            );
        });
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
        }
        if let Some(evaluation) = &self.evaluation {
            ui.collapsing("Evaluation discrepancies", |ui| {
                ui.label(serde_json::to_string_pretty(evaluation).unwrap_or_default());
            });
        }
        let Some(run) = &self.run else {
            ui.label("Open a run created with food-cli epub extract. All source documents remain visible, including content with no extraction.");
            return;
        };
        if !self.images_registered {
            for (path, bytes) in &self.images {
                ui.ctx().include_bytes(
                    format!("bytes://review/{}/{path}", run.epub_sha256),
                    bytes.clone(),
                );
            }
            self.images_registered = true;
        }
        ui.label(format!(
            "{} recipes · {} source documents · {} missing chunks · ${:.4} reserved/spent",
            run.recipes.len(),
            run.documents.len(),
            run.chunks.iter().filter(|c| c.output.is_none()).count(),
            run.reserved_usd
        ));
        let mut save = false;
        ui.horizontal_top(|ui| {
            ui.allocate_ui_with_layout(egui::vec2(220.0, ui.available_height()), egui::Layout::top_down(egui::Align::Min), |ui| {
            egui::ScrollArea::vertical().id_salt("review-nav").max_width(220.0).show(ui, |ui| {
                let query = self.filter.to_lowercase();
                for (i, doc) in run.documents.iter().enumerate() {
                    let missing = run.chunks.iter().any(|c| c.source.doc_path == doc.path && c.output.is_none());
                    if self.missing_only && !missing { continue; }
                    if self.discrepancies_only && !self.discrepancies.contains_key(&doc.path) { continue; }
                    let status = self.decisions.get(&doc.path).map(|d| d.status.as_str()).filter(|s| !s.is_empty()).unwrap_or("Unreviewed");
                    if !self.status_filter.is_empty() && self.status_filter != status { continue; }
                    if !query.is_empty() && !doc.path.to_lowercase().contains(&query) && !doc.blocks.iter().any(|b| b.text.to_lowercase().contains(&query)) { continue; }
                    let title = doc.blocks.iter().find(|b| !b.text.is_empty()).map(|b| b.text.as_str()).unwrap_or(&doc.path);
                    ui.selectable_value(&mut self.selected, i, format!("{}{}", if missing { "Missing: " } else { "" }, title.chars().take(65).collect::<String>())).on_hover_text(&doc.path);
                }
            });
            });
            ui.separator();
            let Some(doc) = run.documents.get(self.selected) else { return; };
            ui.vertical(|ui| {
                ui.label(&doc.path);
                if let Some(issues) = self.discrepancies.get(&doc.path) {
                    for issue in issues { ui.colored_label(egui::Color32::LIGHT_RED, issue.to_string()); }
                }
                let decision = self.decisions.entry(doc.path.clone()).or_default();
                ui.horizontal_wrapped(|ui| { for status in ["Unreviewed", "Accepted", "Incorrect", "Uncertain"] { ui.selectable_value(&mut decision.status, status.into(), status); } save = ui.button("Save review").clicked(); });
                ui.add(egui::TextEdit::singleline(&mut decision.note).hint_text("Source evidence or uncertainty"));
                ui.columns(2, |columns| {
                    columns[0].heading("Source");
                    egui::ScrollArea::vertical().id_salt("review-source").show(&mut columns[0], |ui| {
                        for block in &doc.blocks { ui.label(&block.text).on_hover_text(&block.id); }
                        for image in &doc.images {
                            ui.label(&image.path);
                            if let Some(annotation) = run.image_text.as_ref().and_then(|text|text.images.iter().find(|a|a.path == image.path)) {
                                for caption in &annotation.captions { ui.label(caption); }
                            }
                            if self.images.iter().any(|(p, _)| p == &image.path) { ui.add(egui::Image::new(format!("bytes://review/{}/{}", run.epub_sha256, image.path)).fit_to_exact_size(egui::vec2(ui.available_width(), 240.0)).maintain_aspect_ratio(true)); }
                        }
                        if doc.blocks.is_empty() && doc.images.is_empty() { ui.label("No paragraph blocks or images. Inspect the cleaned chunks below for other markup."); }
                        for chunk in run.chunks.iter().filter(|c| c.source.doc_path == doc.path) {
                            ui.collapsing(format!("Model input: {}",chunk.id), |ui| { ui.label(&chunk.source.text); if let Some(error) = &chunk.error { ui.label(error); } });
                        }
                    });
                    columns[1].heading("Extracted recipe and ingredients");
                    egui::ScrollArea::vertical().id_salt("review-output").show(&mut columns[1], |ui| {
                        let recipes: Vec<_> = run.recipes.iter().enumerate().filter(|(_, r)| r.url.rsplit_once('#').is_some_and(|(_, d)| d == doc.path) || r.image.as_ref().is_some_and(|image|doc.images.iter().any(|source|source.path == image.path))).collect();
                        if recipes.is_empty() { ui.label("No assembled recipe for this source document. Review it before marking it accepted."); }
                        for (ri, recipe) in recipes {
                            ui.push_id(ri, |ui| {
                                ui.heading(&recipe.meta.title);
                                if let Some(description) = &recipe.meta.description { ui.label(description); }
                                if let Some(yield_) = &recipe.meta.recipe_yield { ui.strong(yield_); }
                                for (si, section) in recipe.sections.iter().enumerate() {
                                    ui.push_id(si, |ui| {
                                        if let Some(name) = &section.name { ui.strong(name); }
                                        for (i, line) in section.ingredients.iter().enumerate() {
                                            ui.push_id(i, |ui| { ui.label(line); super::recipe::show_ingredient_collapsing(ui, &ingredient::from_str(line)); ui.collapsing("Source attribution", |ui| { for span in ingredient::decompose(line).spans { ui.label(format!("{:?}: {}", span.field, span.text)); } }); });
                                        }
                                        for step in &section.instructions { ui.label(step); }
                                    });
                                }
                                for note in &recipe.meta.notes { ui.label(note); }
                                ui.collapsing("Images, references, and metadata", |ui| { ui.label(serde_json::to_string_pretty(recipe).unwrap_or_default()); });
                            });
                        }
                    });
                });
            });
        });
        if save
            && let Some(path) = &self.path
            && let Some(run) = &self.run
        {
            self.error = ReviewDecisions {
                version: 1,
                epub_sha256: run.epub_sha256.clone(),
                documents: self.decisions.clone(),
            }
            .save(&path.with_extension("review.json"))
            .err()
            .map(|e| e.to_string());
        }
    }
}
