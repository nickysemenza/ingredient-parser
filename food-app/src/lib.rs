//! Native maintainer workspaces for parser inspection and cookbook review.
//!
//! THESIS: keep a source, its result, and the evidence for that result together.
//! OWN-WORLD: compact system typography, neutral split panes, semantic color.
//! STORY: choose Parser or Cookbooks, load a source explicitly, inspect evidence.
//! FIRST VIEWPORT: navigation rail, source toolbar, working area and inspector.
//! FORM: user-approved native developer tools, Xcode and Instruments references.
//! FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, and DESIGN.md
#![allow(clippy::unwrap_used)]

mod persist;
mod tabs;
mod theme;

use eframe::egui::{self, RichText};
use ingredient::trace::ParseTrace;
use poll_promise::Promise;
use recipe_scraper::{ParsedRecipe, ScrapedRecipe};
use tabs::{CookbookTab, CorpusAction, CorpusTab, TestTab};
use tabs::{show_debug_tab, show_parsed, show_raw};

#[derive(PartialEq, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
enum Workspace {
    #[default]
    Parser,
    Cookbooks,
}

#[derive(PartialEq, Clone, Copy, Default, serde::Serialize, serde::Deserialize)]
enum ParserSource {
    #[default]
    Ingredients,
    Recipe,
    Corpus,
}

struct Wrapper {
    recipe: ScrapedRecipe,
    parsed: ParsedRecipe,
    traces: Vec<ParseTrace>,
}

pub struct MyApp {
    promise: Option<Promise<ehttp::Result<Wrapper>>>,
    url: String,
    workspace: Workspace,
    parser_source: ParserSource,
    theme: theme::ThemeChoice,
    selected_ingredient_idx: Option<usize>,
    recipe_inspect: bool,
    test: TestTab,
    cookbook: CookbookTab,
    corpus: CorpusTab,
}

impl Default for MyApp {
    fn default() -> Self {
        Self {
            promise: None,
            url: String::new(),
            workspace: Workspace::Parser,
            parser_source: ParserSource::Ingredients,
            theme: theme::ThemeChoice::default(),
            selected_ingredient_idx: None,
            recipe_inspect: true,
            test: TestTab::default(),
            cookbook: CookbookTab::default(),
            corpus: CorpusTab::default(),
        }
    }
}

impl MyApp {
    pub fn open_review(&mut self, path: std::path::PathBuf) {
        self.cookbook.open_review(path);
        self.workspace = Workspace::Cookbooks;
    }

    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut app = Self::default();
        if let Some(storage) = cc.storage
            && let Some(state) =
                eframe::get_value::<persist::PersistedState>(storage, eframe::APP_KEY)
        {
            state.apply_to(&mut app);
        }
        theme::apply(&cc.egui_ctx, app.theme);
        app
    }

    fn fetch_recipe(&mut self, ctx: &egui::Context) {
        let ctx = ctx.clone();
        let (sender, promise) = Promise::new();
        let request = ehttp::Request::get(self.url.trim());
        ehttp::fetch(request, move |response| {
            let result = response.and_then(parse_response).map(|recipe| {
                let parser = ingredient::IngredientParser::new();
                let execution = recipe_parsing::execute_sections(
                    &recipe.sections,
                    &parser,
                    ingredient::ParseOptions {
                        decomposition: false,
                        trace: ingredient::TraceDetail::Full,
                    },
                );
                for diagnostic in &execution.instruction_diagnostics {
                    tracing::warn!(
                        section = diagnostic.section,
                        instruction = diagnostic.instruction,
                        "{}",
                        diagnostic.message
                    );
                }
                Wrapper {
                    recipe,
                    parsed: execution.recipe,
                    traces: execution
                        .observations
                        .into_iter()
                        .flatten()
                        .filter_map(|observation| observation.trace)
                        .collect(),
                }
            });
            sender.send(result);
            ctx.request_repaint();
        });
        self.promise = Some(promise);
        self.selected_ingredient_idx = None;
    }

    fn show_recipe(&mut self, ui: &mut egui::Ui) {
        let pending = self.promise.as_ref().is_some_and(|p| p.ready().is_none());
        let mut load = false;
        ui.horizontal(|ui| {
            ui.label("Recipe URL");
            let available = (ui.available_width() - 85.0).max(80.0);
            let response = ui.add(
                egui::TextEdit::singleline(&mut self.url)
                    .desired_width(available)
                    .hint_text("https://…"),
            );
            load |= response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            load |= ui
                .add_enabled(
                    !pending && !self.url.trim().is_empty(),
                    egui::Button::new("Load recipe"),
                )
                .clicked();
        });
        if load && !pending && !self.url.trim().is_empty() {
            self.fetch_recipe(ui.ctx());
        }
        ui.separator();
        let Some(promise) = &self.promise else {
            ui.add_space(24.0);
            ui.heading("Inspect a web recipe");
            ui.label("Load a URL to inspect its ingredient results and source recipe.");
            return;
        };
        match promise.ready() {
            None => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Loading recipe…");
                });
            }
            Some(Err(error)) => {
                ui.colored_label(ui.visuals().error_fg_color, error);
                ui.label("Check the URL and connection, then choose Load recipe to retry.");
            }
            Some(Ok(w)) => {
                ui.horizontal_wrapped(|ui| {
                    ui.heading(&w.recipe.name);
                    ui.separator();
                    ui.selectable_value(&mut self.recipe_inspect, true, "Ingredients");
                    ui.selectable_value(&mut self.recipe_inspect, false, "Recipe & source");
                    if !w.recipe.url.is_empty() {
                        ui.hyperlink_to("Open source", &w.recipe.url);
                    }
                });
                ui.separator();
                if self.recipe_inspect {
                    show_debug_tab(ui, &w.traces, &mut self.selected_ingredient_idx);
                } else {
                    egui::ScrollArea::vertical()
                        .id_salt("recipe_content")
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                if let Some(category) = &w.recipe.category {
                                    ui.label(category);
                                }
                                if let Some(yield_) = &w.recipe.recipe_yield {
                                    ui.label(format!(
                                        "{} {} {}",
                                        theme::icon::YIELD,
                                        yield_.value,
                                        yield_.unit
                                    ));
                                }
                                if let Some(servings) = &w.recipe.servings {
                                    ui.label(format!(
                                        "{} {servings} servings",
                                        theme::icon::SERVINGS
                                    ));
                                }
                                if let Some(times) = &w.recipe.times {
                                    for (label, value) in [
                                        ("active", &times.active),
                                        ("total", &times.total),
                                        ("prep", &times.prep),
                                        ("cook", &times.cook),
                                    ] {
                                        if let Some(value) = value {
                                            ui.label(format!(
                                                "{} {label}: {value}",
                                                theme::icon::TIME
                                            ));
                                        }
                                    }
                                }
                            });
                            if let Some(description) = &w.recipe.description {
                                ui.label(description);
                            }
                            if let Some(image) = &w.recipe.image {
                                ui.add(egui::Image::from_uri(image).max_height(180.0));
                            }
                            show_parsed(ui, &w.parsed);
                            ui.collapsing("Scraped source", |ui| show_raw(ui, &w.recipe));
                        });
                }
            }
        }
    }
}

impl eframe::App for MyApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(
            storage,
            eframe::APP_KEY,
            &persist::PersistedState::capture(self),
        );
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show(ui);
    }
}

impl MyApp {
    fn show(&mut self, ui: &mut egui::Ui) {
        egui::Panel::left("workspace_rail_v2")
            .resizable(false)
            .exact_size(176.0)
            .frame(theme::sidebar_frame())
            .show(ui, |ui| {
                ui.add_space(12.0);
                ui.label(RichText::new("ingredient-parser").size(14.0).strong());
                ui.add_space(24.0);
                for (workspace, icon, label) in [
                    (Workspace::Parser, theme::icon::TEST, "Parser"),
                    (Workspace::Cookbooks, theme::icon::COOKBOOK, "Cookbooks"),
                ] {
                    if ui
                        .add_sized(
                            [ui.available_width(), 36.0],
                            egui::Button::new(format!("{icon}  {label}"))
                                .right_text(())
                                .selected(self.workspace == workspace)
                                .frame_when_inactive(self.workspace == workspace),
                        )
                        .clicked()
                    {
                        self.workspace = workspace;
                    }
                    ui.add_space(4.0);
                }
                ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                    let label = match self.theme {
                        theme::ThemeChoice::Mocha => "Light appearance",
                        theme::ThemeChoice::Latte => "Dark appearance",
                    };
                    if ui.button(label).clicked() {
                        self.theme = match self.theme {
                            theme::ThemeChoice::Mocha => theme::ThemeChoice::Latte,
                            theme::ThemeChoice::Latte => theme::ThemeChoice::Mocha,
                        };
                        theme::set_theme(ui.ctx(), self.theme);
                        self.cookbook.invalidate_graph();
                    }
                });
            });
        if self.workspace == Workspace::Parser {
            egui::Panel::top("workspace_toolbar_v2")
                .frame(theme::workspace_frame())
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new("Parser").size(24.0).strong());
                        ui.add_space(24.0);
                        ui.selectable_value(
                            &mut self.parser_source,
                            ParserSource::Ingredients,
                            "Ingredients",
                        );
                        ui.selectable_value(
                            &mut self.parser_source,
                            ParserSource::Recipe,
                            "Recipe URL",
                        );
                        ui.selectable_value(
                            &mut self.parser_source,
                            ParserSource::Corpus,
                            "Corpus",
                        );
                    });
                });
        }
        egui::CentralPanel::default()
            .frame(theme::workspace_frame())
            .show(ui, |ui| match self.workspace {
                Workspace::Cookbooks => self.cookbook.show(ui),
                Workspace::Parser => match self.parser_source {
                    ParserSource::Ingredients => self.test.show(ui),
                    ParserSource::Recipe => self.show_recipe(ui),
                    ParserSource::Corpus => {
                        if let Some(CorpusAction::SendToTest(input)) = self.corpus.show(ui) {
                            self.test.set_input(input);
                            self.parser_source = ParserSource::Ingredients;
                        }
                    }
                },
            });
    }
}

#[allow(clippy::needless_pass_by_value)]
fn parse_response(response: ehttp::Response) -> Result<ScrapedRecipe, String> {
    if !response.ok {
        return Err(format!(
            "HTTP {} {} from {}",
            response.status, response.status_text, response.url
        ));
    }
    // The URL is arbitrary user input: a binary body (image, PDF) has no UTF-8
    // text, which must surface as an error, not a panic in the fetch callback.
    let Some(text) = response.text() else {
        return Err(format!("non-text response from {}", response.url));
    };
    match recipe_scraper::scrape(text, &response.url) {
        Ok(r) => Ok(r),
        Err(x) => Err(format!("failed to get recipe {x:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn response(url: &str, bytes: Vec<u8>) -> ehttp::Response {
        ehttp::Response {
            url: url.to_string(),
            ok: true,
            status: 200,
            status_text: "OK".to_string(),
            headers: ehttp::Headers::default(),
            bytes,
        }
    }

    #[test]
    fn http_failure_does_not_parse_error_page_as_recipe() {
        let mut resp = response("https://example.com/missing", b"error".to_vec());
        resp.ok = false;
        resp.status = 404;
        resp.status_text = "Not Found".to_owned();
        assert!(parse_response(resp).unwrap_err().contains("HTTP 404"));
    }

    /// A body with valid ld+json recipe markup parses through to `Ok`.
    #[test]
    fn parse_response_success() {
        let html = r#"<html><head><script type="application/ld+json">
            {"name": "Chocolate Cake", "recipeIngredient": ["2 cups flour", "1 cup sugar"], "recipeInstructions": []}
        </script></head><body></body></html>"#;
        let resp = response("https://example.com/cake", html.as_bytes().to_vec());

        let recipe = parse_response(resp).unwrap();
        assert_eq!(recipe.name, "Chocolate Cake");
        assert_eq!(recipe.ingredients().count(), 2);
    }

    /// Non-UTF-8 bytes (e.g. an image mistakenly fetched as the URL) must
    /// surface as an `Err`, not panic — this is the doc comment's must-not-panic
    /// path (`response.text()` returning `None`).
    #[test]
    fn parse_response_non_utf8_body_is_error() {
        let resp = response("https://example.com/photo.jpg", vec![0xFF, 0xFE, 0x00]);

        let err = parse_response(resp).unwrap_err();
        assert!(err.contains("non-text response"));
    }

    /// Valid UTF-8 that has no recognizable recipe content still surfaces as
    /// an `Err` from `recipe_scraper::scrape`, not a panic.
    #[test]
    fn parse_response_unscrapable_body_is_error() {
        let resp = response(
            "https://example.com/not-a-recipe",
            b"<html><body>hello</body></html>".to_vec(),
        );

        let err = parse_response(resp).unwrap_err();
        assert!(err.contains("failed to get recipe"));
    }
    #[test]
    fn workspaces_render_at_desktop_sizes_without_starting_a_fetch() {
        for size in [[800.0, 560.0], [1280.0, 820.0]] {
            let ctx = egui::Context::default();
            let mut app = MyApp::default();
            for source in [
                ParserSource::Ingredients,
                ParserSource::Recipe,
                ParserSource::Corpus,
            ] {
                app.parser_source = source;
                let _ = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size.into())),
                        ..Default::default()
                    },
                    |ui| app.show(ui),
                );
                assert!(app.promise.is_none());
            }
            app.workspace = Workspace::Cookbooks;
            let _ = ctx.run_ui(egui::RawInput::default(), |ui| app.show(ui));
            assert!(app.promise.is_none());
        }
    }
}
