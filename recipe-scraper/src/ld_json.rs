use ingredient::IngredientParser;
use scraper::{Html, Selector};
use serde_json::Value;
use tracing::{error, info};

use crate::{ld_schema, RecipeYield, ScrapeError, ScrapedRecipe};

/// Parse a yield string like "4 servings" or "12 pancakes" into RecipeYield
/// Also returns servings as integer if the unit is "serving(s)"
fn parse_yield_string(input: &str) -> (Option<RecipeYield>, Option<u32>) {
    let parser = IngredientParser::new().with_units(&["serving", "servings"]);
    match parser.parse_amount(input) {
        Ok(amounts) if !amounts.is_empty() => {
            let first = &amounts[0];
            let unit = first.unit().to_str(); // Use to_str() for proper string, not Debug format
            let value = first.values().0;

            // Check if this is servings
            let servings = if unit == "serving" || unit == "servings" {
                Some(value as u32)
            } else {
                None
            };

            (Some(RecipeYield { value, unit }), servings)
        }
        _ => {
            // Try to parse as a simple number (just servings)
            if let Ok(num) = input.trim().parse::<u32>() {
                (
                    Some(RecipeYield {
                        value: num as f64,
                        unit: "servings".to_string(),
                    }),
                    Some(num),
                )
            } else {
                info!("Could not parse yield: {}", input);
                (None, None)
            }
        }
    }
}

/// Extract the first useful yield string from the RecipeYieldWrapper
fn extract_yield_from_wrapper(
    wrapper: &ld_schema::RecipeYieldWrapper,
) -> (Option<RecipeYield>, Option<u32>) {
    match wrapper {
        ld_schema::RecipeYieldWrapper::String(s) => parse_yield_string(s),
        ld_schema::RecipeYieldWrapper::Number(n) => (
            Some(RecipeYield {
                value: *n,
                unit: "servings".to_string(),
            }),
            Some(*n as u32),
        ),
        ld_schema::RecipeYieldWrapper::StringArray(arr) => {
            // Try each string until we get a successful parse
            for s in arr {
                let (recipe_yield, servings) = parse_yield_string(s);
                if recipe_yield.is_some() {
                    return (recipe_yield, servings);
                }
            }
            (None, None)
        }
        ld_schema::RecipeYieldWrapper::NumberArray(arr) => {
            // Use first number as servings
            arr.first().map_or((None, None), |n| {
                (
                    Some(RecipeYield {
                        value: *n,
                        unit: "servings".to_string(),
                    }),
                    Some(*n as u32),
                )
            })
        }
    }
}

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

    // Parse yield if present
    let (recipe_yield, servings) = ld_schema
        .recipe_yield
        .as_ref()
        .map(extract_yield_from_wrapper)
        .unwrap_or((None, None));

    Ok(ScrapedRecipe {
        ingredients: ld_schema.recipe_ingredient,
        instructions,
        name: ld_schema.name,
        url: url.to_string(),
        image: ld_schema.image.map(|image| match image {
            ld_schema::ImageOrList::Url(i) => i,
            ld_schema::ImageOrList::List(l) => l[0].url.clone(),
            ld_schema::ImageOrList::UrlList(i) => i[0].clone(),
            ld_schema::ImageOrList::Image(i) => i.url,
        }),
        recipe_yield,
        servings,
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
        ld_json::{
            extract_ld, extract_yield_from_wrapper, normalize_ld_json, parse_ld_json,
            parse_yield_string, scrape_from_ld_json,
        },
        ld_schema::{InstructionWrapper, RecipeYieldWrapper, Root, RootRecipe},
        RecipeYield,
    };
    use rstest::rstest;
    use scraper::Html;

    // ============================================================================
    // parse_ld_json() Tests
    // ============================================================================

    #[test]
    fn test_parse_valid_json() {
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
                recipe_yield: None,
                recipe_ingredient: vec![],
                recipe_instructions: InstructionWrapper::A(vec![]),
            })
        );
    }

    #[rstest]
    #[case::invalid_json("not valid json")]
    #[case::empty("")]
    #[case::incomplete("{")]
    fn test_parse_invalid_json(#[case] input: &str) {
        let result = parse_ld_json(input.to_string());
        assert!(result.is_err());
    }

    // ============================================================================
    // normalize_ld_json() Tests
    // ============================================================================

    #[test]
    fn test_normalize_list_root() {
        let root = Root::List(vec![RootRecipe {
            context: None,
            name: "Test Recipe".to_string(),
            image: None,
            recipe_yield: None,
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
        let root = Root::List(vec![]);
        let result = normalize_ld_json(root, "https://example.com");
        assert!(result.is_err());
    }

    // ============================================================================
    // extract_ld() Tests
    // ============================================================================

    #[test]
    fn test_extract_ld_no_script() {
        let html = Html::parse_document("<html><body>No recipe here</body></html>");
        let result = extract_ld(html);
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_ld_with_script() {
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

    // ============================================================================
    // scrape_from_ld_json() Tests
    // ============================================================================

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

    #[rstest]
    #[case::invalid_json("not valid json at all")]
    #[case::empty("")]
    #[case::incomplete("{\"name\":")]
    fn test_scrape_from_ld_json_invalid(#[case] json: &str) {
        let result = scrape_from_ld_json(json, "https://example.com");
        assert!(result.is_err());
    }

    #[test]
    fn test_instruction_wrapper_c() {
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

    // ============================================================================
    // parse_yield_string() Tests
    // ============================================================================

    #[rstest]
    // Parser normalizes "servings" to singular "serving"
    #[case::servings_plural("4 servings", Some(RecipeYield { value: 4.0, unit: "serving".to_string() }), Some(4))]
    #[case::servings_singular("1 serving", Some(RecipeYield { value: 1.0, unit: "serving".to_string() }), Some(1))]
    // Unknown units become "whole"
    #[case::pancakes("12 pancakes", Some(RecipeYield { value: 12.0, unit: "whole".to_string() }), None)]
    #[case::cookies("24 cookies", Some(RecipeYield { value: 24.0, unit: "whole".to_string() }), None)]
    // Plain numbers are parsed by parse_amount as "whole", not by the fallback
    #[case::plain_number("12", Some(RecipeYield { value: 12.0, unit: "whole".to_string() }), None)]
    #[case::empty("", None, None)]
    #[case::invalid_text("invalid", None, None)]
    fn test_parse_yield_string(
        #[case] input: &str,
        #[case] expected_yield: Option<RecipeYield>,
        #[case] expected_servings: Option<u32>,
    ) {
        let (recipe_yield, servings) = parse_yield_string(input);
        assert_eq!(recipe_yield, expected_yield);
        assert_eq!(servings, expected_servings);
    }

    // ============================================================================
    // extract_yield_from_wrapper() Tests
    // ============================================================================

    #[rstest]
    #[case::string_wrapper(
        RecipeYieldWrapper::String("4 servings".to_string()),
        Some(RecipeYield { value: 4.0, unit: "serving".to_string() }),
        Some(4)
    )]
    #[case::number_wrapper(
        RecipeYieldWrapper::Number(4.0),
        Some(RecipeYield { value: 4.0, unit: "servings".to_string() }),
        Some(4)
    )]
    #[case::string_array_first_valid(
        RecipeYieldWrapper::StringArray(vec!["6 servings".to_string()]),
        Some(RecipeYield { value: 6.0, unit: "serving".to_string() }),
        Some(6)
    )]
    #[case::string_array_skips_invalid(
        RecipeYieldWrapper::StringArray(vec!["invalid".to_string(), "8 pancakes".to_string()]),
        Some(RecipeYield { value: 8.0, unit: "whole".to_string() }),
        None
    )]
    #[case::string_array_empty(
        RecipeYieldWrapper::StringArray(vec![]),
        None,
        None
    )]
    #[case::number_array(
        RecipeYieldWrapper::NumberArray(vec![8.0, 2.0]),
        Some(RecipeYield { value: 8.0, unit: "servings".to_string() }),
        Some(8)
    )]
    #[case::number_array_empty(
        RecipeYieldWrapper::NumberArray(vec![]),
        None,
        None
    )]
    fn test_extract_yield_from_wrapper(
        #[case] wrapper: RecipeYieldWrapper,
        #[case] expected_yield: Option<RecipeYield>,
        #[case] expected_servings: Option<u32>,
    ) {
        let (recipe_yield, servings) = extract_yield_from_wrapper(&wrapper);
        assert_eq!(recipe_yield, expected_yield);
        assert_eq!(servings, expected_servings);
    }
}
