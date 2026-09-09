//! Counts over saved parses. No reparsing, normalization, or model calls.
use super::{ReviewRun, error};
use crate::EpubError;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize)]
pub struct IngredientOccurrence {
    pub recipe_index: usize,
    pub section_index: usize,
    pub line_index: usize,
    pub source: String,
    pub recipe: String,
    pub section: Option<String>,
    pub input: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct IngredientNameCount {
    pub name: String,
    pub occurrences: usize,
    pub recipes: usize,
    pub distinct_inputs: usize,
    pub examples: Vec<IngredientOccurrence>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IngredientStats {
    pub epub_sha256: String,
    pub complete: bool,
    pub total_occurrences: usize,
    pub total_recipes: usize,
    pub unique_names: usize,
    pub singleton_names: usize,
    pub names: Vec<IngredientNameCount>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum NameSort {
    #[default]
    Occurrences,
    Recipes,
    Name,
}

impl IngredientStats {
    /// Filtering leaves book-wide totals unchanged. Ties use exact name order.
    pub fn select(
        &self,
        query: &str,
        max_count: Option<usize>,
        sort: NameSort,
    ) -> Vec<&IngredientNameCount> {
        let query = query.to_lowercase();
        let mut names: Vec<_> = self
            .names
            .iter()
            .filter(|n| {
                n.name.to_lowercase().contains(&query)
                    && max_count.is_none_or(|max| n.occurrences <= max)
            })
            .collect();
        names.sort_by(|a, b| match sort {
            NameSort::Occurrences => b.occurrences.cmp(&a.occurrences).then(a.name.cmp(&b.name)),
            NameSort::Recipes => b.recipes.cmp(&a.recipes).then(a.name.cmp(&b.name)),
            NameSort::Name => a.name.cmp(&b.name),
        });
        names
    }
}

/// Use stored names exactly, including empty names, and retain every occurrence
/// for drill-down. Reject stale/malformed parse shapes rather than undercounting.
pub fn ingredient_stats(run: &ReviewRun) -> Result<IngredientStats, EpubError> {
    #[derive(Deserialize)]
    struct ParsedRecipe {
        sections: Vec<ParsedSection>,
    }
    #[derive(Deserialize)]
    struct ParsedSection {
        ingredients: Vec<ParsedIngredient>,
    }
    #[derive(Deserialize)]
    struct ParsedIngredient {
        name: String,
    }
    let parsed: Vec<ParsedRecipe> = serde_json::from_value(run.parsed.clone())?;
    if parsed.len() != run.recipes.len() {
        return Err(error(
            "saved recipe/parse counts differ; replay the run first",
        ));
    }
    let mut groups: BTreeMap<String, Vec<IngredientOccurrence>> = BTreeMap::new();
    for (ri, (recipe, parsed)) in run.recipes.iter().zip(parsed).enumerate() {
        if recipe.sections.len() != parsed.sections.len() {
            return Err(error(
                "saved section/parse counts differ; replay the run first",
            ));
        }
        for (si, (section, parsed)) in recipe.sections.iter().zip(parsed.sections).enumerate() {
            if section.ingredients.len() != parsed.ingredients.len() {
                return Err(error(
                    "saved ingredient/parse counts differ; replay the run first",
                ));
            }
            for (li, (input, parsed)) in section
                .ingredients
                .iter()
                .zip(parsed.ingredients)
                .enumerate()
            {
                groups
                    .entry(parsed.name)
                    .or_default()
                    .push(IngredientOccurrence {
                        recipe_index: ri,
                        section_index: si,
                        line_index: li,
                        source: recipe.url.clone(),
                        recipe: recipe.meta.title.clone(),
                        section: section.name.clone(),
                        input: input.clone(),
                    });
            }
        }
    }
    let mut names: Vec<_> = groups
        .into_iter()
        .map(|(name, examples)| IngredientNameCount {
            name,
            occurrences: examples.len(),
            recipes: examples
                .iter()
                .map(|e| e.recipe_index)
                .collect::<BTreeSet<_>>()
                .len(),
            distinct_inputs: examples
                .iter()
                .map(|e| &e.input)
                .collect::<BTreeSet<_>>()
                .len(),
            examples,
        })
        .collect();
    names.sort_by(|a, b| b.occurrences.cmp(&a.occurrences).then(a.name.cmp(&b.name)));
    Ok(IngredientStats {
        epub_sha256: run.epub_sha256.clone(),
        complete: !run.incomplete(),
        total_occurrences: names.iter().map(|n| n.occurrences).sum(),
        total_recipes: run.recipes.len(),
        unique_names: names.len(),
        singleton_names: names.iter().filter(|n| n.occurrences == 1).count(),
        names,
    })
}
