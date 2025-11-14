use scraper::{Html, Selector};

use crate::{ScrapeError, ScrapedRecipe};

pub fn scrape_from_html(dom: Html, url: &str) -> Result<ScrapedRecipe, ScrapeError> {
    let title_selector = Selector::parse("title")
        .map_err(|e| ScrapeError::Parse(format!("Invalid title selector: {e:?}")))?;
    let title = match dom.select(&title_selector).next() {
        Some(x) => x.inner_html(),
        None => "".to_string(),
    };
    // smitten kitchen
    let ingredient_selector = Selector::parse("li.jetpack-recipe-ingredient")
        .map_err(|e| ScrapeError::Parse(format!("Invalid ingredient selector: {e:?}")))?;
    let ingredients = dom
        .select(&ingredient_selector)
        .map(|i| i.text().collect::<Vec<_>>().join(""))
        .collect::<Vec<String>>();

    let ul_selector = Selector::parse(r#"div.jetpack-recipe-directions"#)
        .map_err(|e| ScrapeError::Parse(format!("Invalid directions selector: {e:?}")))?;

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

    let image_selector = Selector::parse(r#"meta[property="og:image"]"#)
        .map_err(|e| ScrapeError::Parse(format!("Invalid image selector: {e:?}")))?;
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
    })
}
