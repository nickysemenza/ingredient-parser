use chefsteps::parse_chefsteps;
use html::scrape_from_html;
use ingredient::{
    ingredient::Ingredient,
    rich_text::{Rich, RichParser},
    IngredientParser,
};
use ld_json::extract_ld;
use scraper::Html;

use serde::{Deserialize, Serialize};
mod chefsteps;
mod html;
pub mod ld_json;
mod ld_schema;
use thiserror::Error;
use tracing::info;

#[derive(Error, Debug)]
pub enum ScrapeError {
    #[error("could not find fetch `{0}`")]
    Http(String),
    #[error("could not find ld+json for `{0}`")]
    NoLDJSON(String),
    #[error("could not find recipe in ld+json for `{0}`, tried {1} chunks")]
    LDJSONMissingRecipe(String, usize),
    #[error("could not deserialize `{0}`")]
    Deserialize(#[from] serde_json::Error),
    #[error("could not parse `{0}`")]
    Parse(String),
}
/// Structured yield from a recipe (e.g., "12 pancakes")
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct RecipeYield {
    pub value: f64,
    pub unit: String,
}

/// Printed times. Any field may be absent. Shared workspace-wide: the web scraper
/// fills it from JSON-LD ISO-8601 durations, `recipe-epub` from the model's output.
/// `active` has no JSON-LD source, so it stays `None` for scraped recipes.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq)]
pub struct RecipeTimes {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prep: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cook: Option<String>,
}

impl RecipeTimes {
    /// `true` when every field is absent (so callers can collapse to `None`).
    pub fn is_empty(&self) -> bool {
        self.active.is_none() && self.total.is_none() && self.prep.is_none() && self.cook.is_none()
    }
}

/// One component of a recipe (e.g. "For the sauce"). A recipe is fundamentally
/// metadata + sections; the common case is a single unnamed section. Ingredient
/// and instruction lines are raw strings — [`ScrapedRecipe::parse`] structures
/// them with the core `ingredient` parser.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct RecipeSection {
    /// Component label; `None` for the main/only section.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub ingredients: Vec<String>,
    #[serde(default)]
    pub instructions: Vec<String>,
}

impl RecipeSection {
    /// An unnamed section — the common single-section case.
    pub fn new(ingredients: Vec<String>, instructions: Vec<String>) -> Self {
        Self {
            name: None,
            ingredients,
            instructions,
        }
    }
}

/// A section with its ingredient/instruction lines parsed.
#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct ParsedSection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub ingredients: Vec<Ingredient>,
    pub instructions: Vec<Rich>,
}

/// A scraped recipe: sections plus the metadata we can source from a page. The
/// metadata fields mirror `recipe-epub::RecipeMeta`'s names/types so the web and
/// EPUB flows converge on one shape (with `recipe_yield`/`servings`/`image` being
/// the web-only structured extras). `Default` lets the non-LD-JSON scrapers omit
/// the metadata fields via struct-update syntax.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct ScrapedRecipe {
    /// Recipe components; most recipes have a single unnamed section.
    pub sections: Vec<RecipeSection>,
    pub name: String,
    pub url: String,
    pub image: Option<String>,
    /// Parsed yield (e.g., value=12, unit="pancakes")
    pub recipe_yield: Option<RecipeYield>,
    /// Servings as integer (extracted from yield if unit is "serving(s)")
    pub servings: Option<u32>,
    /// Headnote / intro blurb (schema.org `description`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Prep/cook/total times (humanized from JSON-LD ISO-8601 durations).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub times: Option<RecipeTimes>,
    /// Dish category (schema.org `recipeCategory`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Tips / do-ahead notes (best-effort: a "Notes"/"Tips" instruction section).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
    /// Special equipment (best-effort: schema.org HowTo `tool`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub equipment: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct ParsedRecipe {
    pub sections: Vec<ParsedSection>,
}

/// Parse each section's raw ingredient/instruction lines with the core parser.
/// The [`RichParser`] is seeded with every ingredient name across all sections
/// so instructions in one component can reference ingredients from another.
/// Shared by [`ScrapedRecipe::parse`] and `recipe-epub`.
pub fn parse_sections(sections: &[RecipeSection]) -> Vec<ParsedSection> {
    let ip = IngredientParser::new();
    let parsed_ings: Vec<Vec<Ingredient>> = sections
        .iter()
        .map(|s| s.ingredients.iter().map(|i| ip.from_str(i)).collect())
        .collect();
    let names: Vec<String> = parsed_ings
        .iter()
        .flatten()
        .map(|i| i.name.clone())
        .collect();
    let rtp = RichParser::new(names);
    sections
        .iter()
        .zip(parsed_ings)
        .map(|(s, ingredients)| ParsedSection {
            name: s.name.clone(),
            ingredients,
            instructions: s
                .instructions
                .iter()
                .filter_map(|i| rtp.clone().parse(i).ok())
                .collect(),
        })
        .collect()
}

impl ScrapedRecipe {
    pub fn parse(&self) -> ParsedRecipe {
        ParsedRecipe {
            sections: parse_sections(&self.sections),
        }
    }

    /// All ingredient lines across every section, in order.
    pub fn ingredients(&self) -> impl Iterator<Item = &str> {
        self.sections
            .iter()
            .flat_map(|s| s.ingredients.iter().map(String::as_str))
    }

    /// All instruction lines across every section, in order.
    pub fn instructions(&self) -> impl Iterator<Item = &str> {
        self.sections
            .iter()
            .flat_map(|s| s.instructions.iter().map(String::as_str))
    }
}
// inspiration
// https://github.com/pombadev/sunny/blob/main/src/lib/spider.rs
// https://github.com/megametres/recettes-api/blob/dev/src/html_parser/mod.rs

pub fn scrape(body: &str, url: &str) -> Result<ScrapedRecipe, ScrapeError> {
    info!("scraping {} bytes from {url}", body.len());
    if url.contains("chefsteps.com") {
        info!("scraping chefsteps");
        return parse_chefsteps(body);
    }
    let dom = Html::parse_document(body);

    // Prefer LD+JSON, but fall back to HTML scraping on *any* LD failure —
    // missing, malformed/undeserializable, or present-but-no-recipe — rather
    // than erroring out. Previously only the "no ld+json at all" case degraded.
    let from_ld = match extract_ld(dom.clone()) {
        Ok(ld_schemas) => {
            let items = ld_schemas.len();
            ld_schemas
                .into_iter()
                .find_map(|ld| ld_json::scrape_from_ld_json(ld.as_str(), url).ok())
                .ok_or_else(|| ScrapeError::LDJSONMissingRecipe(url.to_string(), items))
        }
        Err(e) => Err(e),
    };

    let res = from_ld.or_else(|ld_err| {
        info!("ld+json scrape failed ({ld_err}); falling back to HTML");
        scrape_from_html(dom, url)
    });

    res.map(|mut r| {
        for s in &mut r.sections {
            s.ingredients = std::mem::take(&mut s.ingredients)
                .into_iter()
                .map(clean_string)
                .collect();
            s.instructions = std::mem::take(&mut s.instructions)
                .into_iter()
                .map(clean_string)
                .collect();
        }
        r
    })
}
fn clean_string(i: String) -> String {
    i.replace("&nbsp;", " ").replace('\n', " ")
}
