use std::error::Error;

use scraper::{Html, Selector};

use serde_json::Value;
mod schema;

pub struct ScrapedRecipe {
    pub ingredients: Vec<String>,
    pub instructions: Vec<String>,
}
// inspiration
// https://github.com/pombadev/sunny/blob/main/src/lib/spider.rs
// https://github.com/megametres/recettes-api/blob/dev/src/html_parser/mod.rs

pub fn scrape_url(url: &str) -> Result<ScrapedRecipe, Box<dyn Error>> {
    let body = fetch_html(url).unwrap();
    scrape(body.as_ref())
}
pub fn scrape(body: &str) -> Result<ScrapedRecipe, Box<dyn Error>> {
    let dom = Html::parse_document(body);
    let ld_schema = parse(dom);
    Ok(ScrapedRecipe {
        ingredients: ld_schema.recipe_ingredient,
        instructions: vec![],
        // instructions: ld_schema.recipe_instructions,
    })
}

fn fetch_html(url: &str) -> Result<String, Box<dyn Error>> {
    let resp = reqwest::blocking::get(url)?;
    let body = resp.text()?;
    Ok(body)
}
fn parse(dom: Html) -> schema::Root {
    let selector = Selector::parse("script[type='application/ld+json']").unwrap();

    let element = dom.select(&selector).next().unwrap();
    let json = element.inner_html();
    parse_ld_json(json)
}
fn parse_ld_json(json: String) -> schema::Root {
    let json = json.as_str();
    dbg!(serde_json::from_str::<Value>(json).unwrap());
    let v: schema::Root = serde_json::from_str(dbg!(json)).unwrap();

    // format!("res{}", json);
    // dbg!(v.recipe_ingredient);
    return v;
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
    fn it_works_file() {
        let res = crate::scrape(include_str!(
            "../test_data/nytimes_chocolate_chip_cookies.html"
        ))
        .unwrap();
        assert_eq!(res.ingredients.len(), 12);

        let result = 2 + 2;
        assert_eq!(result, 4);
    }
    #[test]
    fn it_works_file_se() {
        let res =
            crate::scrape(include_str!("../test_data/seriouseats_grilled_naan.html")).unwrap();
        assert_eq!(res.ingredients.len(), 6);

        let result = 2 + 2;
        assert_eq!(result, 4);
    }
    #[test]
    fn json() {
        assert_eq!(
            crate::parse_ld_json(
                r#"{"@context": "","recipeIngredient": [],"recipeInstructions": []}"#.to_string()
            ),
            crate::schema::Root::default()
        );
        assert_eq!(
            crate::parse_ld_json(
                "\t\t\t\t{\"@context\": \"\",\"recipeIngredient\": [],\"recipeInstructions\": []}"
                    .to_string()
            ),
            crate::schema::Root::default()
        );
    }
}
