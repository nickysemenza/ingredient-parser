#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use eframe::{
    egui::{self, RichText},
    epaint::Color32,
};
use egui_extras::RetainedImage;
use poll_promise::Promise;
use rand::Rng;
use recipe_scraper::{ParsedRecipe, ScrapedRecipe};

fn main() {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "Download and show an image with eframe/egui",
        options,
        Box::new(|_cc| Box::new(MyApp::default())),
    );
}

struct Wrapper {
    recipe: ScrapedRecipe,
    parsed: ParsedRecipe,
    image: Option<RetainedImage>,
}

struct MyApp {
    /// `None` when download hasn't started yet.
    promise: Option<Promise<ehttp::Result<Wrapper>>>,
    url: String,
}

impl Default for MyApp {
    fn default() -> Self {
        Self {
            promise: None,
            url: "https://cooking.nytimes.com/recipes/1022674-chewy-gingerbread-cookies"
                .to_string(),
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
        let mut rng = rand::thread_rng();
        *url = format!(
            "https://cooking.nytimes.com/recipes/{}",
            rng.gen_range(10..15000)
        );
        trigger_fetch = true;
    }

    trigger_fetch
}

fn show_parsed(ui: &mut egui::Ui, parsed: &ParsedRecipe) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            parsed.ingredients.iter().for_each(|x| {
                // ui.label(x.to_string());
                ui.collapsing(x.to_string(), |ui| {
                    ui.label(serde_json::to_string_pretty(&x).unwrap())
                });
            });
        });
        ui.vertical(|ui| {
            parsed.instructions.iter().for_each(|x| {
                ui.horizontal_wrapped(|ui| {
                    ui.group(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        x.iter().for_each(|x| match x {
                            ingredient::rich_text::Chunk::Amount(x) => x.iter().for_each(|x| {
                                ui.label(RichText::new(x.to_string()).color(Color32::GOLD));
                            }),
                            ingredient::rich_text::Chunk::Text(t) => {
                                ui.label(t);
                            }
                            ingredient::rich_text::Chunk::Ing(i) => {
                                ui.label(RichText::new(i).color(Color32::LIGHT_BLUE));
                            }
                        });
                    });
                });
            });
        });
    });
}

fn show_raw(ui: &mut egui::Ui, recipe: &ScrapedRecipe) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            recipe.ingredients.iter().for_each(|x| {
                ui.label(x);
            });
        });
        ui.vertical(|ui| {
            recipe.instructions.iter().for_each(|x| {
                ui.label(x);
            });
        });
    });
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("my_panel").show(ctx, |ui| {
            let trigger_fetch = ui_url(ui, &mut self.url);

            if trigger_fetch || self.promise.is_none() {
                // Begin download.
                // We download the image using `ehttp`, a library that works both in WASM and on native.
                // We use the `poll-promise` library to communicate with the UI thread.
                let ctx = ctx.clone();
                let (sender, promise) = Promise::new();
                let request = ehttp::Request::get(self.url.clone());
                ehttp::fetch(request, move |response| {
                    let recipe = response.and_then(parse_response);
                    // sender.send(recipe); // send the results back to the UI thread.

                    if recipe.is_ok() {
                        let recipe = recipe.unwrap();
                        if recipe.image.is_some() {
                            let image_url = recipe.image.as_ref().unwrap();
                            let request = ehttp::Request::get(image_url);
                            ehttp::fetch(request, move |response| {
                                let image = response.and_then(parse_response_image).unwrap();
                                sender.send(Ok(Wrapper {
                                    recipe: recipe.clone(),
                                    parsed: recipe.parse(),
                                    image: Some(image),
                                }));
                            });
                        }
                    } else {
                        sender.send(Err(recipe.err().unwrap()));
                    }

                    ctx.request_repaint(); // wake up UI thread
                });

                self.promise = Some(promise);
            };
        });
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.separator();

            if let Some(promise) = &self.promise {
                match promise.ready() {
                    None => {
                        ui.spinner(); // still loading
                    }
                    Some(Err(err)) => {
                        ui.colored_label(ui.visuals().error_fg_color, err); // something went wrong
                    }
                    Some(Ok(w)) => {
                        ui.horizontal(|ui| {
                            ui.set_min_height(200.0);
                            ui.heading(w.recipe.name.clone());
                            w.image
                                .as_ref()
                                .unwrap()
                                .show_max_size(ui, ui.available_size());
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
        });
    }
}

#[allow(clippy::needless_pass_by_value)]
fn parse_response(response: ehttp::Response) -> Result<ScrapedRecipe, String> {
    match recipe_scraper::scrape(&response.text().unwrap(), &response.url) {
        Ok(r) => Ok(r),
        Err(x) => Err(format!("failed to get recipe {:?}", x)),
    }
}

#[allow(clippy::needless_pass_by_value)]
fn parse_response_image(response: ehttp::Response) -> Result<RetainedImage, String> {
    let content_type = response.content_type().unwrap_or_default();
    if content_type.starts_with("image/") {
        RetainedImage::from_image_bytes(&response.url, &response.bytes)
    } else {
        Err(format!(
            "Expected image, found content-type {:?}",
            content_type
        ))
    }
}
