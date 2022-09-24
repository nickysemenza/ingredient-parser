#![allow(deprecated)]

mod utils;

use ingredient::{self, rich_text::RichParser, Amount, IngredientParser};
use wasm_bindgen::prelude::*;

// When the `wee_alloc` feature is enabled, use `wee_alloc` as the global
// allocator.
#[cfg(feature = "wee_alloc")]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;
#[wasm_bindgen]
pub fn parse_ingredient(input: &str) -> IIngredient {
    utils::set_panic_hook();
    let si = ingredient::from_str(input);
    JsValue::from_serde(&si).unwrap().into()
}
#[wasm_bindgen]
pub fn parse_rich_text(r: String, ings: &JsValue) -> Result<RichItems, JsValue> {
    utils::set_panic_hook();
    let ings2: Vec<String> = ings.into_serde().unwrap();
    let rtp = RichParser {
        ingredient_names: ings2,
        ip: IngredientParser::new(true),
    };
    match rtp.parse(r.as_str()) {
        Ok(r) => Ok(JsValue::from_serde(&r).unwrap().into()),
        Err(e) => Err(JsValue::from_str(&e.to_string())),
    }
}

#[wasm_bindgen]
pub fn format_amount(amount: &IAmount) -> String {
    utils::set_panic_hook();
    let a1: Result<Amount, _> = amount.into_serde();
    match a1 {
        Ok(a) => format!("{}", a),
        Err(e) => {
            format!("failed to format {:#?}: {:?}", amount, e)
        }
    }
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "Ingredient")]
    pub type IIngredient;
    #[wasm_bindgen(typescript_type = "Amount")]
    #[derive(Debug)]
    pub type IAmount;
    #[wasm_bindgen(typescript_type = "Amount[]")]
    pub type IAmounts;
    #[wasm_bindgen(typescript_type = "RichItem[]")]
    pub type RichItems;
}

#[wasm_bindgen(typescript_custom_section)]
const ITEXT_STYLE: &'static str = r#"
interface Ingredient {
    amounts: Amount[];
    modifier?: string;
    name: string;
}
interface Amount {
  unit: string;
  value: number;
  upper_value?: number;
}
export type RichItem =
  | { kind: "Text"; value: string }
  | { kind: "Ing"; value: string }
  | { kind: "Amount"; value: Amount[] }
"#;
