//! Golden snapshot tests (insta) for parser output.
//!
//! These capture the parse of a curated set of representative inputs as
//! human-readable snapshots, so any behavior change surfaces as a clear diff in
//! review (rather than many hand-written assertions). Snapshots live in
//! `tests/snapshots/`. Update intentionally with `cargo insta review`
//! (or `INSTA_UPDATE=always cargo test`). In CI, mismatches fail.

#![allow(clippy::unwrap_used)]

use ingredient::{IngredientParser, from_str};

/// Representative inputs spanning the parser's features (units, fractions,
/// ranges, multiple units, optional, parenthesized secondary amounts, fallback).
const SAMPLES: &[&str] = &[
    "2 cups all-purpose flour, sifted",
    "1¼ cups / 155.5g flour",
    "2-3 cups chicken broth",
    "1 to 2 tablespoons honey",
    "(1/2 cup chopped walnuts)",
    "1 (14.5 oz) can diced tomatoes",
    "1 cup (2 sticks) butter",
    "3 large eggs",
    "salt to taste",
    "Chocolate Chip Cookies",
];

/// Round-trip each sample through `from_str` and render via `Display`. One
/// combined snapshot acts as a readable golden file of parser behavior.
#[test]
fn display_roundtrip() {
    let rendered = SAMPLES
        .iter()
        .map(|input| format!("{input:<35} => {}", from_str(input)))
        .collect::<Vec<_>>()
        .join("\n");
    insta::assert_snapshot!(rendered);
}

/// Full structured form of a feature-rich parse (multiple units + fraction).
#[test]
fn debug_structure_multi_unit() {
    insta::assert_debug_snapshot!(from_str("1¼ cups / 155.5g flour"));
}

/// The parser decision-tree trace for a representative line.
#[test]
fn trace_tree() {
    let traced = IngredientParser::new().parse_with_trace("2 cups flour, sifted");
    insta::assert_snapshot!(traced.trace.format_tree(false));
}
