use scraper::{Html, Selector};

use serde_json::Value;
mod http_utils;
mod ld_schema;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum ScrapeError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("could not find ld+json for `{0}`")]
    NoLDJSON(String),
    #[error("could not deserialize `{0}`")]
    Deserialize(#[from] serde_json::Error),
    #[error("could not parse `{0}`")]
    Parse(String),
}
#[derive(Debug)]
pub struct ScrapedRecipe {
    pub ingredients: Vec<String>,
    pub instructions: Vec<String>,
    pub name: String,
    pub url: String,
    pub image: Option<String>,
}
// inspiration
// https://github.com/pombadev/sunny/blob/main/src/lib/spider.rs
// https://github.com/megametres/recettes-api/blob/dev/src/html_parser/mod.rs

#[derive(Debug)]
pub struct Scraper {
    client: reqwest_middleware::ClientWithMiddleware,
}
impl Scraper {
    pub fn new() -> Self {
        return Scraper {
            client: http_utils::http_client(),
        };
    }
    #[tracing::instrument(name = "scrape_url")]
    pub async fn scrape_url(&self, url: &str) -> Result<ScrapedRecipe, ScrapeError> {
        let body = self.fetch_html(url).await?;
        scrape(body.as_ref(), url)
    }

    #[tracing::instrument]
    async fn fetch_html(&self, url: &str) -> Result<String, ScrapeError> {
        let r = match self
            .client
            .get(url)
            .header("user-agent", "recipe")
            .send()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                return Err(match e {
                    reqwest_middleware::Error::Middleware(e) => panic!("{}", e),
                    reqwest_middleware::Error::Reqwest(e) => ScrapeError::Http(e),
                })
            }
        };
        if !r.status().is_success() {
            let e = Err(ScrapeError::Http(r.error_for_status_ref().unwrap_err()));
            dbg!(r.text().await?);
            return e;
        }
        Ok(r.text().await?)
    }
}
pub fn scrape(body: &str, url: &str) -> Result<ScrapedRecipe, ScrapeError> {
    let dom = Html::parse_document(body);
    let ld_schema = extract_ld(dom)?;
    scrape_from_json(ld_schema.as_str(), url)
}

fn scrape_from_json(json: &str, url: &str) -> Result<ScrapedRecipe, ScrapeError> {
    normalize_ld_json(parse_ld_json(json.to_owned())?, url)
}

#[tracing::instrument]
fn normalize_root_recipe(ld_schema: ld_schema::RootRecipe, url: &str) -> ScrapedRecipe {
    ScrapedRecipe {
        ingredients: ld_schema.recipe_ingredient,
        instructions: match ld_schema.recipe_instructions {
            ld_schema::RecipeInstructionFOO::A(a) => a.into_iter().map(|i| i.text).collect(),
            ld_schema::RecipeInstructionFOO::B(b) => b
                .clone()
                .pop()
                .unwrap()
                .item_list_element
                .iter()
                .map(|i| i.text.clone().unwrap())
                .collect(),
        },

        name: ld_schema.name,
        url: url.to_string(),
        image: match ld_schema.image {
            Some(image) => match image {
                ld_schema::ImageOrList::URL(i) => Some(i),
                ld_schema::ImageOrList::List(l) => Some(l[0].url.clone()),
                ld_schema::ImageOrList::URLList(i) => Some(i[0].clone()),
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
                None => Err(ScrapeError::NoLDJSON(
                    "failed to find recipe in ld json".to_string(),
                )),
            }
        }
    }
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
    dbg!(_raw);
    // tracing::info!("raw json: {:#?}", raw);
    let v: ld_schema::Root = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(e) => return Err(ScrapeError::Deserialize(e)),
    };

    return Ok(v);
}

#[cfg(test)]
mod tests {
    use crate::{ld_schema::RecipeInstructionFOO, Scraper};

    macro_rules! include_testdata {
        ($x:expr) => {
            include_str!(concat!("../test_data/", $x))
        };
    }

    #[tokio::test]
    async fn it_works() {
        let res = Scraper::new()
            .scrape_url("https://cooking.nytimes.com/recipes/1015819-chocolate-chip-cookies")
            .await
            .unwrap();
        assert_eq!(res.ingredients.len(), 12);
    }
    #[tokio::test]
    async fn it_works_live() {
        let res = Scraper::new()
            .scrape_url("https://diningwithskyler.com/carbone-spicy-rigatoni-vodka/")
            .await
            .unwrap();
        assert_eq!(res.ingredients.len(), 11);
    }

    #[test]
    fn it_works_file() {
        let res = crate::scrape(
            include_testdata!("nytimes_chocolate_chip_cookies.html"),
            "https://cooking.nytimes.com/recipes/1015819-chocolate-chip-cookies",
        )
        .unwrap();
        assert_eq!(res.ingredients.len(), 12);
    }
    #[test]
    fn it_works_file_se() {
        let res = crate::scrape(
            include_testdata!("seriouseats_grilled_naan.html"),
            "http://www.seriouseats.com/recipes/2011/08/grilled-naan-recipe.html",
        )
        .unwrap();
        assert_eq!(res.ingredients.len(), 6);
    }
    #[test]
    fn json() {
        assert_eq!(
            crate::parse_ld_json(include_testdata!("empty.json").to_string()).unwrap(),
            crate::ld_schema::Root::Recipe(crate::ld_schema::RootRecipe {
                context: "".to_string(),
                name: "".to_string(),
                image: None,
                recipe_ingredient: vec![],
                recipe_instructions: RecipeInstructionFOO::A(vec![]),
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
            crate::ScrapeError::NoLDJSON(_)
        ));

        assert!(matches!(
            crate::scrape(include_testdata!("malformed.html"), "https://malformed.com",)
                .unwrap_err(),
            crate::ScrapeError::NoLDJSON(_)
        ));
    }
}
