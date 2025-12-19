#![cfg(target_arch = "wasm32")]

use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
fn test_parse_ingredient_simple() {
    let result = ingredient_wasm::parse_ingredient("2 cups flour");
    assert!(result.is_ok());
}

#[wasm_bindgen_test]
fn test_parse_ingredient_with_modifier() {
    let result = ingredient_wasm::parse_ingredient("1 cup sugar, sifted");
    assert!(result.is_ok());
}

#[wasm_bindgen_test]
fn test_parse_ingredient_with_fraction() {
    let result = ingredient_wasm::parse_ingredient("1/2 tsp salt");
    assert!(result.is_ok());
}

#[wasm_bindgen_test]
fn test_parse_ingredient_range() {
    let result = ingredient_wasm::parse_ingredient("1-2 tbsp olive oil");
    assert!(result.is_ok());
}

#[wasm_bindgen_test]
fn test_format_amount() {
    let amount = js_sys::Object::new();
    js_sys::Reflect::set(
        &amount,
        &JsValue::from_str("unit"),
        &JsValue::from_str("cup"),
    )
    .unwrap();
    js_sys::Reflect::set(
        &amount,
        &JsValue::from_str("value"),
        &JsValue::from_f64(2.0),
    )
    .unwrap();

    let result = ingredient_wasm::format_amount(&amount.into());
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), "2 cup");
}

#[wasm_bindgen_test]
fn test_format_amount_value() {
    let amount = js_sys::Object::new();
    js_sys::Reflect::set(&amount, &JsValue::from_str("unit"), &JsValue::from_str("g")).unwrap();
    js_sys::Reflect::set(
        &amount,
        &JsValue::from_str("value"),
        &JsValue::from_f64(100.5),
    )
    .unwrap();

    let result = ingredient_wasm::format_amount_value(&amount.into());
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), 100.5);
}

#[wasm_bindgen_test]
fn test_amount_kind_weight() {
    let amount = js_sys::Object::new();
    js_sys::Reflect::set(&amount, &JsValue::from_str("unit"), &JsValue::from_str("g")).unwrap();
    js_sys::Reflect::set(
        &amount,
        &JsValue::from_str("value"),
        &JsValue::from_f64(100.0),
    )
    .unwrap();

    let result = ingredient_wasm::amount_kind(&amount.into());
    assert!(result.is_ok());
}

#[wasm_bindgen_test]
fn test_amount_kind_volume() {
    let amount = js_sys::Object::new();
    js_sys::Reflect::set(
        &amount,
        &JsValue::from_str("unit"),
        &JsValue::from_str("cup"),
    )
    .unwrap();
    js_sys::Reflect::set(
        &amount,
        &JsValue::from_str("value"),
        &JsValue::from_f64(1.0),
    )
    .unwrap();

    let result = ingredient_wasm::amount_kind(&amount.into());
    assert!(result.is_ok());
}

#[wasm_bindgen_test]
fn test_is_valid_unit_builtin() {
    assert!(ingredient_wasm::is_valid_unit("cup", vec![]));
    assert!(ingredient_wasm::is_valid_unit("gram", vec![]));
    assert!(ingredient_wasm::is_valid_unit("tbsp", vec![]));
    assert!(ingredient_wasm::is_valid_unit("oz", vec![]));
}

#[wasm_bindgen_test]
fn test_is_valid_unit_custom() {
    assert!(!ingredient_wasm::is_valid_unit("handful", vec![]));
    assert!(ingredient_wasm::is_valid_unit(
        "handful",
        vec!["handful".to_string()]
    ));
}

#[wasm_bindgen_test]
fn test_parse_unit_mapping_conversion_format() {
    let result = ingredient_wasm::parse_unit_mapping("4 lb = $5".to_string());
    assert!(result.is_ok());
}

#[wasm_bindgen_test]
fn test_parse_unit_mapping_price_per_format() {
    let result = ingredient_wasm::parse_unit_mapping("$5/4lb".to_string());
    assert!(result.is_ok());
}

#[wasm_bindgen_test]
fn test_parse_unit_mapping_with_source() {
    let result = ingredient_wasm::parse_unit_mapping("4 lb = $5 @ costco".to_string());
    assert!(result.is_ok());
}

#[wasm_bindgen_test]
fn test_parse_unit_mapping_invalid() {
    let result = ingredient_wasm::parse_unit_mapping("invalid".to_string());
    assert!(result.is_err());
}

#[wasm_bindgen_test]
fn test_parse_rich_text_simple() {
    let result = ingredient_wasm::parse_rich_text(
        "Add 2 cups of flour".to_string(),
        vec!["flour".to_string()],
    );
    assert!(result.is_ok());
}

#[wasm_bindgen_test]
fn test_parse_rich_text_multiple_ingredients() {
    let result = ingredient_wasm::parse_rich_text(
        "Mix the flour and sugar together".to_string(),
        vec!["flour".to_string(), "sugar".to_string()],
    );
    assert!(result.is_ok());
}

#[wasm_bindgen_test]
fn test_graph_unit_mappings_empty() {
    let result = ingredient_wasm::graph_unit_mappings(vec![]);
    assert!(result.is_ok());
    let dot = result.unwrap();
    assert!(dot.contains("digraph"));
}
