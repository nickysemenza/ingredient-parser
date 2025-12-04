use scraper::{Html, Selector};
use serde_json::Value;
use tracing::error;

use crate::{ld_schema, ScrapeError, ScrapedRecipe};

#[tracing::instrument]
fn normalize_root_recipe(
    ld_schema: ld_schema::RootRecipe,
    url: &str,
) -> Result<ScrapedRecipe, ScrapeError> {
    let instructions = match ld_schema.recipe_instructions {
        ld_schema::InstructionWrapper::A(a) => a.into_iter().map(|i| i.text).collect(),
        ld_schema::InstructionWrapper::B(b) => b
            .into_iter()
            .flat_map(|i| match i {
                ld_schema::BOrWrapper::B(b) => b
                    .item_list_element
                    .iter()
                    .filter_map(|i| i.text.clone())
                    .collect::<Vec<_>>(),
                ld_schema::BOrWrapper::Wrapper(w) => w.text.into_iter().collect::<Vec<_>>(),
            })
            .collect(),

        ld_schema::InstructionWrapper::C(c) => {
            let selector = Selector::parse("p")
                .map_err(|e| ScrapeError::Parse(format!("invalid selector 'p': {e}")))?;

            Html::parse_fragment(c.as_ref())
                .select(&selector)
                .map(|i| i.text().collect::<Vec<_>>().join(""))
                .collect::<Vec<_>>()
        }
        ld_schema::InstructionWrapper::D(d) => d[0].clone().into_iter().map(|i| i.text).collect(),
    };

    Ok(ScrapedRecipe {
        ingredients: ld_schema.recipe_ingredient,
        instructions,
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
    })
}
#[tracing::instrument]
fn normalize_ld_json(
    ld_schema_a: ld_schema::Root,
    url: &str,
) -> Result<ScrapedRecipe, ScrapeError> {
    match ld_schema_a {
        ld_schema::Root::List(mut l) => match l.pop() {
            Some(recipe) => normalize_root_recipe(recipe, url),
            None => Err(ScrapeError::LDJSONMissingRecipe(url.to_string(), 0)),
        },
        ld_schema::Root::Recipe(ld_schema) => normalize_root_recipe(ld_schema, url),
        ld_schema::Root::Graph(g) => {
            let items = g.graph.len();
            let recipe = g.graph.iter().find_map(|d| match d {
                ld_schema::Graph::Recipe(a) => Some(a.to_owned()),
                _ => None,
            });
            match recipe {
                Some(r) => normalize_root_recipe(r, url),
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
            // Try to log the raw JSON for debugging if possible
            if let Ok(raw) = serde_json::from_str::<Value>(json) {
                if let Ok(pretty) = serde_json::to_string_pretty(&raw) {
                    error!("failed to find ld json root: {}", pretty);
                }
            }
            return Err(ScrapeError::Deserialize(e));
        }
    };

    Ok(v)
}

pub fn scrape_from_ld_json(json: &str, url: &str) -> Result<ScrapedRecipe, ScrapeError> {
    let ld_schema = parse_ld_json(json.to_owned())?;
    normalize_ld_json(ld_schema, url)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use crate::{
        ld_json::{extract_ld, normalize_ld_json, parse_ld_json, scrape_from_ld_json},
        ld_schema::{InstructionWrapper, Root, RootRecipe},
    };
    use scraper::Html;

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

    #[test]
    fn test_parse_invalid_json() {
        // Test that invalid JSON returns an error
        let result = parse_ld_json("not valid json".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_normalize_list_root() {
        // Test normalizing a Root::List with one recipe
        let root = Root::List(vec![RootRecipe {
            context: None,
            name: "Test Recipe".to_string(),
            image: None,
            recipe_ingredient: vec!["1 cup flour".to_string()],
            recipe_instructions: InstructionWrapper::A(vec![]),
        }]);

        let result = normalize_ld_json(root, "https://example.com");
        assert!(result.is_ok());
        let recipe = result.unwrap();
        assert_eq!(recipe.name, "Test Recipe");
    }

    #[test]
    fn test_normalize_empty_list() {
        // Test normalizing an empty Root::List
        let root = Root::List(vec![]);
        let result = normalize_ld_json(root, "https://example.com");
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_ld_no_script() {
        // HTML without ld+json script
        let html = Html::parse_document("<html><body>No recipe here</body></html>");
        let result = extract_ld(html);
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_ld_with_script() {
        // HTML with ld+json script
        let html = Html::parse_document(
            r#"<html>
            <head>
                <script type="application/ld+json">{"name": "test"}</script>
            </head>
            <body></body>
            </html>"#,
        );
        let result = extract_ld(html);
        assert!(result.is_ok());
        let scripts = result.unwrap();
        assert_eq!(scripts.len(), 1);
        assert!(scripts[0].contains("name"));
    }

    #[test]
    fn test_scrape_from_ld_json_valid() {
        let json = r#"{
            "name": "Chocolate Cake",
            "recipeIngredient": ["2 cups flour", "1 cup sugar"],
            "recipeInstructions": []
        }"#;

        let result = scrape_from_ld_json(json, "https://example.com/cake");
        assert!(result.is_ok());
        let recipe = result.unwrap();
        assert_eq!(recipe.name, "Chocolate Cake");
        assert_eq!(recipe.ingredients.len(), 2);
        assert_eq!(recipe.url, "https://example.com/cake");
    }

    #[test]
    fn test_scrape_from_ld_json_invalid() {
        let json = "not valid json at all";
        let result = scrape_from_ld_json(json, "https://example.com");
        assert!(result.is_err());
    }

    #[test]
    fn test_instruction_wrapper_c() {
        // Test InstructionWrapper::C (HTML string instructions)
        let json = r#"{
            "name": "Test",
            "recipeIngredient": [],
            "recipeInstructions": "<p>Step 1</p><p>Step 2</p>"
        }"#;

        let result = scrape_from_ld_json(json, "https://example.com");
        assert!(result.is_ok());
        let recipe = result.unwrap();
        assert_eq!(recipe.instructions.len(), 2);
        assert_eq!(recipe.instructions[0], "Step 1");
        assert_eq!(recipe.instructions[1], "Step 2");
    }
}
