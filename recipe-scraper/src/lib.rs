use ingredient::{
    ingredient::Ingredient,
    rich_text::{Rich, RichParser},
    IngredientParser,
};
use scraper::{Html, Selector};

use serde::{Deserialize, Serialize};
use serde_json::Value;
mod ld_schema;
use thiserror::Error;
use tracing::{error, info};

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
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct ScrapedRecipe {
    pub ingredients: Vec<String>,
    pub instructions: Vec<String>,
    pub name: String,
    pub url: String,
    pub image: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
pub struct ParsedRecipe {
    pub ingredients: Vec<Ingredient>,
    pub instructions: Vec<Rich>,
}
impl ScrapedRecipe {
    pub fn parse(&self) -> ParsedRecipe {
        let ip = IngredientParser::new(false);
        let ingredients = self
            .ingredients
            .iter()
            .map(|i| ip.clone().from_str(i))
            .collect::<Vec<_>>();
        let names = ingredients.iter().map(|i| i.name.clone()).collect();
        let rtp = RichParser {
            ingredient_names: names,
            ip: IngredientParser::new(true),
        };
        let parsed_instructions = self
            .instructions
            .iter()
            .map(|i| rtp.clone().parse(i).unwrap())
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
    info!("scraping {} from {}", body.len(), url);
    let dom = Html::parse_document(body);
    let res = match extract_ld(dom.clone()) {
        Ok(ld_schemas) => {
            let items = ld_schemas.len();
            match ld_schemas
                .into_iter()
                .map(|ld| scrape_from_json(ld.as_str(), url))
                // .collect::<Vec<Result<ScrapedRecipe, ScrapeError>>>()
                .find_map(Result::ok)
            {
                Some(r) => Ok(r),
                None => Err(ScrapeError::LDJSONMissingRecipe(url.to_string(), items)),
            }
        }
        Err(e) => match e {
            ScrapeError::NoLDJSON(_) => scrape_from_html(dom, url),
            _ => Err(e),
        },
    };
    match res {
        Ok(mut r) => {
            r.ingredients = r.ingredients.into_iter().map(clean_string).collect();
            r.instructions = r.instructions.into_iter().map(clean_string).collect();
            Ok(r)
        }
        Err(e) => Err(e),
    }
}
fn clean_string(i: String) -> String {
    i.replace("&nbsp;", " ").replace('\n', " ")
}
pub fn scrape_from_json(json: &str, url: &str) -> Result<ScrapedRecipe, ScrapeError> {
    normalize_ld_json(parse_ld_json(json.to_owned())?, url)
}

#[tracing::instrument]
fn normalize_root_recipe(ld_schema: ld_schema::RootRecipe, url: &str) -> ScrapedRecipe {
    ScrapedRecipe {
        ingredients: ld_schema.recipe_ingredient,
        instructions: match ld_schema.recipe_instructions {
            ld_schema::InstructionWrapper::A(a) => a.into_iter().map(|i| i.text).collect(),
            ld_schema::InstructionWrapper::B(b) => b
                .into_iter()
                .flat_map(|i| match i {
                    ld_schema::BOrWrapper::B(b) => b
                        .item_list_element
                        .iter()
                        .map(|i| i.text.clone().unwrap())
                        .collect(),
                    ld_schema::BOrWrapper::Wrapper(w) => {
                        vec![w.text.unwrap()]
                    }
                })
                .collect(),

            ld_schema::InstructionWrapper::C(c) => {
                let selector = Selector::parse("p").unwrap();

                Html::parse_fragment(c.as_ref())
                    .select(&selector)
                    .map(|i| i.text().collect::<Vec<_>>().join(""))
                    .collect::<Vec<_>>()
            }
            ld_schema::InstructionWrapper::D(d) => {
                d[0].clone().into_iter().map(|i| i.text).collect()
            }
        },

        name: ld_schema.name,
        url: url.to_string(),
        image: match ld_schema.image {
            Some(image) => match image {
                ld_schema::ImageOrList::Url(i) => Some(i),
                ld_schema::ImageOrList::List(l) => Some(l[0].url.clone()),
                ld_schema::ImageOrList::UrlList(i) => Some(i[0].clone()),
                ld_schema::ImageOrList::Image(i) => Some(i.url),
            },
            None => None,
        },
    }
}
#[tracing::instrument]
fn normalize_ld_json(
    ld_schema_a: ld_schema::Root,
    url: &str,
) -> Result<ScrapedRecipe, ScrapeError> {
    match ld_schema_a {
        ld_schema::Root::List(mut l) => Ok(normalize_root_recipe(l.pop().unwrap(), url)),
        ld_schema::Root::Recipe(ld_schema) => Ok(normalize_root_recipe(ld_schema, url)),
        ld_schema::Root::Graph(g) => {
            let items = g.graph.len();
            let recipe = g.graph.iter().find_map(|d| match d {
                ld_schema::Graph::Recipe(a) => Some(a.to_owned()),
                _ => None,
            });
            match recipe {
                Some(r) => Ok(normalize_root_recipe(r, url)),
                None => Err(ScrapeError::LDJSONMissingRecipe(
                    "failed to find recipe in ld json graph".to_string(),
                    items,
                )),
            }
        }
    }
}
fn scrape_from_html(dom: Html, url: &str) -> Result<ScrapedRecipe, ScrapeError> {
    let title = match dom.select(&Selector::parse("title").unwrap()).next() {
        Some(x) => x.inner_html(),
        None => "".to_string(),
    };
    // smitten kitchen
    let ingredient_selector = Selector::parse("li.jetpack-recipe-ingredient").unwrap();
    let ingredients = dom
        .select(&ingredient_selector)
        .map(|i| i.text().collect::<Vec<_>>().join(""))
        .collect::<Vec<String>>();

    let ul_selector = Selector::parse(r#"div.jetpack-recipe-directions"#).unwrap();

    let instruction_list_item_elem = match dom.select(&ul_selector).next() {
        Some(x) => x,
        None => return Err(ScrapeError::Parse("no ld json or parsed html".to_string())),
    };

    let instructions = instruction_list_item_elem
        .text()
        .collect::<Vec<_>>()
        .join("")
        .split('\n')
        .map(|s| s.into())
        .collect::<Vec<String>>();

    let image_selector = Selector::parse(r#"meta[property="og:image"]"#).unwrap();
    let image = dom
        .select(&image_selector)
        .next()
        .map(|i| i.value().attr("content").unwrap().to_string());

    Ok(dbg!(ScrapedRecipe {
        ingredients,
        instructions,
        name: title,
        url: url.to_string(),
        image,
    }))
    // Err(ScrapeError::Parse("foo".to_string()))
}
fn extract_ld(dom: Html) -> Result<Vec<String>, ScrapeError> {
    let selector = match Selector::parse("script[type='application/ld+json']") {
        Ok(s) => s,
        Err(e) => return Err(ScrapeError::Parse(format!("{e:?}"))),
    };

    let json_chunks: Vec<String> = dom
        .select(&selector)
        .map(|element| element.inner_html())
        .collect();
    match json_chunks.len() {
        0 => Err(ScrapeError::NoLDJSON(
            dom.root_element().html().chars().take(40).collect(),
        )),
        _ => Ok(json_chunks),
    }
}
fn parse_ld_json(json: String) -> Result<ld_schema::Root, ScrapeError> {
    let json = json.as_str();
    let raw = serde_json::from_str::<Value>(json)?;
    // tracing::info!("raw json: {:#?}", raw);
    let v: ld_schema::Root = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(e) => {
            error!(
                "failed to parse ld json: {}",
                serde_json::to_string_pretty(&raw).unwrap()
            );
            return Err(ScrapeError::Deserialize(e));
        }
    };

    Ok(v)
}
#[cfg(test)]
mod tests {
    use crate::ld_schema::InstructionWrapper;

    #[test]
    fn json() {
        assert_eq!(
            crate::parse_ld_json(
                r#"{
  "name": "",
  "recipeIngredient": [],
  "recipeInstructions": []
}
"#
                .to_string()
            )
            .unwrap(),
            crate::ld_schema::Root::Recipe(crate::ld_schema::RootRecipe {
                context: None,
                name: "".to_string(),
                image: None,
                recipe_ingredient: vec![],
                recipe_instructions: InstructionWrapper::A(vec![]),
            })
        );
    }
}
