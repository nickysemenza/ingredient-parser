use crate::{ScrapeError, ScrapedRecipe};
use serde::{Deserialize, Serialize};

pub(crate) fn parse_chefsteps(json: &str) -> Result<ScrapedRecipe, ScrapeError> {
    let v: Root = serde_json::from_str(json)?;
    let ingredients = v
        .ingredients
        .iter()
        .map(|i| {
            if i.note.is_empty() {
                format!("{} {} {}", i.quantity, i.unit, i.title)
            } else {
                format!("{} {} {}, {}", i.quantity, i.unit, i.title, i.note)
            }
        })
        .collect();
    let instructions = v.steps.iter().map(|i| i.directions.clone()).collect();
    Ok(ScrapedRecipe {
        sections: vec![crate::RecipeSection::new(ingredients, instructions)],
        name: v.title,
        url: v.url,
        image: Some(v.image),
        // ChefSteps API doesn't provide yield/metadata in the parsed format.
        ..Default::default()
    })
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Root {
    pub title: String,
    pub image: String,
    pub url: String,
    pub ingredients: Vec<Ingredient>,
    pub steps: Vec<Step>,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ingredient {
    pub title: String,
    pub quantity: String,
    pub unit: String,
    pub note: String,
}

#[derive(Default, Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    pub directions: String,
}
