use eframe::egui::{self, RichText, TextFormat, WidgetText};
use eframe::epaint::Color32;
use recipe_scraper::{ParsedRecipe, ScrapedRecipe};
use std::sync::Arc;

use eframe::epaint::text::LayoutJob;

fn make_rich(i: &ingredient::ingredient::Ingredient) -> WidgetText {
    let amounts: Vec<String> = i.amounts.iter().map(|id| id.to_string()).collect();
    let modifier = match i.modifier.clone() {
        Some(m) => {
            format!(", {m}")
        }
        None => "".to_string(),
    };
    let amount_list = match amounts.len() {
        0 => "n/a ".to_string(),
        _ => format!("{} ", amounts.join(" / ")),
    };
    let name = i.name.clone();

    let mut job = LayoutJob::default();
    job.append(
        amount_list.as_str(),
        0.0,
        TextFormat {
            color: Color32::GOLD,
            ..Default::default()
        },
    );
    job.append(
        name.as_str(),
        0.0,
        TextFormat {
            color: Color32::LIGHT_BLUE,
            ..Default::default()
        },
    );
    job.append(
        modifier.as_str(),
        0.0,
        TextFormat {
            color: Color32::LIGHT_GRAY,
            ..Default::default()
        },
    );
    WidgetText::LayoutJob(Arc::new(job))
}

pub fn show_parsed(ui: &mut egui::Ui, parsed: &ParsedRecipe) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            parsed.ingredients.iter().for_each(|x| {
                ui.collapsing(make_rich(x), |ui| {
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
                            ingredient::rich_text::Chunk::Measure(x) => x.iter().for_each(|x| {
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

pub fn show_raw(ui: &mut egui::Ui, recipe: &ScrapedRecipe) {
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
