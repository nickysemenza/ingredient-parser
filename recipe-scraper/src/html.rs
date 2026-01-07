use scraper::{Html, Selector};

use crate::{ScrapeError, ScrapedRecipe};

fn parse_selector(selector: &str) -> Result<Selector, ScrapeError> {
    Selector::parse(selector)
        .map_err(|e| ScrapeError::Parse(format!("invalid selector '{selector}': {e}")))
}

pub fn scrape_from_html(dom: Html, url: &str) -> Result<ScrapedRecipe, ScrapeError> {
    let title_selector = parse_selector("title")?;
    let title = match dom.select(&title_selector).next() {
        Some(x) => x.inner_html(),
        None => "".to_string(),
    };
    // smitten kitchen
    let ingredient_selector = parse_selector("li.jetpack-recipe-ingredient")?;
    let ingredients = dom
        .select(&ingredient_selector)
        .map(|i| i.text().collect::<Vec<_>>().join(""))
        .collect::<Vec<String>>();

    let ul_selector = parse_selector(r#"div.jetpack-recipe-directions"#)?;

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

    let image_selector = parse_selector(r#"meta[property="og:image"]"#)?;
    let image = dom
        .select(&image_selector)
        .next()
        .and_then(|i| i.value().attr("content").map(|s| s.to_string()));

    Ok(ScrapedRecipe {
        ingredients,
        instructions,
        name: title,
        url: url.to_string(),
        image,
        recipe_yield: None, // HTML fallback doesn't have yield data
        servings: None,
    })
    // Err(ScrapeError::Parse("foo".to_string()))
}
