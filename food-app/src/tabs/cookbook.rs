//! Cookbook tab: load a local EPUB cookbook and browse its AI-extracted
//! recipes. Native-only — `recipe-epub` pulls file/network/tokio deps that
//! don't build for wasm, so the whole module is gated out of the web build.

use eframe::egui::{self, Color32, RichText};
use poll_promise::Promise;
use recipe_epub::{CookbookRecipe, ExtractionStats, Options};

use super::recipe::show_parsed_sections;

type LoadResult = Result<(Vec<CookbookRecipe>, ExtractionStats), String>;

/// State for the Cookbook (EPUB) tab.
#[derive(Default)]
pub struct CookbookTab {
    path: String,
    no_cache: bool,
    /// `None` until a load is started.
    promise: Option<Promise<LoadResult>>,
    selected: usize,
}

impl CookbookTab {
    pub fn show(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("EPUB:");
            ui.add(
                egui::TextEdit::singleline(&mut self.path)
                    .hint_text("/path/to/cookbook.epub")
                    .desired_width(440.0),
            );
            ui.checkbox(&mut self.no_cache, "No cache");
            let can_load = !self.path.trim().is_empty();
            if ui
                .add_enabled(can_load, egui::Button::new("Load"))
                .clicked()
            {
                self.start_load(ui.ctx().clone());
            }
        });
        ui.label(
            RichText::new(
                "Reads ANTHROPIC_BASE_URL / AI_GATEWAY_API_KEY / ANTHROPIC_API_KEY from the \
                 environment. First load runs LLM extraction (cached afterwards).",
            )
            .weak()
            .small(),
        );
        ui.separator();

        let Some(promise) = &self.promise else {
            ui.label("Enter the path to a .epub file and click Load.");
            return;
        };

        match promise.ready() {
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Extracting recipes…");
                });
            }
            Some(Err(err)) => {
                ui.colored_label(ui.visuals().error_fg_color, err);
            }
            Some(Ok((recipes, _))) if recipes.is_empty() => {
                ui.label("No recipes found in this EPUB.");
            }
            Some(Ok((recipes, stats))) => {
                // Disjoint field borrows: `recipes` from self.promise (shared),
                // `selected` from self.selected (mut). Bind both before any
                // closure so neither closure captures all of `self`.
                let selected = &mut self.selected;
                if *selected >= recipes.len() {
                    *selected = 0;
                }

                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("{} recipes", recipes.len())).weak());
                    ui.separator();
                    let cost = stats
                        .cost_usd()
                        .map_or_else(|| "cost n/a".to_string(), |c| format!("~${c:.4}"));
                    ui.label(
                        RichText::new(format!(
                            "{cost} · {}/{} chunks cached",
                            stats.chunks_cached, stats.chunks_total
                        ))
                        .weak()
                        .small(),
                    )
                    .on_hover_text(format!(
                        "model: {}\n{} input tok\n{} output tok\n{} cache-read tok\n{} cache-write tok",
                        stats.model,
                        stats.usage.input_tokens,
                        stats.usage.output_tokens,
                        stats.usage.cache_read_input_tokens,
                        stats.usage.cache_creation_input_tokens,
                    ));
                });
                egui::Panel::left("cookbook_list")
                    .resizable(true)
                    .default_size(240.0)
                    .show_inside(ui, |ui| {
                        egui::ScrollArea::vertical().show(ui, |ui| {
                            for (i, r) in recipes.iter().enumerate() {
                                ui.selectable_value(selected, i, &r.meta.title);
                            }
                        });
                    });
                egui::CentralPanel::default().show_inside(ui, |ui| {
                    show_recipe_detail(ui, &recipes[*selected]);
                });
            }
        }
    }

    fn start_load(&mut self, ctx: egui::Context) {
        let path = self.path.trim().to_string();
        let no_cache = self.no_cache;
        self.selected = 0;
        self.promise = Some(Promise::spawn_thread("scrape_epub", move || {
            let result = (|| -> LoadResult {
                let bytes = std::fs::read(&path).map_err(|e| format!("read {path}: {e}"))?;
                let rt = tokio::runtime::Builder::new_multi_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| format!("tokio runtime: {e}"))?;
                let opts = Options {
                    use_cache: !no_cache,
                    ..Default::default()
                };
                rt.block_on(recipe_epub::extract_cookbook(&bytes, &path, &opts))
                    .map_err(|e| e.to_string())
            })();
            // Wake the UI thread when extraction finishes (poll-promise doesn't
            // repaint on its own).
            ctx.request_repaint();
            result
        }));
    }
}

fn show_recipe_detail(ui: &mut egui::Ui, r: &CookbookRecipe) {
    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.heading(&r.meta.title);
        if let Some(category) = &r.meta.category {
            ui.label(RichText::new(category).italics().weak());
        }

        ui.horizontal_wrapped(|ui| {
            if let Some(y) = &r.meta.recipe_yield {
                ui.label(RichText::new(format!("📊 {y}")).color(Color32::GOLD));
            }
            if let Some(t) = &r.meta.times {
                for (label, value) in [
                    ("active", &t.active),
                    ("total", &t.total),
                    ("prep", &t.prep),
                    ("cook", &t.cook),
                ] {
                    if let Some(value) = value {
                        ui.label(format!("⏱ {label}: {value}"));
                    }
                }
            }
        });

        if let Some(description) = &r.meta.description {
            ui.add_space(4.0);
            ui.label(RichText::new(description).italics());
        }

        ui.separator();
        // Parse this recipe's verbatim lines with the core nom parser for the
        // color-coded view (cheap — one recipe per frame).
        let parsed = r.parse();
        show_parsed_sections(ui, &parsed.sections);

        if !r.meta.equipment.is_empty() || !r.meta.notes.is_empty() {
            ui.separator();
            for e in &r.meta.equipment {
                ui.label(format!("🔧 {e}"));
            }
            for n in &r.meta.notes {
                ui.label(RichText::new(format!("📝 {n}")).weak());
            }
        }

        ui.add_space(8.0);
        ui.label(RichText::new(&r.url).weak().small());
    });
}
