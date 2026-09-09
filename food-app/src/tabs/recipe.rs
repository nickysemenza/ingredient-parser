use crate::theme;
use eframe::egui::{self, RichText, TextFormat, WidgetText};
use recipe_scraper::{ParsedRecipe, ParsedSection, ScrapedRecipe};
use std::sync::Arc;

use eframe::epaint::text::LayoutJob;

pub(crate) fn make_rich(i: &ingredient::ingredient::Ingredient) -> WidgetText {
    make_rich_scaled(i, 1.0)
}

/// The recipe views share this display path so cookbook scaling applies to every
/// authored alternative (for example, cup and gram), without changing the
/// parsed source object shown in the disclosure below.
pub(crate) fn make_rich_scaled(i: &ingredient::ingredient::Ingredient, scale: f64) -> WidgetText {
    let amounts: Vec<String> = i
        .amounts
        .iter()
        .map(|amount| amount.scale(scale).to_string())
        .collect();
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
            color: theme::palette().amount(),
            ..Default::default()
        },
    );
    job.append(
        name.as_str(),
        0.0,
        TextFormat {
            color: theme::palette().name(),
            ..Default::default()
        },
    );
    job.append(
        modifier.as_str(),
        0.0,
        TextFormat {
            color: theme::palette().modifier(),
            ..Default::default()
        },
    );
    WidgetText::LayoutJob(Arc::new(job))
}

pub fn show_parsed(ui: &mut egui::Ui, parsed: &ParsedRecipe) {
    show_parsed_sections(ui, &parsed.sections);
}

/// A parsed ingredient as a collapsible row: the color-coded one-liner, with
/// the raw JSON underneath. Shared by the web Recipe tab and the Cookbook
/// (EPUB) tab, which adds a cross-recipe link next to it.
pub(crate) fn show_ingredient_collapsing(
    ui: &mut egui::Ui,
    i: &ingredient::ingredient::Ingredient,
) {
    ui.collapsing(make_rich(i), |ui| {
        ui.label(serde_json::to_string_pretty(&i).unwrap())
    });
}

/// One instruction line as a card of measurement-aware chunks (amounts and
/// ingredient names color-coded). Shared by the web Recipe tab and the
/// Cookbook (EPUB) tab.
pub(crate) fn show_instruction_chunks(ui: &mut egui::Ui, chunks: &[ingredient::rich_text::Chunk]) {
    show_instruction_chunks_scaled(ui, chunks, 1.0);
}

/// Render instruction chunks with the same scaling semantics as ingredients.
/// `Measure::scale` deliberately leaves dimensions, temperatures, and times
/// alone while scaling every alternative quantity in a measure chunk.
pub(crate) fn show_instruction_chunks_scaled(
    ui: &mut egui::Ui,
    chunks: &[ingredient::rich_text::Chunk],
    scale: f64,
) {
    ui.horizontal_wrapped(|ui| {
        theme::card(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            for chunk in chunks {
                match chunk {
                    ingredient::rich_text::Chunk::Measure(ms) => {
                        for m in ms {
                            ui.label(
                                RichText::new(m.scale(scale).to_string())
                                    .color(theme::palette().amount()),
                            );
                        }
                    }
                    ingredient::rich_text::Chunk::Text(t) => {
                        ui.label(t);
                    }
                    ingredient::rich_text::Chunk::Ing(i) => {
                        ui.label(RichText::new(i).color(theme::palette().name()));
                    }
                }
            }
        });
    });
}

/// Below this width the ingredient/instruction columns stack vertically
/// instead of cramping side by side.
const STACK_BREAKPOINT: f32 = 640.0;

/// Lay out a section's ingredients and instructions: two columns when there's
/// room, stacked (with sub-headings) below [`STACK_BREAKPOINT`].
fn section_columns(
    ui: &mut egui::Ui,
    ingredients: impl FnOnce(&mut egui::Ui),
    instructions: impl FnOnce(&mut egui::Ui),
) {
    if ui.available_width() < STACK_BREAKPOINT {
        ui.label(RichText::new("Ingredients").strong());
        ingredients(ui);
        ui.add_space(4.0);
        ui.label(RichText::new("Instructions").strong());
        instructions(ui);
    } else {
        ui.columns(2, |columns| {
            ingredients(&mut columns[0]);
            instructions(&mut columns[1]);
        });
    }
}

/// Render parsed recipe sections (ingredients with color-coded amounts +
/// measurement-aware instructions) for the web Recipe tab.
fn show_parsed_sections(ui: &mut egui::Ui, sections: &[ParsedSection]) {
    for section in sections {
        if let Some(name) = &section.name {
            ui.heading(name);
        }
        section_columns(
            ui,
            |ui| {
                section.ingredients.iter().for_each(|x| {
                    show_ingredient_collapsing(ui, x);
                });
            },
            |ui| {
                section.instructions.iter().for_each(|x| {
                    show_instruction_chunks(ui, x);
                });
            },
        );
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
                .color(theme::palette().amount()),
            );
            ui.separator();
        }
        if let Some(servings) = &recipe.servings {
            ui.label(
                RichText::new(format!("Servings: {servings}")).color(theme::palette().amount()),
            );
        }
    });
    ui.separator();
    for section in &recipe.sections {
        if let Some(name) = &section.name {
            ui.heading(name);
        }
        section_columns(
            ui,
            |ui| {
                section.ingredients.iter().for_each(|x| {
                    ui.label(x);
                });
            },
            |ui| {
                section.instructions.iter().for_each(|x| {
                    ui.label(x);
                });
            },
        );
    }

    // Equipment + notes, mirroring the Cookbook tab's detail view.
    if !recipe.equipment.is_empty() || !recipe.notes.is_empty() {
        ui.separator();
        for e in &recipe.equipment {
            ui.label(format!("{} {e}", theme::icon::EQUIPMENT));
        }
        for n in &recipe.notes {
            ui.label(RichText::new(format!("{} {n}", theme::icon::NOTE)).weak());
        }
    }
}
