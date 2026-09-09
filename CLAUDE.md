# Project Instructions

## Parser Development

- Always add tests when updating the parser
- Prefer adding new cases to existing rstest-parameterized tests over creating separate test functions
- **Where a test goes:** `from_str` accuracy (input → name/amounts/modifier/optional) belongs in `tests/corpus/corpus.jsonl` — the regression ratchet scored by `tests/accuracy.rs`. `tests/trace.rs` tests the trace *tree*, not parse correctness; the traced path's accuracy is already proven by `accuracy.rs::trace_path_matches_from_str`. Keep `parse_amount`, `RichParser`, `Display`, custom-parser config, and unit/conversion behavior in Rust `#[rstest]` tests — the corpus schema can't express those.
- See `ingredient-parser/src/lib.rs` "Design Decisions" section for key parsing philosophy (size words, modifiers, etc.)
- **Where a fix goes:** run `food-cli ingredient parse --explain "<line>"`, then fix the earliest incorrect structural interpretation. Preserve source ownership through explicit text edits and lower the result once. When replacing an interpretation, delete its superseded repairs. See `ingredient-parser/src/parser/mod.rs` for module ownership and `docs/parser-semantics.md` before changing expected behavior.
- **Independent evaluation:** cookbook labels and source selection live in `ingredient-parser/tests/corpus/cookbooks/`. Read its README before sampling, labeling, or evaluating. Keep held-out labels independent of implementation output; document label corrections with source/contract evidence.
- For debugging or iteration, parse ingredients into JSON with:
  ```
  cargo run -p food-cli --quiet -- ingredient parse "1 cup flour, sifted"
  ```
  Add `--explain` for the compact stage view, or `--debug` for the full grammar trace tree.

## Before deleting a `pub` item

`recipe-scraper`, `recipe-epub` and `recipe-types` are `publish = false` but have
external consumers. Zero callers in this repo does not prove a public API is
unused. Preserve compatibility and check any consumer code supplied with the
task before narrowing an API. See CONTRIBUTING.md.

## Testing

- Use `cargo nextest run` for faster parallel test execution
- Benchmarks require the `bench` feature: `cargo bench -p ingredient --features bench`

## Cookbook extraction review

Use `food-cli` for headless inspection, saved-run replay, and evaluation. Read [docs/cookbook-review.md](docs/cookbook-review.md) before extracting books, refreshing model results, or assessing whole-book fidelity.
