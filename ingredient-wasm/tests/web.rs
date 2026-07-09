#![cfg(target_arch = "wasm32")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use ingredient_wasm::{WAmount, WUnitMapping, WUnitMappings};
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

fn amount(unit: &str, value: f64) -> WAmount {
    WAmount {
        unit: unit.to_string(),
        value,
        upper_value: None,
    }
}

/// A single unit-mapping pair (e.g. 1 cup ↔ 120 g), for the conversion entry points.
fn mapping(a: WAmount, b: WAmount) -> WUnitMappings {
    WUnitMappings(vec![WUnitMapping { a, b, source: None }])
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
    assert_eq!(ingredient_wasm::format_amount(amount("cup", 2.0)), "2 cups");
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
    let result = ingredient_wasm::parse_unit_mapping("4 lb = $5");
    assert!(result.is_ok());
}

#[wasm_bindgen_test]
fn test_parse_unit_mapping_price_per_format() {
    let result = ingredient_wasm::parse_unit_mapping("$5/4lb");
    assert!(result.is_ok());
}

#[wasm_bindgen_test]
fn test_parse_unit_mapping_with_source() {
    let result = ingredient_wasm::parse_unit_mapping("4 lb = $5 @ costco");
    assert_eq!(result.unwrap().source.as_deref(), Some("costco"));
}

#[wasm_bindgen_test]
fn test_parse_unit_mapping_invalid() {
    let result = ingredient_wasm::parse_unit_mapping("invalid");
    assert!(result.is_err());
}

#[wasm_bindgen_test]
fn test_parse_rich_text_simple() {
    let result = ingredient_wasm::parse_rich_text("Add 2 cups of flour", vec!["flour".to_string()]);
    assert!(result.is_ok());
}

#[wasm_bindgen_test]
fn test_parse_rich_text_multiple_ingredients() {
    let result = ingredient_wasm::parse_rich_text(
        "Mix the flour and sugar together",
        vec!["flour".to_string(), "sugar".to_string()],
    );
    assert!(result.is_ok());
}

#[wasm_bindgen_test]
fn test_graph_unit_mappings_empty() {
    let dot = ingredient_wasm::graph_unit_mappings(WUnitMappings(vec![]));
    assert!(dot.contains("digraph"));
}

// ── conversion + decomposition entry points (marshal through WUnitMappings) ──

#[wasm_bindgen_test]
fn test_conv_amount_to_unit() {
    // 1 cup = 120 g, so 240 g of this ingredient converts to ~2 cups.
    let mappings = mapping(amount("cup", 1.0), amount("g", 120.0));
    let result =
        ingredient_wasm::conv_amount_to_unit(mappings, "cup".to_string(), amount("g", 240.0));
    let converted = result.expect("g → cup should convert via the mapping");
    assert_eq!(converted.unit, "cup");
    assert!(
        (converted.value - 2.0).abs() < 1e-9,
        "expected ~2 cups, got {}",
        converted.value
    );
}

#[wasm_bindgen_test]
fn test_conv_amount_to_unit_no_path_errors() {
    // No mapping bridges weight and this volume → conversion fails (not a panic).
    let result = ingredient_wasm::conv_amount_to_unit(
        WUnitMappings(vec![]),
        "cup".to_string(),
        amount("g", 100.0),
    );
    assert!(result.is_err());
}

#[wasm_bindgen_test]
fn test_conv_amount_to_kind() {
    // Build the target kind (volume) from a known volume amount, then convert a
    // weight to volume across the cup↔g mapping.
    let target_kind = ingredient_wasm::amount_kind(amount("cup", 1.0)).expect("kind");
    let mappings = mapping(amount("cup", 1.0), amount("g", 120.0));
    let result = ingredient_wasm::conv_amount_to_kind(mappings, target_kind, amount("g", 240.0));
    assert!(result.is_ok(), "g → volume should convert via the mapping");
}

#[wasm_bindgen_test]
fn test_conv_amount_to_nutrients() {
    // With no conversion path the call still succeeds, returning an object whose
    // target keys map to null (exercises the per-target loop + Reflect::set).
    let result = ingredient_wasm::conv_amount_to_nutrients(
        WUnitMappings(vec![]),
        vec!["g protein".to_string(), "mg sodium".to_string()],
        amount("g", 100.0),
    );
    assert!(result.is_ok());
}

#[wasm_bindgen_test]
fn test_decompose_ingredient() {
    let decomp = ingredient_wasm::decompose_ingredient("2 cups flour, sifted");
    assert_eq!(decomp.source, "2 cups flour, sifted");
    assert!(!decomp.segments.is_empty());
}

/// `WDecomposition::from` slices `d.source` by upstream byte-range spans (see
/// the doc comment on `WSegment` in src/lib.rs) — the whole reason it exists is
/// that Rust spans are UTF-8 byte ranges, not JS UTF-16 indices. Assert the
/// segments reconstruct the exact source string, and that no segment is empty
/// (an empty segment would mean a zero-width or misaligned slice slipped
/// through the byte-offset walk), across multibyte-heavy inputs: a 4-byte
/// emoji + variation selector, a ZWJ family emoji, stacked combining marks, and
/// multibyte characters sitting right at a clause boundary.
fn assert_segments_reconstruct(source: &str) {
    let decomp = ingredient_wasm::decompose_ingredient(source);
    assert_eq!(decomp.source, source);

    let joined: String = decomp.segments.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(
        joined, source,
        "segments must reconstruct the source exactly"
    );

    let texts: Vec<&str> = decomp.segments.iter().map(|s| s.text.as_str()).collect();
    assert!(
        texts.iter().all(|t| !t.is_empty()),
        "no segment should be empty: {texts:?}"
    );
}

#[wasm_bindgen_test]
fn test_decompose_ingredient_emoji() {
    // 🌶️ = U+1F336 (4-byte) HOT PEPPER + U+FE0F (3-byte) variation selector.
    assert_segments_reconstruct("2 cups 🌶️ pepper flakes");
}

#[wasm_bindgen_test]
fn test_decompose_ingredient_zwj_sequence() {
    // 👨‍👩‍👧‍👦 = four 4-byte emoji joined by three U+200D ZWJ (3-byte) codepoints.
    assert_segments_reconstruct("1 lb 👨‍👩‍👧‍👦 family-size ground beef, thawed");
}

#[wasm_bindgen_test]
fn test_decompose_ingredient_combining_marks() {
    // "cafe\u{0301}" spells "café" with a combining acute accent (U+0301, 2-byte)
    // rather than the precomposed é, so the grapheme spans two codepoints.
    assert_segments_reconstruct("1 tsp cafe\u{0301} powder, finely ground");
}

#[wasm_bindgen_test]
fn test_decompose_ingredient_multibyte_at_segment_boundaries() {
    // ½ (2-byte) opens the amount span; ñ and é (2-byte each) sit right at the
    // name/modifier boundary carved by the comma.
    assert_segments_reconstruct("½ jalapeño, émincé");
}
