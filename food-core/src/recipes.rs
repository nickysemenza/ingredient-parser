//! Web recipes and instruction prose: scraped pages parsed into sections whose
//! instructions keep their measurements, scaled from the original source.
use ingredient::rich_text::{Chunk, RichParser};
use ingredient::unit::Measure;
use ingredient::{IngredientParser, ParseOptions, TraceDetail};
use recipe_scraper::ScrapedRecipe;
use recipe_types::RecipeTimes;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use ts_rs::TS;

use crate::AppResult;
use crate::ingredients::{IngredientRow, row};

/// One span of instruction prose.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum RichChunk {
    Text {
        text: String,
    },
    /// A known ingredient name.
    Ingredient {
        text: String,
    },
    /// Measurements, formatted by the parser in `text`.
    Measure {
        text: String,
        amounts: Vec<Measure>,
    },
}

/// A scraped recipe, parsed and scaled by `factor`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct RecipeView {
    pub title: String,
    pub url: String,
    pub image: Option<String>,
    pub description: Option<String>,
    pub category: Option<String>,
    pub times: Option<RecipeTimes>,
    /// The printed yield, e.g. "12 pancakes".
    pub recipe_yield: Option<String>,
    pub notes: Vec<String>,
    pub equipment: Vec<String>,
    pub factor: f64,
    pub sections: Vec<SectionView>,
    /// Instructions kept as authored text because they could not be parsed.
    pub diagnostics: Vec<String>,
    /// The scraped page; pass it back to `scale_recipe`.
    pub source: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
pub struct SectionView {
    pub name: Option<String>,
    pub ingredients: Vec<IngredientRow>,
    pub instructions: Vec<Vec<RichChunk>>,
}

fn chunk(chunk: Chunk, factor: f64) -> RichChunk {
    match chunk {
        Chunk::Text(text) => RichChunk::Text { text },
        Chunk::Ing(text) => RichChunk::Ingredient { text },
        Chunk::Measure(amounts) => {
            let amounts: Vec<Measure> = amounts.iter().map(|m| m.scale(factor)).collect();
            RichChunk::Measure {
                text: amounts
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(" / "),
                amounts,
            }
        }
    }
}

/// Measurements and known ingredient names in a piece of prose.
pub fn rich_text(text: &str, names: Vec<String>) -> Vec<RichChunk> {
    match RichParser::new(names).parse(text) {
        Ok(chunks) => chunks.into_iter().map(|c| chunk(c, 1.0)).collect(),
        Err(_) => vec![RichChunk::Text {
            text: text.to_owned(),
        }],
    }
}

fn validate_scale(factor: f64) -> AppResult<()> {
    if !factor.is_finite() || !(0.01..=100.0).contains(&factor) {
        return Err("Scale must be between 0.01 and 100".into());
    }
    Ok(())
}

/// Parse a scraped page at `factor`. Scaling always starts from the source, so
/// repeated changes never compound rounding; amounts that do not scale (pan
/// sizes, temperatures, times) stay as printed.
pub fn view(recipe: &ScrapedRecipe, factor: f64) -> AppResult<RecipeView> {
    validate_scale(factor)?;
    let parser = IngredientParser::new();
    let execution = recipe_parsing::execute_sections(
        &recipe.sections,
        &parser,
        ParseOptions {
            decomposition: true,
            trace: TraceDetail::None,
        },
    );
    let mut line_number = 0;
    let sections = execution
        .recipe
        .sections
        .into_iter()
        .zip(&recipe.sections)
        .zip(&execution.observations)
        .map(|((parsed, source), observations)| SectionView {
            name: parsed.name,
            ingredients: parsed
                .ingredients
                .into_iter()
                .zip(&source.ingredients)
                .zip(observations)
                .map(|((mut ingredient, input), observation)| {
                    line_number += 1;
                    ingredient.amounts =
                        ingredient.amounts.iter().map(|m| m.scale(factor)).collect();
                    row(
                        input,
                        line_number,
                        ingredient,
                        observation.decomposition.as_ref(),
                    )
                })
                .collect(),
            instructions: parsed
                .instructions
                .into_iter()
                .map(|chunks| chunks.into_iter().map(|c| chunk(c, factor)).collect())
                .collect(),
        })
        .collect();
    Ok(RecipeView {
        title: recipe.name.clone(),
        url: recipe.url.clone(),
        image: recipe.image.clone(),
        description: recipe.description.clone(),
        category: recipe.category.clone(),
        times: recipe.times.clone(),
        recipe_yield: recipe.recipe_yield.as_ref().map(|y| {
            format!("{} {}", ingredient::util::format_quantity(y.value), y.unit)
                .trim()
                .to_owned()
        }),
        notes: recipe.notes.clone(),
        equipment: recipe.equipment.clone(),
        factor,
        sections,
        diagnostics: execution
            .instruction_diagnostics
            .iter()
            .map(|d| {
                format!(
                    "Section {}, instruction {}: {}",
                    d.section + 1,
                    d.instruction + 1,
                    d.message
                )
            })
            .collect(),
        source: serde_json::to_value(recipe).map_err(|e| e.to_string())?,
    })
}

/// Scrape a fetched page.
pub fn from_html(html: &str, url: &str) -> AppResult<RecipeView> {
    let recipe = recipe_scraper::scrape(html, url).map_err(|e| e.to_string())?;
    view(&recipe, 1.0)
}

/// Rescale a recipe from the `source` a previous view returned.
pub fn scale(source: Value, factor: f64) -> AppResult<RecipeView> {
    let recipe: ScrapedRecipe = serde_json::from_value(source).map_err(|e| e.to_string())?;
    view(&recipe, factor)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use ingredient::unit::{MeasureKind, Unit};

    const PAGE: &str = r#"<script type="application/ld+json">{"name":"Soup","recipeIngredient":["1 cup (240 g) water","1 tsp salt"],"recipeInstructions":[{"@type":"HowToStep","text":"Add 1 cup (240 g) water; cut into 3cm cubes, then bake at 365 degrees F for 20 minutes."}]}</script>"#;

    fn measures(view: &RecipeView) -> Vec<Measure> {
        view.sections
            .iter()
            .flat_map(|s| s.instructions.iter().flatten())
            .filter_map(|c| match c {
                RichChunk::Measure { amounts, .. } => Some(amounts.clone()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    #[test]
    fn scaling_changes_quantities_once_and_preserves_source_and_constraints() {
        let original = from_html(PAGE, "https://example.com/soup").unwrap();
        assert_eq!(original.title, "Soup");
        let first = &original.sections[0].ingredients[0];
        assert_eq!(first.ingredient.name, "water");
        assert_eq!(first.line_number, 1);
        assert_eq!(original.sections[0].ingredients[1].line_number, 2);

        let scaled = scale(original.source.clone(), 2.0).unwrap();
        assert_eq!(scaled.source, original.source);
        let row = &scaled.sections[0].ingredients[0];
        assert_eq!(row.input, first.input);
        assert_eq!(row.segments, first.segments);
        assert!(
            row.ingredient
                .amounts
                .iter()
                .any(|m| *m.unit() == Unit::Cup && m.value() == 2.0)
        );
        assert!(
            row.ingredient
                .amounts
                .iter()
                .any(|m| *m.unit() == Unit::Gram && m.value() == 480.0)
        );
        assert!(row.amounts.iter().any(|a| a.contains('2')));

        let before = measures(&original);
        let after = measures(&scaled);
        for kind in [
            MeasureKind::Length,
            MeasureKind::Temperature,
            MeasureKind::Time,
        ] {
            assert_eq!(
                before.iter().find(|m| m.kind() == kind).unwrap(),
                after.iter().find(|m| m.kind() == kind).unwrap()
            );
        }
        assert_eq!(scale(scaled.source, 1.0).unwrap(), original);
        assert!(scale(original.source, 0.0).is_err());
    }

    #[test]
    fn prose_highlights_names_and_measures() {
        let chunks = rich_text(
            "Add 1/2 cup / 236 grams water to the bowl with the salt and mix.",
            vec!["water".into(), "salt".into()],
        );
        assert!(
            chunks
                .iter()
                .any(|c| matches!(c, RichChunk::Ingredient { text } if text == "salt"))
        );
        let measures: Vec<_> = chunks
            .iter()
            .filter_map(|c| match c {
                RichChunk::Measure { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(measures, vec!["½ cup", "236 g"]);
    }
}
