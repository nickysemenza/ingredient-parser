#![allow(deprecated)]
// WASM bindings use unwrap for JsValue serialization which should not fail for well-formed data
#![allow(clippy::unwrap_used)]

use ingredient::{self, rich_text::RichParser, unit::Measure, IngredientParser};
use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    // print pretty errors in wasm https://github.com/rustwasm/console_error_panic_hook
    // This is not needed for tracing_wasm to work, but it is a common tool for getting proper error line numbers for panics.
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();

    // Add this line:
    tracing_wasm::set_as_global_default();

    Ok(())
}

// When the `wee_alloc` feature is enabled, use `wee_alloc` as the global
// allocator.
#[cfg(feature = "wee_alloc")]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;
#[wasm_bindgen]
pub fn parse_ingredient(input: &str) -> IIngredient {
    let si = ingredient::from_str(input);
    JsValue::from_serde(&si).unwrap().into()
}
#[wasm_bindgen]
pub fn parse_rich_text(r: String, ings: &JsValue) -> Result<RichItems, JsValue> {
    let ings2: Vec<String> = ings.into_serde().unwrap();
    let rtp = RichParser {
        ingredient_names: ings2,
        ip: IngredientParser::new().with_rich_text(),
    };
    match rtp.parse(r.as_str()) {
        Ok(r) => Ok(JsValue::from_serde(&r).unwrap().into()),
        Err(e) => Err(JsValue::from_str(&e)),
    }
}

#[wasm_bindgen]
pub fn format_amount(amount: &IMeasure) -> String {
    let a1: Result<Measure, _> = amount.into_serde();
    match a1 {
        Ok(a) => format!("{a}"),
        Err(e) => {
            format!("failed to format {amount:#?}: {e:?}")
        }
    }
}

#[wasm_bindgen]
pub fn scrape(body: String, url: String) -> Result<IScrapedRecipe, JsValue> {
    match recipe_scraper::scrape(body.as_str(), &url) {
        Ok(r) => Ok(JsValue::from_serde(&r).unwrap().into()),
        Err(x) => Err(JsValue::from_str(&format!("failed to get recipe: {x:?}"))),
    }
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "Ingredient")]
    #[derive(Debug)]
    pub type IIngredient;
    #[wasm_bindgen(typescript_type = "Measure")]
    #[derive(Debug)]
    pub type IMeasure;
    #[wasm_bindgen(typescript_type = "Measure[]")]
    pub type IMeasures;
    #[wasm_bindgen(typescript_type = "RichItem[]")]
    pub type RichItems;
    #[wasm_bindgen(typescript_type = "ScrapedRecipe")]
    pub type IScrapedRecipe;
}

#[wasm_bindgen(typescript_custom_section)]
const ITEXT_STYLE: &'static str = r#"
interface Ingredient {
    amounts: Measure[];
    modifier?: string;
    name: string;
}
interface Measure {
  unit: string;
  value: number;
  upper_value?: number;
}

interface ScrapedRecipe {
    image: string;
    ingredients: string[];
    instructions: string[];
    name: string;
    url: string;
}

export type RichItem =
  | { kind: "Text"; value: string }
  | { kind: "Ing"; value: string }
  | { kind: "Measure"; value: Measure[] }
"#;
