//! Source-first inspection of the same durable runs used by food-cli.
use crate::theme;
use eframe::egui;
use poll_promise::Promise;
use recipe_epub::review::ReviewRun;
use recipe_epub::review::{ReviewDecision, ReviewDecisions};
use recipe_epub::{CookbookRecipeExt, ParsedCookbookRecipe};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

#[derive(Default)]
pub(super) struct ReviewPanel {
    path: Option<PathBuf>,
    worker: Option<Promise<Result<ReviewRun, String>>>,
    extracting: bool,
    completed: Arc<AtomicUsize>,
    total: usize,
    allow_network: bool,
    budget_usd: Option<f64>,
    graph: Option<super::cookbook::RefGraph>,
    graph_prewarmed: bool,
    graph_selected: usize,
    show_graph: bool,
    compact_source: bool,
    selected_ingredient: Option<String>,
    parsed_previews: BTreeMap<usize, (ParsedCookbookRecipe, f64)>,
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
    saved_document: Option<String>,
    evaluation: Option<serde_json::Value>,
    images: Vec<(String, Vec<u8>)>,
    images_registered: bool,
    discrepancies: BTreeMap<String, Vec<serde_json::Value>>,
}

impl ReviewPanel {
    pub fn source_path(&self) -> Option<&str> {
        self.run.as_ref().map(|run| run.source.as_str())
    }

    fn save_decisions(&self, path: &std::path::Path, run: &ReviewRun) -> Result<(), String> {
        ReviewDecisions {
            version: 1,
            epub_sha256: run.epub_sha256.clone(),
            documents: self.decisions.clone(),
        }
        .save(&path.with_extension("review.json"))
        .map_err(|error| error.to_string())
    }

    pub fn busy(&self) -> bool {
        self.worker.is_some()
    }
    pub fn invalidate_graph(&mut self) {
        self.graph = None;
        self.graph_prewarmed = false;
    }

    fn accept_run(&mut self, run: ReviewRun, reset: bool) {
        self.saved_document = None;
        self.stats = super::ingredient_stats::StatsPanel::default();
        match recipe_epub::review::stats::ingredient_stats(&run) {
            Ok(stats) => self.stats.stats = Some(stats),
            Err(error) => self.stats.error = Some(error.to_string()),
        }
        self.invalidate_graph();
        self.run = Some(run);
        self.parsed_previews.clear();
        self.selected_ingredient = None;
        self.evaluation = None;
        self.discrepancies.clear();
        if reset {
            self.selected = 0;
            self.filter.clear();
            self.missing_only = false;
            self.discrepancies_only = false;
            self.status_filter.clear();
            self.show_stats = false;
            self.show_graph = false;
            self.selected_ingredient = None;
            self.evaluation = None;
            self.discrepancies.clear();
            self.images.clear();
            self.images_registered = false;
        }
    }

    pub fn open(&mut self, path: PathBuf) {
        if self.busy() {
            return;
        }
        let result = (|| -> Result<_, Box<dyn std::error::Error>> {
            let run = ReviewRun::read(&path)?;
            let decisions =
                ReviewDecisions::read(&path.with_extension("review.json"), &run)?.documents;
            Ok((run, decisions))
        })();
        match result {
            Ok((run, decisions)) => {
                self.path = Some(path);
                self.decisions = decisions;
                self.error = None;
                self.accept_run(run, true);
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    /// Inspection never reads cache entries, runs extraction, or creates a file.
    pub fn inspect(&mut self, path: PathBuf, ctx: egui::Context) {
        if self.busy() {
            return;
        }
        self.run = None;
        self.path = None;
        self.decisions.clear();
        self.error = None;
        self.extracting = false;
        self.worker = Some(Promise::spawn_thread("inspect_cookbook", move || {
            let result = std::fs::read(&path)
                .map_err(|e| format!("Read {}: {e}", path.display()))
                .and_then(|bytes| {
                    ReviewRun::inspect(&bytes, &path.to_string_lossy(), "gemini-2.5-flash")
                        .map_err(|e| e.to_string())
                });
            ctx.request_repaint();
            result
        }));
    }

    fn extract(&mut self, path: PathBuf, ctx: egui::Context) {
        if self.busy() {
            return;
        }
        let Some(mut run) = self.run.clone() else {
            return;
        };
        self.total = run.chunks.len();
        self.completed = Arc::new(AtomicUsize::new(
            run.chunks.iter().filter(|c| c.output.is_some()).count(),
        ));
        let completed = self.completed.clone();
        let options = recipe_epub::review::RunOptions {
            allow_network: self.allow_network,
            budget_usd: self.budget_usd.unwrap_or(10.0),
            ..Default::default()
        };
        self.path = Some(path.clone());
        self.error = None;
        self.extracting = true;
        self.worker = Some(Promise::spawn_thread("extract_cookbook", move || {
            let result = (|| -> Result<_, String> {
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| e.to_string())?;
                rt.block_on(recipe_epub::review::extract_run_with_progress(
                    &mut run,
                    &options,
                    &path,
                    |run| {
                        completed.store(
                            run.chunks.iter().filter(|c| c.output.is_some()).count(),
                            Ordering::Relaxed,
                        );
                        ctx.request_repaint();
                    },
                ))
                .map_err(|e| e.to_string())?;
                Ok(run)
            })();
            ctx.request_repaint();
            result
        }));
    }

    fn extraction_menu(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Extraction…", |ui| {
            ui.set_min_width(260.0);
            ui.strong("Extraction settings");
            ui.add_space(8.0);
            ui.checkbox(&mut self.allow_network, "Allow network requests");
            if self.allow_network {
                ui.horizontal(|ui| {
                    ui.label("Total budget $");
                    ui.add(
                        egui::DragValue::new(self.budget_usd.get_or_insert(10.0))
                            .range(0.0..=1000.0)
                            .speed(0.25),
                    );
                });
            } else {
                ui.weak("Uses cached results only.");
            }
            ui.add_space(12.0);
            if theme::primary_button(
                ui,
                if self.path.is_some() {
                    "Resume extraction"
                } else {
                    "Extract to saved run…"
                },
            )
            .clicked()
            {
                let path = self.path.clone().or_else(|| {
                    rfd::FileDialog::new()
                        .add_filter("Run", &["json"])
                        .set_file_name("cookbook-run.json")
                        .save_file()
                });
                if let Some(path) = path {
                    self.extract(path, ui.ctx().clone());
                }
                ui.close();
            }
        });
    }

    fn run_tools(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Run tools", |ui| {
            if let Some(path) = &self.path {
                ui.label(
                    egui::RichText::new(path.display().to_string())
                        .small()
                        .weak(),
                );
            }
            if let Some(run) = &self.run {
                ui.weak(format!("${:.4} reserved/spent", run.reserved_usd));
            }
            ui.separator();
            if ui.button("Copy recipes JSON").clicked() {
                if let Some(run) = &self.run {
                    match serde_json::to_string_pretty(&run.recipes) {
                        Ok(json) => ui.ctx().copy_text(json),
                        Err(error) => self.error = Some(error.to_string()),
                    }
                }
                ui.close();
            }
            if ui.button("Ingredient statistics").clicked() {
                self.show_stats = true;
                self.show_graph = false;
                ui.close();
            }
            if ui.button("Reference graph").clicked() {
                self.show_graph = true;
                self.show_stats = false;
                ui.close();
            }
            if ui.button("Replay to new run…").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Run", &["json"])
                    .set_file_name("replayed-run.json")
                    .save_file()
                    && let Some(mut run) = self.run.clone()
                {
                    match run
                        .replay()
                        .and_then(|()| run.save(&path))
                        .map_err(|error| error.to_string())
                        .and_then(|()| self.save_decisions(&path, &run))
                    {
                        Ok(()) => {
                            self.path = Some(path);
                            self.accept_run(run, false);
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
                ui.close();
            }
            ui.separator();
            if ui.button("Load source EPUB images…").clicked() {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Source EPUB", &["epub"])
                    .pick_file()
                    && let Some(run) = &self.run
                {
                    let result = std::fs::read(path)
                        .map_err(|e| e.to_string())
                        .and_then(|bytes| {
                            recipe_epub::review::source_images(run, &bytes)
                                .map_err(|e| e.to_string())
                        });
                    match result {
                        Ok(images) => {
                            self.images = images;
                            self.images_registered = false;
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
                ui.close();
            }
            if ui.button("Load expectations…").clicked() {
                if let Some(path) = rfd::FileDialog::new()
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
                ui.close();
            }
        });
    }

    pub fn show(&mut self, ui: &mut egui::Ui) {
        if self
            .worker
            .as_ref()
            .is_some_and(|worker| worker.ready().is_some())
            && let Some(worker) = self.worker.take()
        {
            match worker.block_and_take() {
                Ok(run) => self.accept_run(run, !self.extracting),
                Err(error) => {
                    // Checkpoints preserve completed work even when extraction fails.
                    if self.extracting
                        && let Some(path) = &self.path
                        && let Ok(run) = ReviewRun::read(path)
                    {
                        self.accept_run(run, false);
                    }
                    self.error = Some(error);
                }
            }
        }
        if self.busy() {
            if self.extracting {
                let done = self.completed.load(Ordering::Relaxed);
                ui.add(
                    egui::ProgressBar::new(done as f32 / self.total.max(1) as f32)
                        .text(format!("Extracting {done}/{} chunks", self.total)),
                );
            } else {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Inspecting source…");
                });
            }
        }
        if let Some(run) = &self.run {
            let compact_header = ui.available_width() < 760.0 || ui.available_height() < 580.0;
            let title = std::path::Path::new(&run.source)
                .file_stem()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| run.source.clone());
            let summary = format!(
                "{} recipes · {} documents",
                run.recipes.len(),
                run.documents.len()
            );
            let missing = run
                .chunks
                .iter()
                .filter(|chunk| chunk.output.is_none())
                .count();
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(egui::RichText::new(title).size(20.0).strong())
                        .on_hover_text(&summary);
                    if !compact_header {
                        ui.label(egui::RichText::new(summary).weak());
                    }
                    if missing > 0 {
                        ui.colored_label(
                            theme::palette().trace_incomplete(),
                            format!("{missing} chunks need extraction"),
                        );
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add_enabled_ui(!self.busy(), |ui| {
                        self.run_tools(ui);
                        self.extraction_menu(ui);
                    });
                });
            });
            ui.add_space(if compact_header { 4.0 } else { 16.0 });
        }
        if (self.show_stats || self.show_graph) && ui.button("Back to source review").clicked() {
            self.show_stats = false;
            self.show_graph = false;
        }
        if self.show_graph
            && let Some(run) = &self.run
        {
            if run
                .recipes
                .iter()
                .all(|recipe| recipe.references.is_empty())
            {
                ui.label("No cross-recipe references in this run.");
                return;
            }
            let graph = self
                .graph
                .get_or_insert_with(|| super::cookbook::build_reference_graph(&run.recipes));
            if let Some(index) = super::cookbook::show_reference_graph(
                ui,
                graph,
                &mut self.graph_selected,
                &mut self.graph_prewarmed,
            ) {
                if let Some(recipe) = run.recipes.get(index)
                    && let Some((_, path)) = recipe.url.rsplit_once('#')
                    && let Some(index) = run.documents.iter().position(|d| d.path == path)
                {
                    self.selected = index;
                    self.selected_ingredient = None;
                }
                self.show_graph = false;
            }
            return;
        }
        if self.show_stats {
            if let Some(doc) = self.stats.show(ui)
                && let Some(index) = self
                    .run
                    .as_ref()
                    .and_then(|run| run.documents.iter().position(|d| d.path == doc))
            {
                self.selected = index;
                self.selected_ingredient = None;
                self.filter.clear();
                self.status_filter.clear();
                self.missing_only = false;
                self.discrepancies_only = false;
                self.show_stats = false;
            }
            return;
        }
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        if let Some(evaluation) = &self.evaluation {
            ui.collapsing("Evaluation discrepancies", |ui| {
                ui.label(serde_json::to_string_pretty(evaluation).unwrap_or_default());
            });
        }
        let Some(run) = &self.run else {
            ui.label("Open an EPUB to inspect its source, or a saved run to continue review. Extraction starts only when requested; saved runs work offline.");
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
        // A short window cannot fit document controls plus two independently
        // scrolling work areas. Drill into inspection, retaining the document
        // selection so closing it returns to the same recipe.
        if (ui.available_width() < 760.0 || ui.available_height() < 480.0)
            && let Some(line) = &self.selected_ingredient
        {
            let mut close = ui.button("← Back to review").clicked();
            if let Some(doc) = run.documents.get(self.selected) {
                let title = doc
                    .blocks
                    .iter()
                    .find(|block| matches!(block.tag.as_str(), "h1" | "h2" | "h3"))
                    .or_else(|| doc.blocks.first())
                    .map(|block| block.text.as_str())
                    .unwrap_or(&doc.path);
                ui.label(egui::RichText::new(title).strong());
            }
            ui.add_space(8.0);
            theme::sidebar_frame().show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                close |= super::inspector::show_line_inspector(ui, line);
            });
            if close {
                self.selected_ingredient = None;
            }
            return;
        }
        let mut save = false;
        let mut close_inspector = false;
        if let Some(line) = &self.selected_ingredient {
            let panel = if ui.available_width() < 1050.0 {
                egui::Panel::bottom("cookbook-ingredient-inspector-bottom-v2")
                    .default_size(185.0)
                    .max_size((ui.available_height() * 0.42).max(130.0))
            } else {
                egui::Panel::right("cookbook-ingredient-inspector").default_size(330.0)
            };
            panel
                .resizable(true)
                .frame(theme::sidebar_frame())
                .show(ui, |ui| {
                    close_inspector = super::inspector::show_line_inspector(ui, line);
                });
        }
        if close_inspector {
            self.selected_ingredient = None;
        }
        let previous_selected = self.selected;
        let query = self.filter.to_lowercase();
        let visible: Vec<usize> = run
            .documents
            .iter()
            .enumerate()
            .filter(|(_, doc)| {
                let missing = run
                    .chunks
                    .iter()
                    .any(|c| c.source.doc_path == doc.path && c.output.is_none());
                let status = self
                    .decisions
                    .get(&doc.path)
                    .map(|d| d.status.as_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or("Unreviewed");
                (!self.missing_only || missing)
                    && (!self.discrepancies_only || self.discrepancies.contains_key(&doc.path))
                    && (self.status_filter.is_empty() || self.status_filter == status)
                    && (query.is_empty()
                        || doc.path.to_lowercase().contains(&query)
                        || doc
                            .blocks
                            .iter()
                            .any(|b| b.text.to_lowercase().contains(&query)))
            })
            .map(|(i, _)| i)
            .collect();
        if !ui.ctx().egui_wants_keyboard_input() {
            let direction = ui.input(|i| {
                if i.key_pressed(egui::Key::ArrowDown) {
                    1
                } else if i.key_pressed(egui::Key::ArrowUp) {
                    -1
                } else {
                    0
                }
            });
            if direction != 0 && !visible.is_empty() {
                let current = visible
                    .iter()
                    .position(|&i| i == self.selected)
                    .unwrap_or(0);
                let next =
                    (current as isize + direction).clamp(0, visible.len() as isize - 1) as usize;
                self.selected = visible[next];
            }
        }
        let narrow_navigation = ui.available_width() < 760.0;
        let mut navigation = |ui: &mut egui::Ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Documents").size(17.0).strong());
                ui.menu_button("Filter", |ui| {
                    ui.checkbox(&mut self.missing_only, "Missing extraction");
                    ui.add_enabled(
                        self.evaluation.is_some(),
                        egui::Checkbox::new(&mut self.discrepancies_only, "Discrepancies"),
                    );
                    ui.separator();
                    for status in ["", "Unreviewed", "Accepted", "Incorrect", "Uncertain"] {
                        ui.selectable_value(
                            &mut self.status_filter,
                            status.into(),
                            if status.is_empty() {
                                "All review states"
                            } else {
                                status
                            },
                        );
                    }
                });
            });
            ui.add_space(8.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.filter)
                    .desired_width(f32::INFINITY)
                    .hint_text("Find document…"),
            );
            ui.add_space(12.0);
            egui::ScrollArea::vertical()
                .id_salt("review-nav")
                .show(ui, |ui| {
                    if visible.is_empty() {
                        ui.label("No documents match these filters.");
                    }
                    for &i in &visible {
                        let doc = &run.documents[i];
                        let missing = run
                            .chunks
                            .iter()
                            .any(|c| c.source.doc_path == doc.path && c.output.is_none());
                        let title = doc
                            .blocks
                            .iter()
                            .find(|b| !b.text.is_empty())
                            .map(|b| b.text.as_str())
                            .unwrap_or(&doc.path);
                        let response = ui.add_sized(
                            [ui.available_width(), 34.0],
                            egui::Button::new(egui::RichText::new(title).strong())
                                .right_text(())
                                .selected(self.selected == i)
                                .frame_when_inactive(self.selected == i),
                        );
                        if response.clicked() {
                            self.selected = i;
                        }
                        response.clone().on_hover_text(&doc.path);
                        if missing {
                            ui.label(
                                egui::RichText::new("Needs extraction")
                                    .small()
                                    .color(theme::palette().trace_incomplete()),
                            );
                        }
                        ui.add_space(4.0);
                        if self.selected == i && previous_selected != self.selected {
                            response.scroll_to_me(Some(egui::Align::Center));
                        }
                    }
                });
        };
        if narrow_navigation {
            ui.menu_button("Documents…", |ui| {
                ui.set_width(260.0);
                navigation(ui);
            });
            ui.add_space(8.0);
        } else {
            egui::Panel::left("review-documents-v2")
                .resizable(true)
                .default_size(220.0)
                .size_range(170.0..=300.0)
                .frame(theme::sidebar_frame())
                .show(ui, navigation);
        }
        if previous_selected != self.selected {
            self.selected_ingredient = None;
        }
        let Some(doc) = run.documents.get(self.selected) else {
            return;
        };
        ui.vertical(|ui| {
                let compact_comparison = ui.available_width() < 650.0;
                let title = doc.blocks.iter().find(|block| matches!(block.tag.as_str(), "h1" | "h2" | "h3"))
                    .or_else(|| doc.blocks.first()).map(|block| block.text.as_str()).unwrap_or(&doc.path);
                ui.label(egui::RichText::new(title).size(24.0).strong());
                ui.label(egui::RichText::new(&doc.path).small().weak());
                ui.add_space(if compact_comparison { 4.0 } else { 12.0 });
                if let Some(issues) = self.discrepancies.get(&doc.path) {
                    for issue in issues { ui.colored_label(ui.visuals().error_fg_color, issue.to_string()); }
                }
                let decision = self.decisions.entry(doc.path.clone()).or_default();
                ui.horizontal_wrapped(|ui| {
                    ui.label("Review status");
                    egui::ComboBox::from_id_salt("document-review-status")
                        .selected_text(if decision.status.is_empty() { "Unreviewed" } else { &decision.status })
                        .show_ui(ui, |ui| { for status in ["Unreviewed", "Accepted", "Incorrect", "Uncertain"] {
                            if ui.selectable_value(&mut decision.status, status.into(), status).changed() { self.saved_document = None; }
                        }});
                    ui.add_space(8.0);
                    ui.add_enabled_ui(self.path.is_some(), |ui| { save = theme::primary_button(ui, "Save review").clicked(); });
                    if self.saved_document.as_deref() == Some(&doc.path) { ui.colored_label(theme::palette().trace_ok(), "Saved"); }
                    if compact_comparison { ui.menu_button("Review note", |ui| {
                        if ui.add(egui::TextEdit::multiline(&mut decision.note).desired_width(260.0).desired_rows(3).hint_text("Evidence, uncertainty, or corrections…")).changed() { self.saved_document = None; }
                    }); }
                });
                if !compact_comparison {
                ui.add_space(6.0);
                ui.collapsing(if decision.note.is_empty() { "Add review note" } else { "Review note" }, |ui| {
                    if ui.add(egui::TextEdit::multiline(&mut decision.note).desired_width(f32::INFINITY).desired_rows(2).hint_text("Evidence, uncertainty, or corrections…")).changed() { self.saved_document = None; }
                });
                }
                ui.add_space(if compact_comparison { 4.0 } else { 16.0 });
                if compact_comparison {
                    ui.horizontal(|ui| {
                        ui.selectable_value(&mut self.compact_source, true, "Source document");
                        ui.selectable_value(&mut self.compact_source, false, "Extracted result");
                    });
                    ui.add_space(8.0);
                }
                let render_source = |ui: &mut egui::Ui| {
                    if !compact_comparison { theme::pane_heading(ui, "Source document"); }
                    egui::ScrollArea::vertical().id_salt("review-source").show(ui, |ui| {
                        for block in &doc.blocks {
                            let text = egui::RichText::new(&block.text);
                            if matches!(block.tag.as_str(), "h1" | "h2" | "h3") {
                                ui.add_space(8.0); ui.label(text.size(20.0).strong()).on_hover_text(&block.id); ui.add_space(10.0);
                            } else { ui.label(text).on_hover_text(&block.id); ui.add_space(4.0); }
                        }
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
                };
                if compact_comparison {
                    if self.compact_source { theme::evidence_frame().show(ui, render_source); }
                } else {
                    theme::comparison_panel(ui).resizable(true).frame(theme::evidence_frame()).show(ui, render_source);
                }
                if !compact_comparison || !self.compact_source {
                    egui::Frame::new().inner_margin(egui::Margin::symmetric(16, 16)).show(ui, |ui| {
                    if !compact_comparison { theme::pane_heading(ui, "Extracted result"); }
                    egui::ScrollArea::vertical().id_salt("review-output").show(ui, |ui| {
                        let recipes: Vec<_> = run.recipes.iter().enumerate().filter(|(_, r)| r.url.rsplit_once('#').is_some_and(|(_, d)| d == doc.path) || r.image.as_ref().is_some_and(|image|doc.images.iter().any(|source|source.path == image.path))).collect();
                        if recipes.is_empty() { ui.label("No assembled recipe for this source document. Review it before marking it accepted."); }
                        for (ri, recipe) in recipes {
                            ui.push_id(ri, |ui| {
                                if !compact_comparison || recipe.meta.title != title {
                                    ui.label(egui::RichText::new(&recipe.meta.title).size(20.0).strong());
                                    ui.add_space(8.0);
                                }
                                if let Some(description) = &recipe.meta.description { ui.label(description); }
                                if let Some(yield_) = &recipe.meta.recipe_yield { ui.strong(yield_); }
                                for (si, section) in recipe.sections.iter().enumerate() {
                                    ui.push_id(si, |ui| {
                                        if let Some(name) = &section.name { ui.strong(name); }
                                        ui.add_space(16.0);
                                        ui.strong("Ingredients");
                                        ui.add_space(6.0);
                                        for (i, line) in section.ingredients.iter().enumerate() {
                                            ui.push_id(i, |ui| {
                                                if ui.add_sized([ui.available_width(), 30.0], egui::Button::new(line).right_text(())
                                                    .selected(self.selected_ingredient.as_deref() == Some(line.as_str())).frame_when_inactive(self.selected_ingredient.as_deref() == Some(line.as_str()))).clicked() {
                                                    self.selected_ingredient = Some(line.clone());
                                                }
                                            });
                                        }
                                        if !section.instructions.is_empty() { ui.add_space(18.0); ui.strong("Instructions"); ui.add_space(6.0); }
                                        for (i, step) in section.instructions.iter().enumerate() {
                                            ui.horizontal_top(|ui| { ui.weak(format!("{}.", i+1)); ui.add(egui::Label::new(step).wrap()); }); ui.add_space(8.0);
                                        }
                                    });
                                }
                                ui.collapsing("Parsed preview", |ui| {
                                    let (parsed, scale) = self.parsed_previews.entry(ri).or_insert_with(|| (recipe.parse(), 1.0));
                                    show_scaled_preview(ui, parsed, scale);
                                });
                                for note in &recipe.meta.notes { ui.label(note); }
                                ui.collapsing("Images, references, and metadata", |ui| { ui.label(serde_json::to_string_pretty(recipe).unwrap_or_default()); });
                            });
                        }
                    });
                    });
                }
            });
        if save
            && let Some(path) = &self.path
            && let Some(run) = &self.run
        {
            self.error = self.save_decisions(path, run).err();
            if self.error.is_none() {
                self.saved_document = Some(doc.path.clone());
            }
        }
    }
}

/// Scaling is presentation-only; authored and saved values remain unchanged.
fn show_scaled_preview(ui: &mut egui::Ui, parsed: &ParsedCookbookRecipe, scale: &mut f64) {
    ui.horizontal_wrapped(|ui| {
        ui.label("Scale");
        for factor in [0.5, 1.0, 2.0, 3.0] {
            ui.selectable_value(scale, factor, format!("{factor}×"));
        }
        ui.add(
            egui::DragValue::new(scale)
                .range(0.01..=100.0)
                .speed(0.1)
                .suffix("×"),
        );
    });
    for (index, section) in parsed.sections.iter().enumerate() {
        ui.push_id(index, |ui| {
            if let Some(name) = &section.name {
                ui.strong(name);
            }
            for line in &section.ingredients {
                ui.label(super::recipe::make_rich_scaled(line, *scale));
            }
            for chunks in &section.instructions {
                super::recipe::show_instruction_chunks_scaled(ui, chunks, *scale);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run() -> ReviewRun {
        ReviewRun {
            version: recipe_epub::review::RUN_VERSION,
            epub_sha256: "test-source".into(),
            source: "fixture.epub".into(),
            model: "offline".into(),
            prompt_version: "test".into(),
            parent: None,
            chunks: vec![],
            documents: vec![],
            recipes: vec![],
            parsed: serde_json::json!([]),
            reserved_usd: 0.0,
            image_text: None,
        }
    }

    #[test]
    fn resumed_results_keep_review_and_source_but_invalidate_computed_views() {
        let mut panel = ReviewPanel::default();
        panel.accept_run(run(), true);
        panel.selected = 3;
        panel.filter = "soup".into();
        panel.selected_ingredient = Some("old result".into());
        panel.evaluation = Some(serde_json::json!({"stale": true}));
        panel.decisions.insert(
            "chapter.xhtml".into(),
            ReviewDecision {
                status: "Accepted".into(),
                note: "Checked source".into(),
            },
        );
        panel.accept_run(run(), false);
        assert_eq!(panel.selected, 3);
        assert_eq!(panel.filter, "soup");
        assert_eq!(panel.decisions["chapter.xhtml"].status, "Accepted");
        assert!(panel.selected_ingredient.is_none());
        assert!(panel.evaluation.is_none());
        assert!(!panel.allow_network);
    }

    #[test]
    fn new_source_resets_selection_and_presentation() {
        let mut panel = ReviewPanel {
            selected: 9,
            filter: "old".into(),
            show_graph: true,
            missing_only: true,
            ..Default::default()
        };
        panel.accept_run(run(), true);
        assert_eq!(panel.selected, 0);
        assert!(panel.filter.is_empty());
        assert!(!panel.show_graph);
        assert!(!panel.missing_only);
    }

    #[test]
    fn saved_cli_run_and_review_sidecar_open_offline() {
        let directory = std::env::temp_dir().join(format!(
            "food-app-review-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("run.json");
        let run = run();
        run.save(&path).unwrap();
        let mut panel = ReviewPanel::default();
        panel.decisions.insert(
            "chapter.xhtml".into(),
            ReviewDecision {
                status: "Uncertain".into(),
                note: "Check image".into(),
            },
        );
        panel.save_decisions(&path, &run).unwrap();
        let mut reopened = ReviewPanel::default();
        reopened.open(path);
        assert!(reopened.error.is_none(), "{:?}", reopened.error);
        assert!(reopened.run.is_some());
        assert!(!reopened.busy());
        assert_eq!(reopened.decisions["chapter.xhtml"].note, "Check image");
        std::fs::remove_dir_all(directory).unwrap();
    }
}
