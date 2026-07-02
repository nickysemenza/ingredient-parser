# Project Instructions

## Parser Development

- Always add tests when updating the parser
- Prefer adding new cases to existing rstest-parameterized tests over creating separate test functions
- **Where a test goes:** `from_str` accuracy (input → name/amounts/modifier/optional) belongs in `tests/corpus/corpus.jsonl` — the regression ratchet scored by `tests/accuracy.rs`. `tests/trace.rs` tests the trace *tree*, not parse correctness; the traced path's accuracy is already proven by `accuracy.rs::trace_path_matches_from_str`. Keep `parse_amount`, `RichParser`, `Display`, custom-parser config, and unit/conversion behavior in Rust `#[rstest]` tests — the corpus schema can't express those.
- See `ingredient-parser/src/lib.rs` "Design Decisions" section for key parsing philosophy (size words, modifiers, etc.)
- **Where a fix goes:** the pipeline is normalize → recognize → grammar (amounts) → segment (clause split + assembly) → refine (name-internal passes). To see which stage shaped a line (and therefore where a fix belongs), run `parse-ingredient --explain "<line>"`. The routing decision tree lives in the `ingredient-parser/src/parser/mod.rs` module doc ("Where does a parser fix go?").
- For debugging or iteration, parse ingredients into JSON with:
  ```
  cargo run -p food-cli --quiet -- parse-ingredient "1 cup flour, sifted"
  ```
  Add `--explain` for the compact stage view, or `--debug` for the full grammar trace tree.

## Testing

- Use `cargo nextest run` for faster parallel test execution
- Benchmarks require the `bench` feature: `cargo bench -p ingredient --features bench`
