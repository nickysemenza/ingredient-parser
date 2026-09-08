# Parser and recipe architecture cleanup

The parser resolves the structure of a written ingredient line. Scraping and
cookbook extraction supply source text; applications consume one parse execution
rather than reimplementing or repeating parsing. The food name remains opaque;
this change does not introduce a food ontology or a public syntax tree.

## Ownership

- `ingredient` owns measurement recognition, ingredient structure, usage,
  diagnostics, source attribution, and the scaling policy. Its measurement
  grammar remains shared by ingredient-list and rich-text parsing.
- `recipe-types` owns dependency-light source recipe data.
- `recipe-parsing` owns parsing a complete sequence of recipe sections using one
  configured parser. Ingredient observations come from the same execution as the
  returned ingredient. Instructions keep their source positions and fall back to
  authored text with a diagnostic if rich parsing fails.
- `recipe-scraper` owns HTML extraction, and reexports the existing recipe-parsing
  types/functions for compatibility. `recipe-scraper-fetcher` owns HTTP/cache
  policy; client initialization failure cannot remove request timeouts.
- `recipe-epub` owns source extraction and cookbook assembly. Its existing
  runtime-independent orchestration is retained. A title is a label, not an
  occurrence identity: only explicit adjacent continuation evidence permits
  merging. Failed or empty chunk slots are barriers, not invisible gaps.
- WASM is an adapter to these modules. Native and browser views reuse parsed
  sections; browser recipe scaling applies Rust's measure policy to both the
  ingredient list and cached instruction chunks.

## Compatibility

The ingredient result remains the existing Name/Measures/Modifier/Optional/Usage
shape. Historical parser behavior changes are recorded in
[parser-semantics.md](parser-semantics.md), separately from architecture edits.

`recipe_scraper::{ParsedRecipe, ParsedSection, parse_sections}` remain available
through reexports. `RichParser::new` and convenience recipe parsing retain default
configuration; configured recipe execution is additive.

Native cookbook extraction has an additive detailed report entrypoint retaining
chunk outcomes, failures, truncation evidence and accounting. Existing tuple
entrypoints adapt over it. `Fetcher` retains existing constructors and adds eager
fallible constructors. Legacy construction failures surface when fetching rather
than silently using an unbounded HTTP client.

No ingredient instruction or repeated continuation section is deduplicated by
text equality: the source windows do not overlap, and repeated steps may be
intentional. Existing notes/equipment union and metadata precedence remain.
Ambiguous same-title recipe references are omitted rather than assigned to an
arbitrary occurrence.

## Evidence and completion gates

The pre-change `cbd43a5` baseline passed 1,262 parser/corpus tests. That establishes
known-behavior preservation, not general accuracy: the existing corpus had 413
rows, 410 distinct inputs, and one known gap, much of it accumulated from fixes.

The cookbook corpus adds source-linked, book-disjoint samples selected without
parser confidence or output. See its README for the labeling protocol and
reproduction command. Held-out data is evaluated after the implementation
milestone; its failures must not be used to tune that milestone.

Acceptance includes the exact regression ratchet, composed structural cases,
source-attribution assertions, configuration/observation parity, instruction
preservation, browser rendering/scaling, EPUB continuation barriers, full
workspace and browser checks, and measured parser performance. Benchmark smoke
execution proves that a benchmark runs, not that performance is unchanged.
