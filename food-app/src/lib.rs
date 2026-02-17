// UI code uses unwrap for display purposes where panics are acceptable
#![allow(clippy::unwrap_used)]

mod tabs;

use eframe::egui::{self, Image};
use ingredient::trace::ParseTrace;
use poll_promise::Promise;
use rand::RngExt;
use recipe_scraper::{ParsedRecipe, ScrapedRecipe};
use tabs::{show_debug_tab, show_parsed, show_raw, show_test_tab};

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Recipe,
    Debug,
    Test,
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
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
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
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Top panel with tab bar (always visible)
        egui::TopBottomPanel::top("tab_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.current_tab, Tab::Test, "🧪 Test Parser");
                ui.separator();
                ui.selectable_value(&mut self.current_tab, Tab::Recipe, "📖 Recipe");
                ui.selectable_value(&mut self.current_tab, Tab::Debug, "🔍 Debug Trace");
            });
        });

        // URL bar panel (only shown for Recipe/Debug tabs)
        if self.current_tab != Tab::Test {
            egui::TopBottomPanel::top("url_panel").show(ctx, |ui| {
                let trigger_fetch = ui_url(ui, &mut self.url);

                if trigger_fetch || self.promise.is_none() {
                    let ctx = ctx.clone();
                    let (sender, promise) = Promise::new();
                    let request = ehttp::Request::get(rewrite_url(&self.url.clone()));
                    ehttp::fetch(request, move |response| {
                        let recipe = response.and_then(parse_response);
                        if let Ok(r) = recipe {
                            let traces: Vec<ParseTrace> = r
                                .ingredients
                                .iter()
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

        egui::CentralPanel::default().show(ctx, |ui| match self.current_tab {
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
                                    // Display yield and servings if present
                                    if let Some(recipe_yield) = &w.recipe.recipe_yield {
                                        ui.label(format!(
                                            "📊 Yield: {} {}",
                                            recipe_yield.value, recipe_yield.unit
                                        ));
                                    }
                                    if let Some(servings) = &w.recipe.servings {
                                        ui.label(format!("🍽️ Servings: {servings}"));
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
