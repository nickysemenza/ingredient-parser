use chefsteps::parse_chefsteps;
use html::scrape_from_html;
use html_escape::decode_html_entities;
use ingredient::{
    ingredient::Ingredient,
    rich_text::{Rich, RichParser},
    IngredientParser,
};
use ld_json::extract_ld;
// Re-exported on purpose: cubby's recipebridge wasm crate (separate repo)
// calls `recipe_scraper::parse_yield_string` — pub(crate) breaks its build.
pub use ld_json::parse_yield_string;
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
    #[error("fetch failed: {0}")]
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
// The plain recipe data shapes (yield, times, section) live in the deps-light
// `recipe-types` crate so the JSON contract can be depended on without the
// scraper/parser. Re-exported here so existing `recipe_scraper::RecipeSection`
// (etc.) paths and the workspace-wide "one shape" guarantee are unchanged.
pub use recipe_types::{RecipeSection, RecipeTimes, RecipeYield};

/// A section with its ingredient/instruction lines parsed.
#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct ParsedSection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub ingredients: Vec<Ingredient>,
    pub instructions: Vec<Rich>,
}

/// A scraped recipe: sections plus the metadata we can source from a page.
///
/// The `description`, `times`, `category`, `notes`, and `equipment` fields mirror
/// `recipe_types::RecipeMeta`'s names/types so the web and EPUB flows converge on
/// one shape. They are kept inline rather than `#[serde(flatten)] meta: RecipeMeta`
/// on purpose: the web shape diverges from `RecipeMeta` on two fields a flatten
/// can't reconcile — the page uses `name` (not `RecipeMeta`'s required `title`),
/// and `recipe_yield` is the structured `Option<RecipeYield>` here vs `RecipeMeta`'s
/// `Option<String>`. Flattening would emit a redundant `title` key and collide on
/// `recipe_yield`, changing the public JSON contract. The `drift_guard` test below
/// fails loudly if any of the five mirrored fields ever diverge from `RecipeMeta`.
/// `recipe_yield`/`servings`/`image` are the web-only structured extras. `Default`
/// lets the non-LD-JSON scrapers omit the metadata fields via struct-update syntax.
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
                .filter_map(|i| rtp.parse(i).ok())
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

    /// Normalize scraped text in place: decode HTML entities across every
    /// user-facing field, and collapse the non-breaking spaces / newlines in the
    /// ingredient & instruction lines the line parser consumes.
    ///
    /// JSON-LD extracted via `inner_html()` preserves entities literally and can
    /// be single- OR double-encoded — a page's `&#39;` round-trips through
    /// scrape/store as the literal `&amp;#39;` — so entities are decoded
    /// repeatedly until stable (see [`decode_entities`]). Idempotent on
    /// already-clean text, so running it twice (the ld+json path and the
    /// `scrape` post-pass both call it) or on hand-clean pages is a no-op.
    pub(crate) fn clean_text(&mut self) {
        self.name = decode_entities(&self.name);
        if let Some(d) = self.description.take() {
            self.description = Some(decode_entities(&d));
        }
        if let Some(c) = self.category.take() {
            self.category = Some(decode_entities(&c));
        }
        for note in &mut self.notes {
            *note = decode_entities(note);
        }
        for item in &mut self.equipment {
            *item = decode_entities(item);
        }
        for section in &mut self.sections {
            if let Some(name) = section.name.take() {
                section.name = Some(decode_entities(&name));
            }
            for line in &mut section.ingredients {
                *line = clean_line(line);
            }
            for line in &mut section.instructions {
                *line = clean_line(line);
            }
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
        return parse_chefsteps(body).map(|mut r| {
            r.clean_text();
            r
        });
    }
    let dom = Html::parse_document(body);

    // Prefer LD+JSON, but fall back to HTML scraping on *any* LD failure —
    // missing, malformed/undeserializable, or present-but-no-recipe — rather
    // than erroring out. Previously only the "no ld+json at all" case degraded.
    let from_ld = match extract_ld(&dom) {
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
        r.clean_text();
        r
    })
}

/// Decode HTML entities repeatedly until the string stops changing. Scraped
/// JSON-LD can be single- or double-encoded (e.g. `&amp;#39;` -> `&#39;` -> `'`),
/// so a single pass isn't enough. The cap bounds pathological input; clean text
/// stabilizes on the first pass.
fn decode_entities(s: &str) -> String {
    let mut current = s.to_owned();
    for _ in 0..3 {
        let next = decode_html_entities(&current).into_owned();
        if next == current {
            break;
        }
        current = next;
    }
    current
}

/// An ingredient/instruction line: decode entities, then collapse the
/// non-breaking spaces (decoded from `&nbsp;`) and newlines that would otherwise
/// break the line parser.
fn clean_line(s: &str) -> String {
    decode_entities(s).replace(['\u{a0}', '\n'], " ")
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use recipe_types::RecipeMeta;

    /// Drift guard for the five `RecipeMeta`-mirrored fields on [`ScrapedRecipe`]
    /// (`description`, `times`, `category`, `notes`, `equipment`). These are kept
    /// inline rather than `#[serde(flatten)] meta: RecipeMeta` because the web
    /// shape diverges on `name`/`recipe_yield` (see the `ScrapedRecipe` doc). This
    /// test round-trips a `RecipeMeta` JSON carrying those five fields through
    /// `ScrapedRecipe` deserialization: if `RecipeMeta` ever renames or retypes one
    /// of them, the field stops deserializing into `ScrapedRecipe` and this fails.
    #[test]
    fn scraped_recipe_mirrors_recipe_meta_fields() {
        let meta = RecipeMeta {
            title: "ignored".to_string(),
            description: Some("a blurb".to_string()),
            recipe_yield: Some("Makes 1 loaf".to_string()),
            times: Some(RecipeTimes {
                prep: Some("10 min".to_string()),
                cook: Some("20 min".to_string()),
                ..Default::default()
            }),
            equipment: vec!["stand mixer".to_string()],
            notes: vec!["make ahead".to_string()],
            category: Some("Dessert".to_string()),
            page: None,
        };

        // The five shared fields must deserialize from a RecipeMeta JSON object
        // into a ScrapedRecipe by the SAME keys/types. `title` and `recipe_yield`
        // intentionally DON'T map (the divergence that blocks a flatten): `title`
        // has no ScrapedRecipe counterpart (`name` differs), and `recipe_yield` is
        // a structured type here vs RecipeMeta's `String` — keeping it in the JSON
        // would even fail to deserialize. We strip those two before round-tripping
        // so this test asserts exactly the contract that the five fields share.
        let mut meta_json = serde_json::to_value(&meta).unwrap();
        let obj = meta_json.as_object_mut().unwrap();
        obj.remove("title");
        obj.remove("recipe_yield");
        // ScrapedRecipe requires the structural fields RecipeMeta lacks.
        obj.insert("sections".to_string(), serde_json::json!([]));
        obj.insert("name".to_string(), serde_json::json!(""));
        obj.insert("url".to_string(), serde_json::json!(""));

        let scraped: ScrapedRecipe = serde_json::from_value(meta_json).unwrap();

        assert_eq!(scraped.description, meta.description);
        assert_eq!(scraped.times, meta.times);
        assert_eq!(scraped.category, meta.category);
        assert_eq!(scraped.notes, meta.notes);
        assert_eq!(scraped.equipment, meta.equipment);
    }
}
