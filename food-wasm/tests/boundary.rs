//! The browser boundary on a real wasm32 runtime: `wasm-pack test --node food-wasm`.
//! Native tests cannot catch these: APIs that panic only on wasm32 (clocks),
//! and JSON crossing the JavaScript string boundary.
#![cfg(target_arch = "wasm32")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::{Value, json};
use wasm_bindgen_test::*;

fn call(command: &str, args: Value) -> Value {
    serde_json::from_str(&food_wasm::dispatch(command, &args.to_string()).unwrap()).unwrap()
}

#[wasm_bindgen_test]
fn parse_lines_crosses_the_boundary() {
    let rows = call(
        "parse_lines",
        json!({ "input": "2 cups flour, sifted\n\n1/2 tsp salt" }),
    );
    assert_eq!(rows.as_array().unwrap().len(), 3);
    assert_eq!(rows[0]["ingredient"]["name"], "flour");
    assert_eq!(rows[0]["lineNumber"], 1);
    assert_eq!(rows[0]["amounts"][0], "2 cups");
}

/// The full trace reads the clock; `std::time` panics on wasm32.
#[wasm_bindgen_test]
fn inspection_runs_in_the_browser() {
    let inspection = call("inspect_ingredient", json!({ "input": "Juice of 1 lemon" }));
    assert!(inspection["trace"].is_object());
    assert!(inspection["stages"]["recognizers"].is_array());
    assert!(inspection["jaegerJson"].as_str().unwrap().starts_with('{'));
}

#[wasm_bindgen_test]
fn opened_files_are_read_by_name() {
    let error = food_wasm::dispatch("open_book", r#"{"path":"missing.epub"}"#).unwrap_err();
    assert!(error.contains("missing.epub is not open"), "{error}");

    let epub = cookbook_fixtures::epub3_nav_pagebreaks().unwrap();
    food_wasm::put_file("fixture.epub", epub).unwrap();
    let book = call("open_book", json!({ "path": "fixture.epub" }));
    assert!(book["outline"]["chunks"].as_u64().unwrap() >= 1);
    assert_eq!(book["runs"], json!([]));
    let lines = call(
        "book_source",
        json!({ "path": "fixture.epub", "start": 0, "end": 3 }),
    );
    assert_eq!(lines.as_array().unwrap().len(), 3);
}

#[wasm_bindgen_test]
fn corpus_scores_without_a_filesystem() {
    let corpus = call("score_corpus", json!({ "path": null }));
    assert!(corpus["cases"].as_array().unwrap().len() > 100);
}
