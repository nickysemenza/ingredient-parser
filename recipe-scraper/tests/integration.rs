use pretty_assertions::assert_eq;
use recipe_scraper::ld_json;
use recipe_scraper::{scrape, ParsedRecipe, ScrapeError, ScrapedRecipe};
use std::collections::HashMap;
macro_rules! include_testdata {
    ($x:expr) => {
        include_str!(concat!("../test_data/", $x))
    };
}

fn scrape_url(url: &str) -> Result<ScrapedRecipe, ScrapeError> {
    let binding = get_testdata();
    let html = binding.get(url);
    assert!(html.is_some(), "no test data for {url}");

    scrape(html.unwrap(), url)
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
        (
            "https://cooking.nytimes.com/recipes/1022674-chewy-gingerbread-cookies".to_string(),
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
    let res =
        scrape_url("https://cooking.nytimes.com/recipes/1015819-chocolate-chip-cookies").unwrap();
    assert_eq!(res.ingredients.len(), 12);

    let res =
        scrape_url("https://cooking.nytimes.com/recipes/1019232-toll-house-chocolate-chip-cookies")
            .unwrap();
    assert_eq!(res.ingredients[0], "2 1/4 cups all-purpose flour");
    let scraped = res.parse();
    // testdata from
    // ❯ cargo run --bin food_cli scrape https://cooking.nytimes.com/recipes/1019232-toll-house-chocolate-chip-cookies --json --parse
    let raw = serde_json::from_str::<ParsedRecipe>(include_testdata!(
        "nytimes_toll-house-chocolate-chip-cookies_parsed.json"
    ))
    .unwrap();
    assert_eq!(scraped, raw);

    let res =
        scrape_url("http://www.seriouseats.com/recipes/2011/08/grilled-naan-recipe.html").unwrap();
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
    assert_eq!(res.name, "crispy tofu pad thai – smitten kitchen");
    assert_eq!(res.image, Some("https://i1.wp.com/smittenkitchen.com/wp-content/uploads//2018/04/crispy-tofu-pad-thai.jpg?fit=1200%2C800&ssl=1".to_string()));
}
#[test]
fn json() {
    let r = ld_json::scrape_from_ld_json(
        include_testdata!("diningwithskyler_carbone-spicy-rigatoni-vodka.json"),
        "a",
    )
    .unwrap();
    assert_eq!(r.ingredients.len(), 11);
    assert_eq!(r.instructions.len(), 9); // todo

    let r = ld_json::scrape_from_ld_json(
        include_testdata!("thewoksoflife_vietnamese-rice-noodle-salad-chicken.json"),
        "a",
    )
    .unwrap();
    assert_eq!(r.instructions.len(), 5);
    assert_eq!(r.ingredients.len(), 22);

    let r =
        ld_json::scrape_from_ld_json(include_testdata!("seriouseats_pan_pizza.json"), "a").unwrap();
    assert_eq!(r.instructions.len(), 7);
    assert_eq!(r.ingredients.len(), 10);

    let r = ld_json::scrape_from_ld_json(
        include_testdata!("justonecookbook_chicken-katsu-don.json"),
        "a",
    )
    .unwrap();
    assert_eq!(r.instructions.len(), 7);
    assert_eq!(r.ingredients.len(), 17);
}
#[test]
fn handle_no_ldjson() {
    assert!(matches!(
        scrape(include_testdata!("missing.html"), "https://missing.com",).unwrap_err(),
        ScrapeError::Parse(_)
    ));

    assert!(matches!(
        scrape(include_testdata!("malformed.html"), "https://malformed.com",).unwrap_err(),
        ScrapeError::Parse(_)
    ));
}

#[test]
fn test_scrape_chefsteps() {
    let r = scrape(
        include_testdata!("chefsteps_rich-and-moist-cornbread.json"),
        "https://www.chefsteps.com/activities/rich-and-moist-cornbread",
    )
    .unwrap();
    dbg!(r);
    // assert_eq!(r.instructions.len(), 7);
    // assert_eq!(r.ingredients.len(), 17);
}
