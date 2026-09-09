# recipe-epub-fixtures

Generate a real, deterministic EPUB in memory for import tests and UI previews.
The archive contains three invented recipes: roasted tomato soup with white beans
and basil, flatbread, and lemon dressing. It includes EPUB metadata, a navigation
table of contents, and three XHTML documents in spine order.

```rust
let epub_bytes = recipe_epub_fixtures::cookbook_epub()?;
// Pass epub_bytes to your importer, or write them into a test temp directory.
```

Use this crate as a Rust dev-dependency. It can be consumed from this workspace,
a local checkout, or a pinned Git revision of this repository (package
`recipe-epub-fixtures`). It is not published to crates.io. For example, with a
sibling checkout:

```toml
[dev-dependencies]
recipe-epub-fixtures = { path = "../ingredient-parser/recipe-epub-fixtures" }
```

For manual inspection:

```sh
cargo run -p recipe-epub-fixtures --example write_cookbook -- /tmp/synthetic-cookbook.epub
```

Generation uses only embedded source files and an in-memory ZIP writer. No model,
network, clock, random input, or filesystem access is involved in the library.
The archive is byte-identical across repeated calls with the same crate/dependency
versions. There are no images or canned extraction responses: consumers testing
extraction must supply their own mock transport. This fixture tests ordinary EPUB
import and review; it does not represent the full variety of publisher layouts.
