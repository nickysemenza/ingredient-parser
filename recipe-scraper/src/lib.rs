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

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct ScrapedRecipe {
    pub ingredients: Vec<String>,
    pub instructions: Vec<String>,
    pub name: String,
    pub url: String,
    pub image: Option<String>,
    /// Parsed yield (e.g., value=12, unit="pancakes")
    pub recipe_yield: Option<RecipeYield>,
    /// Servings as integer (extracted from yield if unit is "serving(s)")
    pub servings: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct ParsedRecipe {
    pub ingredients: Vec<Ingredient>,
    pub instructions: Vec<Rich>,
}
impl ScrapedRecipe {
    pub fn parse(&self) -> ParsedRecipe {
        let ip = IngredientParser::new();
        let ingredients = self
            .ingredients
            .iter()
            .map(|i| ip.from_str(i))
            .collect::<Vec<_>>();
        let names = ingredients
            .iter()
            .map(|i| i.name.clone())
            .collect::<Vec<_>>();
        let rtp = RichParser::new(names);
        let parsed_instructions = self
            .instructions
            .iter()
            .filter_map(|i| rtp.clone().parse(i).ok())
            .collect::<Vec<Rich>>();

        ParsedRecipe {
            ingredients,
            instructions: parsed_instructions,
        }
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
        r.ingredients = r.ingredients.into_iter().map(clean_string).collect();
        r.instructions = r.instructions.into_iter().map(clean_string).collect();
        r
    })
}
fn clean_string(i: String) -> String {
    i.replace("&nbsp;", " ").replace('\n', " ")
}
