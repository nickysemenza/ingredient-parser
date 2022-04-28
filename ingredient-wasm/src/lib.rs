mod utils;

use ingredient::{self, IngredientParser};
use wasm_bindgen::prelude::*;

// When the `wee_alloc` feature is enabled, use `wee_alloc` as the global
// allocator.
#[cfg(feature = "wee_alloc")]
#[global_allocator]
static ALLOC: wee_alloc::WeeAlloc = wee_alloc::WeeAlloc::INIT;

#[wasm_bindgen]
pub fn parse_ingredient(input: &str) -> IIngredient {
    utils::set_panic_hook();
    let parser = IngredientParser::new();
    let si = parser.parse_ingredient(input).unwrap().1;
    JsValue::from_serde(&si).unwrap().into()
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(typescript_type = "Ingredient")]
    pub type IIngredient;
    #[wasm_bindgen(typescript_type = "Amount")]
    pub type IAmount;
    #[wasm_bindgen(typescript_type = "Amount[]")]
    pub type IAmounts;
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

"#;
