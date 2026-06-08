#![cfg(target_arch = "wasm32")]

use ingredient_wasm::{WAmount, WUnitMappings};
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn amount(unit: &str, value: f64) -> WAmount {
    WAmount {
        unit: unit.to_string(),
        value,
        upper_value: None,
    }
}

#[wasm_bindgen_test]
fn test_parse_ingredient_simple() {
    let result = ingredient_wasm::parse_ingredient("2 cups flour");
    assert_eq!(result.name, "flour");
    assert_eq!(result.amounts.len(), 1);
}

#[wasm_bindgen_test]
fn test_parse_ingredient_with_modifier() {
    let result = ingredient_wasm::parse_ingredient("1 cup sugar, sifted");
    assert_eq!(result.name, "sugar");
    assert_eq!(result.modifier.as_deref(), Some("sifted"));
}

#[wasm_bindgen_test]
fn test_parse_ingredient_with_fraction() {
    let result = ingredient_wasm::parse_ingredient("1/2 tsp salt");
    assert_eq!(result.name, "salt");
}

#[wasm_bindgen_test]
fn test_parse_ingredient_range() {
    let result = ingredient_wasm::parse_ingredient("1-2 tbsp olive oil");
    assert_eq!(result.name, "olive oil");
}

#[wasm_bindgen_test]
fn test_format_amount() {
    assert_eq!(ingredient_wasm::format_amount(amount("cup", 2.0)), "2 cup");
}

#[wasm_bindgen_test]
fn test_format_amount_value() {
    assert_eq!(
        ingredient_wasm::format_amount_value(amount("g", 100.5)),
        100.5
    );
}

#[wasm_bindgen_test]
fn test_amount_kind_weight() {
    let result = ingredient_wasm::amount_kind(amount("g", 100.0));
    assert!(result.is_ok());
}

#[wasm_bindgen_test]
fn test_amount_kind_volume() {
    let result = ingredient_wasm::amount_kind(amount("cup", 1.0));
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
    assert_eq!(result.unwrap().source.as_deref(), Some("costco"));
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
    let dot = ingredient_wasm::graph_unit_mappings(WUnitMappings(vec![]));
    assert!(dot.contains("digraph"));
}
