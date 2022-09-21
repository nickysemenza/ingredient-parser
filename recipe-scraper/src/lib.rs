use scraper::{Html, Selector};

use serde_json::Value;
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
    // #[error("invalid header (expected {expected:?}, found {found:?})")]
    // InvalidHeader { expected: String, found: String },
    // #[error("unknown data store error")]
    // Unknown,
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

pub fn scrape_url(url: &str) -> Result<ScrapedRecipe, ScrapeError> {
    let body = fetch_html(url).unwrap();
    scrape(body.as_ref(), url)
}
pub fn scrape(body: &str, url: &str) -> Result<ScrapedRecipe, ScrapeError> {
    let dom = Html::parse_document(body);
    let ld_schema = parse(dom)?;
    Ok(ScrapedRecipe {
        ingredients: ld_schema.recipe_ingredient,
        instructions: ld_schema
            .recipe_instructions
            .iter()
            .map(|i| match i {
                ld_schema::RecipeInstruction::A(a) => a.text.clone(),
                ld_schema::RecipeInstruction::B(b) => b.item_list_element[0].text.clone(),
            })
            .collect(),
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
    })
}

fn fetch_html(url: &str) -> Result<String, ScrapeError> {
    let resp = match reqwest::blocking::get(url) {
        Ok(r) => r,
        Err(e) => return Err(ScrapeError::Http(e)),
    };
    let body = resp.text()?;
    Ok(body)
}
fn parse(dom: Html) -> Result<ld_schema::LDJSONRoot, ScrapeError> {
    let selector = Selector::parse("script[type='application/ld+json']").unwrap();

    let element = match dom.select(&selector).next() {
        Some(e) => e,
        None => return Err(ScrapeError::NoLDJSON(dom.root_element().html())),
    };

    let json = element.inner_html();
    parse_ld_json(json)
}
fn parse_ld_json(json: String) -> Result<ld_schema::LDJSONRoot, ScrapeError> {
    let json = json.as_str();
    dbg!(serde_json::from_str::<Value>(json).unwrap());
    let v: ld_schema::LDJSONRoot = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(e) => return Err(ScrapeError::Deserialize(e)),
    };

    return Ok(v);
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let res =
            crate::scrape_url("https://cooking.nytimes.com/recipes/1015819-chocolate-chip-cookies")
                .unwrap();
        assert_eq!(res.ingredients.len(), 12);

        let result = 2 + 2;
        assert_eq!(result, 4);
    }
    #[test]
    fn it_works_live() {
        let res = crate::scrape_url("https://diningwithskyler.com/carbone-spicy-rigatoni-vodka/")
            .unwrap();
        assert_eq!(res.ingredients.len(), 11);
    }

    #[test]
    fn it_works_file() {
        let res = crate::scrape(
            include_str!("../test_data/nytimes_chocolate_chip_cookies.html"),
            "https://cooking.nytimes.com/recipes/1015819-chocolate-chip-cookies",
        )
        .unwrap();
        assert_eq!(res.ingredients.len(), 12);
    }
    #[test]
    fn it_works_file_se() {
        let res = crate::scrape(
            include_str!("../test_data/seriouseats_grilled_naan.html"),
            "http://www.seriouseats.com/recipes/2011/08/grilled-naan-recipe.html",
        )
        .unwrap();
        assert_eq!(res.ingredients.len(), 6);
    }
    #[test]
    fn json() {
        assert_eq!(
            crate::parse_ld_json(include_str!("../test_data/empty.json").to_string()).unwrap(),
            crate::ld_schema::LDJSONRoot::default()
        );
        assert_eq!(
            crate::parse_ld_json(
                include_str!("../test_data/diningwithskyler_carbone-spicy-rigatoni-vodka.json")
                    .to_string()
            )
            .unwrap()
            .recipe_ingredient
            .len(),
            11
        );
    }

    #[test]
    fn handle_no_ldjson() {
        let err = crate::scrape(
            include_str!("../test_data/missing.html"),
            "https://missing.com",
        )
        .unwrap_err();

        assert!(matches!(err, crate::ScrapeError::NoLDJSON(_)));
    }
}
