use eframe::egui::{self, RichText, TextFormat, WidgetText};
use eframe::epaint::Color32;
use recipe_scraper::{ParsedRecipe, ParsedSection, ScrapedRecipe};
use std::sync::Arc;

use eframe::epaint::text::LayoutJob;

fn make_rich(i: &ingredient::ingredient::Ingredient) -> WidgetText {
    let amounts: Vec<String> = i.amounts.iter().map(|id| id.to_string()).collect();
    let modifier = i
        .modifier
        .as_ref()
        .map_or_else(String::new, |m| format!(", {m}"));
    let amount_list = if amounts.is_empty() {
        "n/a ".to_string()
    } else {
        format!("{} ", amounts.join(" / "))
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
    show_parsed_sections(ui, &parsed.sections);
}

/// Render parsed recipe sections (ingredients with color-coded amounts +
/// measurement-aware instructions). Shared by the web Recipe tab and the
/// Cookbook (EPUB) tab.
pub fn show_parsed_sections(ui: &mut egui::Ui, sections: &[ParsedSection]) {
    for section in sections {
        if let Some(name) = &section.name {
            ui.heading(name);
        }
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                section.ingredients.iter().for_each(|x| {
                    ui.collapsing(make_rich(x), |ui| {
                        ui.label(serde_json::to_string_pretty(&x).unwrap())
                    });
                });
            });
            ui.vertical(|ui| {
                section.instructions.iter().for_each(|x| {
                    ui.horizontal_wrapped(|ui| {
                        ui.group(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            x.iter().for_each(|x| match x {
                                ingredient::rich_text::Chunk::Measure(x) => {
                                    x.iter().for_each(|x| {
                                        ui.label(RichText::new(x.to_string()).color(Color32::GOLD));
                                    })
                                }
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
}

pub fn show_raw(ui: &mut egui::Ui, recipe: &ScrapedRecipe) {
    // Show yield/servings metadata in raw view too
    ui.horizontal(|ui| {
        if let Some(recipe_yield) = &recipe.recipe_yield {
            ui.label(
                RichText::new(format!(
                    "Yield: {} {}",
                    recipe_yield.value, recipe_yield.unit
                ))
                .color(Color32::GOLD),
            );
            ui.separator();
        }
        if let Some(servings) = &recipe.servings {
            ui.label(RichText::new(format!("Servings: {servings}")).color(Color32::GOLD));
        }
    });
    ui.separator();
    for section in &recipe.sections {
        if let Some(name) = &section.name {
            ui.heading(name);
        }
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                section.ingredients.iter().for_each(|x| {
                    ui.label(x);
                });
            });
            ui.vertical(|ui| {
                section.instructions.iter().for_each(|x| {
                    ui.label(x);
                });
            });
        });
    }
}
