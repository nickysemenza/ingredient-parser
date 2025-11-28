use scraper::{Html, Selector};

use crate::{ScrapeError, ScrapedRecipe};

pub fn scrape_from_html(dom: Html, url: &str) -> Result<ScrapedRecipe, ScrapeError> {
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

    Ok(ScrapedRecipe {
        ingredients,
        instructions,
        name: title,
        url: url.to_string(),
        image,
    })
    // Err(ScrapeError::Parse("foo".to_string()))
}
