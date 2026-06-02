// UI code uses unwrap for display purposes where panics are acceptable
#![allow(clippy::unwrap_used)]

mod tabs;
mod theme;

use eframe::egui::{self, Image, RichText};
use ingredient::trace::ParseTrace;
use poll_promise::Promise;
use rand::RngExt;
use recipe_scraper::{ParsedRecipe, ScrapedRecipe};
#[cfg(not(target_arch = "wasm32"))]
use tabs::CookbookTab;
use tabs::{show_debug_tab, show_parsed, show_raw, show_test_tab};

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Recipe,
    Debug,
    Test,
    #[cfg(not(target_arch = "wasm32"))]
    Cookbook,
}

struct Wrapper {
    recipe: ScrapedRecipe,
    parsed: ParsedRecipe,
    traces: Vec<ParseTrace>,
}

pub struct MyApp {
    /// `None` when download hasn't started yet.
    promise: Option<Promise<ehttp::Result<Wrapper>>>,
    url: String,
    current_tab: Tab,
    selected_ingredient_idx: Option<usize>,
    // Test tab state
    test_input: String,
    test_trace: Option<ParseTrace>,
    test_result: Option<ingredient::ingredient::Ingredient>,
    // Cookbook (EPUB) tab state — native only
    #[cfg(not(target_arch = "wasm32"))]
    cookbook: CookbookTab,
}

impl Default for MyApp {
    fn default() -> Self {
        Self {
            promise: None,
            url: "https://cooking.nytimes.com/recipes/1022674-chewy-gingerbread-cookies"
                .to_string(),
            current_tab: Tab::Recipe,
            selected_ingredient_idx: None,
            test_input: "2 cups all-purpose flour, sifted".to_string(),
            test_trace: None,
            test_result: None,
            #[cfg(not(target_arch = "wasm32"))]
            cookbook: CookbookTab::default(),
        }
    }
}

fn ui_url(ui: &mut egui::Ui, url: &mut String) -> bool {
    let mut trigger_fetch = false;

    ui.horizontal(|ui| {
        ui.label("URL:");
        trigger_fetch |= ui
            .add(egui::TextEdit::singleline(url).desired_width(f32::INFINITY))
            .lost_focus();
    });
    if ui.button("Random NYT").clicked() {
        let mut rng = rand::rng();
        *url = format!(
            "https://cooking.nytimes.com/recipes/{}",
            rng.random_range(10..15000)
        );
        trigger_fetch = true;
    }

    trigger_fetch
}

impl MyApp {
    /// Called once before the first frame.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        Default::default()
    }
}
#[cfg(target_arch = "wasm32")]
fn rewrite_url(url: &str) -> String {
    format!("https://cors.nicky.workers.dev/?target={}", url)
}
#[cfg(not(target_arch = "wasm32"))]
fn rewrite_url(url: &str) -> String {
    url.to_string()
}

impl eframe::App for MyApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Top panel with tab bar (always visible)
        egui::Panel::top("tab_bar").show_inside(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(
                    &mut self.current_tab,
                    Tab::Test,
                    format!("{} Test Parser", theme::icon::TEST),
                );
                ui.separator();
                ui.selectable_value(
                    &mut self.current_tab,
                    Tab::Recipe,
                    format!("{} Recipe", theme::icon::RECIPE),
                );
                ui.selectable_value(
                    &mut self.current_tab,
                    Tab::Debug,
                    format!("{} Debug Trace", theme::icon::DEBUG),
                );
                #[cfg(not(target_arch = "wasm32"))]
                ui.selectable_value(
                    &mut self.current_tab,
                    Tab::Cookbook,
                    format!("{} Cookbook", theme::icon::COOKBOOK),
                );
            });
        });

        // URL bar panel (only shown for the web-scraping Recipe/Debug tabs)
        if matches!(self.current_tab, Tab::Recipe | Tab::Debug) {
            egui::Panel::top("url_panel").show_inside(ui, |ui| {
                let trigger_fetch = ui_url(ui, &mut self.url);

                if trigger_fetch || self.promise.is_none() {
                    let ctx = ui.ctx().clone();
                    let (sender, promise) = Promise::new();
                    let request = ehttp::Request::get(rewrite_url(&self.url.clone()));
                    ehttp::fetch(request, move |response| {
                        let recipe = response.and_then(parse_response);
                        if let Ok(r) = recipe {
                            let traces: Vec<ParseTrace> = r
                                .ingredients()
                                .map(|ing| {
                                    let parser = ingredient::IngredientParser::new();
                                    parser.parse_with_trace(ing).trace
                                })
                                .collect();

                            sender.send(Ok(Wrapper {
                                recipe: r.clone(),
                                parsed: r.parse(),
                                traces,
                            }));
                        } else {
                            sender.send(Err(recipe.err().unwrap()));
                        }
                        ctx.request_repaint();
                    });
                    self.promise = Some(promise);
                };
            });
        }

        egui::CentralPanel::default().show_inside(ui, |ui| match self.current_tab {
            Tab::Test => {
                show_test_tab(
                    ui,
                    &mut self.test_input,
                    &mut self.test_trace,
                    &mut self.test_result,
                );
            }
            Tab::Recipe => {
                if let Some(promise) = &self.promise {
                    match promise.ready() {
                        None => {
                            ui.spinner();
                        }
                        Some(Err(err)) => {
                            ui.colored_label(ui.visuals().error_fg_color, err);
                        }
                        Some(Ok(w)) => {
                            ui.horizontal(|ui| {
                                ui.set_min_height(200.0);
                                ui.vertical(|ui| {
                                    ui.heading(w.recipe.name.clone());
                                    if let Some(category) = &w.recipe.category {
                                        ui.label(RichText::new(category).italics().weak());
                                    }
                                    // Display yield, servings, and times if present.
                                    ui.horizontal_wrapped(|ui| {
                                        if let Some(recipe_yield) = &w.recipe.recipe_yield {
                                            ui.label(
                                                RichText::new(format!(
                                                    "{} Yield: {} {}",
                                                    theme::icon::YIELD,
                                                    recipe_yield.value,
                                                    recipe_yield.unit
                                                ))
                                                .color(theme::AMOUNT),
                                            );
                                        }
                                        if let Some(servings) = &w.recipe.servings {
                                            ui.label(
                                                RichText::new(format!(
                                                    "{} Servings: {servings}",
                                                    theme::icon::SERVINGS
                                                ))
                                                .color(theme::AMOUNT),
                                            );
                                        }
                                        if let Some(t) = &w.recipe.times {
                                            for (label, value) in [
                                                ("active", &t.active),
                                                ("total", &t.total),
                                                ("prep", &t.prep),
                                                ("cook", &t.cook),
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
                                        ui.add_space(4.0);
                                        ui.label(RichText::new(description).italics());
                                    }
                                    if !w.recipe.url.is_empty() {
                                        ui.hyperlink(&w.recipe.url);
                                    }
                                });
                                if let Some(image) = &w.recipe.image {
                                    ui.add(Image::from_uri(image));
                                }
                            });
                            ui.separator();
                            egui::ScrollArea::vertical().show(ui, |ui| {
                                show_parsed(ui, &w.parsed);
                                ui.separator();
                                show_raw(ui, &w.recipe);
                            });
                        }
                    }
                }
            }
            Tab::Debug => {
                if let Some(promise) = &self.promise {
                    match promise.ready() {
                        None => {
                            ui.spinner();
                        }
                        Some(Err(err)) => {
                            ui.colored_label(ui.visuals().error_fg_color, err);
                        }
                        Some(Ok(w)) => {
                            show_debug_tab(ui, &w.traces, &mut self.selected_ingredient_idx);
                        }
                    }
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            Tab::Cookbook => {
                self.cookbook.show(ui);
            }
        });
    }
}

#[allow(clippy::needless_pass_by_value)]
fn parse_response(response: ehttp::Response) -> Result<ScrapedRecipe, String> {
    match recipe_scraper::scrape(response.text().unwrap(), &response.url) {
        Ok(r) => Ok(r),
        Err(x) => Err(format!("failed to get recipe {x:?}")),
    }
}
