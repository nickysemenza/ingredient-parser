//! Browser-side smoke test of the wasm boundary. Runs under
//! `wasm-pack test --headless --chrome cookbook --no-default-features --features wasm,test-support -- --test wasm`.
#![cfg(all(target_arch = "wasm32", feature = "wasm"))]
#![allow(clippy::unwrap_used)]

use cookbook::ExtractOptions;
use cookbook::wasm::{Book, default_ladder, open_book, usage_from_response};
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

#[wasm_bindgen_test]
fn opens_outlines_and_estimates() {
    let bytes = cookbook_fixtures::epub3_nav_pagebreaks().unwrap();
    let book: Book = open_book(bytes, "fixture".into()).unwrap();
    let outline = book.outline();
    assert_eq!(outline.chapters, ["Pies and Tarts", "Foundational Recipes"]);
    assert_eq!(outline.nav_recipe_titles, 4);
    let estimate = book.estimate(ExtractOptions::default()).unwrap();
    assert!(estimate.cost_usd_high >= estimate.cost_usd_low);
    assert_eq!(estimate.ladder, default_ladder());
    let cover = book.cover();
    assert!(cover.is_none() || book.read_image(&cover.unwrap().path).is_some());
}

#[wasm_bindgen_test]
fn reads_usage_from_provider_bodies() {
    let usage = usage_from_response(
        "gemini-2.5-flash",
        r#"{"usage":{"prompt_tokens":10,"completion_tokens":3}}"#,
    )
    .unwrap();
    assert_eq!((usage.input_tokens, usage.output_tokens), (10, 3));
    assert!(usage_from_response("no-such-model", "{}").is_none());
}
