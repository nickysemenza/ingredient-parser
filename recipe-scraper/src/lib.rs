use ingredient::{
    rich_text::{Rich, RichParser},
    Ingredient, IngredientParser,
};
use scraper::{Html, Selector};

use serde::{Deserialize, Serialize};
use serde_json::Value;
mod ld_schema;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum ScrapeError {
    #[error("could not find fetch `{0}`")]
    Http(String),
    #[error("could not find ld+json for `{0}`")]
    NoLDJSON(String),
    #[error("could not find recipe in ld ld+json for `{0}`")]
    LDJSONMissingRecipe(String),
    #[error("could not deserialize `{0}`")]
    Deserialize(#[from] serde_json::Error),
    #[error("could not parse `{0}`")]
    Parse(String),
}
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ScrapedRecipe {
    pub ingredients: Vec<String>,
    pub instructions: Vec<String>,
    pub name: String,
    pub url: String,
    pub image: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
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
            .map(|i| {
                let res = ip.clone().from_str(i);
                res
            })
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
    let dom = Html::parse_document(body);
    let res = match extract_ld(dom.clone()) {
        Ok(ld_schema) => scrape_from_json(ld_schema.as_str(), url),
        Err(e) => match e {
            ScrapeError::NoLDJSON(_) => scrape_from_html(dom),
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
    i.replace("&nbsp;", " ").replace("\n", " ")
}
fn scrape_from_json(json: &str, url: &str) -> Result<ScrapedRecipe, ScrapeError> {
    normalize_ld_json(parse_ld_json(json.to_owned())?, url)
}

#[tracing::instrument]
fn normalize_root_recipe(ld_schema: ld_schema::RootRecipe, url: &str) -> ScrapedRecipe {
    ScrapedRecipe {
        ingredients: ld_schema.recipe_ingredient,
        instructions: match ld_schema.recipe_instructions {
            ld_schema::InstructionWrapper::A(a) => a.into_iter().map(|i| i.text).collect(),
            ld_schema::InstructionWrapper::B(b) => b
                .clone()
                .pop()
                .unwrap()
                .item_list_element
                .iter()
                .map(|i| i.text.clone().unwrap())
                .collect(),
            ld_schema::InstructionWrapper::C(c) => {
                let selector = Selector::parse("p").unwrap();

                let foo = Html::parse_fragment(c.as_ref())
                    .select(&selector)
                    .map(|i| i.text().collect::<Vec<_>>().join(""))
                    .collect::<Vec<_>>();
                foo
                // c.split("</p>\n, <p>").map(|s| s.into()).collect()
            }
            ld_schema::InstructionWrapper::D(d) => {
                d[0].clone().into_iter().map(|i| i.text).collect()
            }
        },

        name: ld_schema.name,
        url: url.to_string(),
        image: match ld_schema.image {
            Some(image) => match image {
                ld_schema::ImageOrList::URL(i) => Some(i),
                ld_schema::ImageOrList::List(l) => Some(l[0].url.clone()),
                ld_schema::ImageOrList::URLList(i) => Some(i[0].clone()),
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
        ld_schema::Root::Recipe(ld_schema) => Ok(normalize_root_recipe(ld_schema, url)),
        ld_schema::Root::Graph(g) => {
            let recipe = g.graph.iter().find_map(|d| match d {
                ld_schema::Graph::Recipe(a) => Some(a.to_owned()),
                _ => None,
            });
            match recipe {
                Some(r) => Ok(normalize_root_recipe(r, url)),
                None => Err(ScrapeError::LDJSONMissingRecipe(
                    "failed to find recipe in ld json".to_string(),
                )),
            }
        }
    }
}
fn scrape_from_html(dom: Html) -> Result<ScrapedRecipe, ScrapeError> {
    // smitten kitchen
    let ingredient_selector = Selector::parse("li.jetpack-recipe-ingredient").unwrap();
    let ingredients = dom
        .select(&ingredient_selector)
        .map(|i| i.text().collect::<Vec<_>>().join(""))
        .collect::<Vec<String>>();

    let ul_selector = Selector::parse(r#"div.jetpack-recipe-directions"#).unwrap();

    let foo = match dom.select(&ul_selector).next() {
        Some(x) => x,
        None => return Err(ScrapeError::Parse("no ld json or parsed html".to_string())),
    };

    let instructions = foo
        .text()
        .collect::<Vec<_>>()
        .join("")
        .split("\n")
        .map(|s| s.into())
        .collect::<Vec<String>>();

    Ok(dbg!(ScrapedRecipe {
        ingredients,
        instructions,
        name: "".to_string(),
        url: "".to_string(),
        image: None,
    }))
    // Err(ScrapeError::Parse("foo".to_string()))
}
fn extract_ld(dom: Html) -> Result<String, ScrapeError> {
    let selector = match Selector::parse("script[type='application/ld+json']") {
        Ok(s) => s,
        Err(e) => return Err(ScrapeError::Parse(format!("{:?}", e))),
    };

    let element = match dom.select(&selector).next() {
        Some(e) => e,
        None => {
            return Err(ScrapeError::NoLDJSON(
                dom.root_element().html().chars().take(40).collect(),
            ))
        }
    };

    Ok(element.inner_html())
}
fn parse_ld_json(json: String) -> Result<ld_schema::Root, ScrapeError> {
    let json = json.as_str();
    let _raw = serde_json::from_str::<Value>(json)?;
    // dbg!(_raw);
    // tracing::info!("raw json: {:#?}", raw);
    let v: ld_schema::Root = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(e) => return Err(ScrapeError::Deserialize(e)),
    };

    return Ok(v);
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use crate::ld_schema::InstructionWrapper;

    macro_rules! include_testdata {
        ($x:expr) => {
            include_str!(concat!("../test_data/", $x))
        };
    }

    fn scrape_url(url: &str) -> Result<super::ScrapedRecipe, super::ScrapeError> {
        let binding = get_testdata();
        let html = binding.get(url);
        assert!(html.is_some(), "no test data for {}", url);
        let res = super::scrape(html.unwrap(), url);
        res
    }
    fn get_testdata() -> HashMap<String, String> {
        HashMap::from([
            (
                "https://cooking.nytimes.com/recipes/1015819-chocolate-chip-cookies".to_string(),
                include_testdata!("nytimes_chocolate_chip_cookies.html").to_string(),
            ),
            (
                "http://www.seriouseats.com/recipes/2011/08/grilled-naan-recipe.html".to_string(),
                include_testdata!("seriouseats_grilled_naan.html").to_string(),
            ),
            (
                "https://www.kingarthurbaking.com/recipes/pretzel-focaccia-recipe".to_string(),
                include_testdata!("kingarthurbaking_pretzel-focaccia-recipe.html").to_string(),
            ),
            (
                "https://smittenkitchen.com/2018/04/crispy-tofu-pad-thai/".to_string(),
                include_testdata!("smittenkitchen_crispy-tofu-pad-thai.html").to_string(),
            ),
            (
                "http://cooking.nytimes.com/recipes/1017060-doughnuts".to_string(),
                include_testdata!("nytimes_doughnuts.html").to_string(),
            ),
            (
                "https://cooking.nytimes.com/recipes/1019232-toll-house-chocolate-chip-cookies"
                    .to_string(),
                include_testdata!("nytimes_toll-house-chocolate-chip-cookies.html").to_string(),
            ),
        ])
    }

    #[test]
    fn scrape_from_live() {
        let res = scrape_url("http://cooking.nytimes.com/recipes/1017060-doughnuts").unwrap();
        assert_eq!(res.ingredients.len(), 8);
    }

    #[test]
    fn scrape_from_cache() {
        let res = scrape_url("https://cooking.nytimes.com/recipes/1015819-chocolate-chip-cookies")
            .unwrap();
        assert_eq!(res.ingredients.len(), 12);

        let res = scrape_url(
            "https://cooking.nytimes.com/recipes/1019232-toll-house-chocolate-chip-cookies",
        )
        .unwrap();
        assert_eq!(res.ingredients[0], "2 1/4 cups all-purpose flour");

        let res = scrape_url("http://www.seriouseats.com/recipes/2011/08/grilled-naan-recipe.html")
            .unwrap();
        assert_eq!(res.ingredients.len(), 6);

        let res =
            scrape_url("https://www.kingarthurbaking.com/recipes/pretzel-focaccia-recipe").unwrap();
        assert_eq!(res.ingredients.len(), 14);
        assert_eq!(res.instructions[0], "To make the starter: Mix the water and yeast. Weigh your flour; or measure it by gently spooning it into a cup, then sweeping off any excess. Add the flour, stirring until the flour is incorporated. The starter will be paste-like; it won't form a ball.");
    }
    #[test]
    fn scrape_from_cache_html() {
        let res = scrape_url("https://smittenkitchen.com/2018/04/crispy-tofu-pad-thai/").unwrap();
        assert_eq!(res.ingredients.len(), 17);
        assert_eq!(res.instructions.len(), 16);
    }
    #[test]
    fn json() {
        assert_eq!(
            crate::parse_ld_json(include_testdata!("empty.json").to_string()).unwrap(),
            crate::ld_schema::Root::Recipe(crate::ld_schema::RootRecipe {
                context: None,
                name: "".to_string(),
                image: None,
                recipe_ingredient: vec![],
                recipe_instructions: InstructionWrapper::A(vec![]),
            })
        );
        let r = crate::scrape_from_json(
            include_testdata!("diningwithskyler_carbone-spicy-rigatoni-vodka.json"),
            "a".as_ref(),
        )
        .unwrap();
        assert_eq!(r.ingredients.len(), 11);
        assert_eq!(r.instructions.len(), 9); // todo

        let r = crate::scrape_from_json(
            include_testdata!("thewoksoflife_vietnamese-rice-noodle-salad-chicken.json"),
            "a".as_ref(),
        )
        .unwrap();
        assert_eq!(r.instructions.len(), 5);
        assert_eq!(r.ingredients.len(), 22);
    }

    #[test]
    fn handle_no_ldjson() {
        assert!(matches!(
            crate::scrape(include_testdata!("missing.html"), "https://missing.com",).unwrap_err(),
            crate::ScrapeError::Parse(_)
        ));

        assert!(matches!(
            crate::scrape(include_testdata!("malformed.html"), "https://malformed.com",)
                .unwrap_err(),
            crate::ScrapeError::Parse(_)
        ));
    }
}
