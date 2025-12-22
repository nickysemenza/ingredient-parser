# Project Instructions

## Parser Development

- Always add tests when updating the parser
- Prefer adding new cases to existing rstest-parameterized tests over creating separate test functions
- See `ingredient-parser/src/lib.rs` "Design Decisions" section for key parsing philosophy (size words, modifiers, etc.)
- For debugging or iteration, parse ingredients into JSON with:
  ```
  cargo run -p food-cli --quiet -- parse-ingredient "1 cup flour, sifted"
  ```

## Testing

- Use `cargo nextest run` for faster parallel test execution
- Benchmarks require the `bench` feature: `cargo bench -p ingredient --features bench`
