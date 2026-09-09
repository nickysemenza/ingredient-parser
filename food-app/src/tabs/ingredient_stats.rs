//! Cached distribution over the saved run; shared counting lives in recipe-epub.
use eframe::egui;
use recipe_epub::review::stats::{IngredientStats, NameSort};

#[derive(Default)]
pub(super) struct StatsPanel {
    pub stats: Option<IngredientStats>,
    pub error: Option<String>,
    query: String,
    rare_only: bool,
    sort: NameSort,
    selected: Option<String>,
}
impl StatsPanel {
    /// Returns the source document selected for review.
    pub fn show(&mut self, ui: &mut egui::Ui) -> Option<String> {
        let Some(stats) = &self.stats else {
            ui.label(
                self.error
                    .as_deref()
                    .unwrap_or("No saved ingredient statistics available."),
            );
            return None;
        };
        ui.heading("Ingredient names");
        ui.label(format!(
            "{} occurrences · {} exact names · {} recipes · {} names used once",
            stats.total_occurrences, stats.unique_names, stats.total_recipes, stats.singleton_names
        ));
        ui.label("Saved parsed names, without merging synonyms. Select a name to inspect its original lines.");
        if !stats.complete {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                "Incomplete extraction: counts cover saved recipes only.",
            );
        }
        ui.horizontal_wrapped(|ui| {
            ui.add(egui::TextEdit::singleline(&mut self.query).hint_text("Find ingredient name"));
            ui.checkbox(&mut self.rare_only, "Used once");
            egui::ComboBox::from_id_salt("ingredient-stats-sort")
                .selected_text(match self.sort {
                    NameSort::Occurrences => "Most occurrences",
                    NameSort::Recipes => "Most recipes",
                    NameSort::Name => "Name A–Z",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut self.sort, NameSort::Occurrences, "Most occurrences");
                    ui.selectable_value(&mut self.sort, NameSort::Recipes, "Most recipes");
                    ui.selectable_value(&mut self.sort, NameSort::Name, "Name A–Z");
                });
        });
        let names = stats.select(&self.query, self.rare_only.then_some(1), self.sort);
        if names.is_empty() {
            ui.label(if stats.total_occurrences == 0 {
                "No ingredient occurrences in this run."
            } else {
                "No names match these filters."
            });
            return None;
        }
        if !names
            .iter()
            .any(|n| self.selected.as_ref() == Some(&n.name))
        {
            self.selected = names.first().map(|n| n.name.clone());
        }
        ui.label(format!(
            "{} matching names. Bars show occurrences relative to the most frequent name.",
            names.len()
        ));
        let max = stats
            .names
            .first()
            .map(|n| n.occurrences)
            .unwrap_or(1)
            .max(1) as f32;
        let mut jump = None;
        ui.columns(2, |columns| {
            egui::ScrollArea::vertical().id_salt("ingredient-name-distribution").show(&mut columns[0], |ui| {
                for row in &names {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            let name = if row.name.is_empty() { "(empty name)" } else { &row.name };
                            if ui.selectable_label(self.selected.as_ref()==Some(&row.name), name).clicked() { self.selected = Some(row.name.clone()); }
                            ui.label(format!("Recipes: {} · Distinct lines: {}",row.recipes,row.distinct_inputs));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.add(egui::ProgressBar::new(row.occurrences as f32 / max).desired_width(130.0).text(format!("{} ({:.1}%)",row.occurrences,100.0 * row.occurrences as f64 / stats.total_occurrences.max(1) as f64)));
                        });
                    });
                    ui.separator();
                }
            });
            egui::ScrollArea::vertical().id_salt("ingredient-name-examples").show(&mut columns[1], |ui| {
                if let Some(row) = names.iter().find(|n| self.selected.as_ref()==Some(&n.name)) {
                    ui.heading(if row.name.is_empty() { "(empty name)" } else { &row.name });
                    ui.label(format!("Occurrences: {} · Recipes: {}. Percentages use all {} ingredient occurrences.",row.occurrences,row.recipes,stats.total_occurrences));
                    for example in &row.examples {
                        ui.push_id((example.recipe_index,example.section_index,example.line_index), |ui| {
                            ui.strong(&example.input);
                            ui.label(&example.recipe);
                            if let Some(section) = &example.section { ui.label(section); }
                            if ui.button("Open source").on_hover_text(&example.source).clicked() {
                                jump = example.source.rsplit_once('#').map(|(_, doc)|doc.to_owned());
                            }
                            ui.separator();
                        });
                    }
                }
            });
        });
        jump
    }
}
