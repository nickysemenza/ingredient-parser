use scraper::{Html, Selector};
use serde_json::Value;
use tracing::error;

use crate::{ld_schema, ScrapeError, ScrapedRecipe};

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

pub(crate) fn extract_ld(dom: Html) -> Result<Vec<String>, ScrapeError> {
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
    let v: ld_schema::Root = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(e) => {
            let raw = serde_json::from_str::<Value>(json).expect("failed to parse ld json");
            error!(
                "failed to find ld json root: {}",
                serde_json::to_string_pretty(&raw).unwrap()
            );
            return Err(ScrapeError::Deserialize(e));
        }
    };

    Ok(v)
}

pub fn scrape_from_ld_json(json: &str, url: &str) -> Result<ScrapedRecipe, ScrapeError> {
    let ld_schema = parse_ld_json(json.to_owned()).expect("failed to parse ld json");
    normalize_ld_json(ld_schema, url)
}

#[cfg(test)]
mod tests {
    use crate::{ld_json::parse_ld_json, ld_schema::InstructionWrapper};

    #[test]
    fn json() {
        assert_eq!(
            parse_ld_json(
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
