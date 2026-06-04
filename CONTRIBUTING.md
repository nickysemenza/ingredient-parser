# Contributing

Thanks for helping improve the ingredient parser! This guide covers the
day-to-day workflow. The crate's parsing philosophy (how size words, modifiers,
multiple units, etc. are handled) lives in the **"Design Decisions"** section of
[`ingredient-parser/src/lib.rs`](ingredient-parser/src/lib.rs) — read that first
when deciding what a line *should* parse to.

## Workspace layout

| Crate | Purpose |
| --- | --- |
| `ingredient-parser` | Core parser library (published to crates.io as `ingredient`) |
| `ingredient-wasm` | WASM bindings |
| `recipe-scraper` / `recipe-scraper-fetcher` | Extract recipes from web pages |
| `food-cli` | Command-line tool for parsing/scraping |
| `food-app` | egui desktop/web playground |
| `demo-site` | React + Vite demo frontend |

## Quick commands

```bash
# Fast parallel test run (preferred)
cargo nextest run

# Doctests (nextest doesn't run these)
cargo test -p ingredient --doc

# Lint + format (CI denies warnings; the workspace denies unwrap/expect/panic)
cargo clippy --all-targets
cargo fmt

# Parse a single line while iterating
cargo run -p food-cli --quiet -- parse-ingredient "1 cup flour, sifted"

# Benchmarks (need the `bench` feature) and fuzzing (need nightly)
cargo bench -p ingredient --features bench
cd ingredient-parser/fuzz && cargo +nightly fuzz run from_str
```

## The accuracy corpus (most important)

[`ingredient-parser/tests/corpus/corpus.jsonl`](ingredient-parser/tests/corpus/corpus.jsonl)
is the north-star quality metric and the home for parse-accuracy cases. It is
JSON-lines; `//` and blank lines are ignored. Each row is an input plus its
expected `name`, `amounts`, `modifier`, and `optional`.

There are two kinds of rows:

- **Committed rows** (no `xfail`): MUST parse exactly as labeled. A mismatch
  fails [`tests/accuracy.rs`](ingredient-parser/tests/accuracy.rs). This is a
  per-row ratchet — no committed row can ever silently regress.
- **Known gaps** (`"xfail": "reason"`): the label is the *desired* parse the
  parser does not yet produce. A mismatch is reported but tolerated. When the
  parser improves enough that an `xfail` row passes, the test prints a
  `PROMOTE` hint so you can remove the `xfail` marker.

### Adding cases

1. Append real ingredient lines to the corpus.
2. Run `cargo nextest run -p ingredient accuracy_corpus --no-capture`.
3. If a new line parses correctly, leave it as a committed row.
4. If it doesn't (and the *right* answer needs parser work), mark it `xfail`
   with a short reason. Get exact field values from the CLI:
   ```bash
   cargo run -p food-cli --quiet -- parse-ingredient "your line here"
   ```
5. Float values must be exact `f64` (e.g. `⅔` is `0.6666666666666666`).

### Browsing the corpus

To eyeball the whole corpus as a rendered table, run:

```bash
cargo run -p food-cli --quiet -- corpus-table
```

It renders `corpus.jsonl` to a temporary HTML page (grouped by section, with
ranges and `xfail` rows highlighted) and opens it in your default browser. Pass
`--out file.html` to write it somewhere, or `--out -` to stream HTML to stdout.

## When you change the parser

- **Always add tests.** Prefer appending corpus rows (for end-to-end behavior)
  and extending the existing `rstest`-parameterized unit tests over writing new
  one-off test functions.
- Run `cargo nextest run` and `cargo test -p ingredient --doc`; both must pass.
- Run `cargo clippy --all-targets` and `cargo fmt` before pushing.
- If a fix relies on a non-obvious invariant, add a brief code comment.

## Snapshot tests

[`tests/snapshots.rs`](ingredient-parser/tests/snapshots.rs) uses `insta`. If a
change intentionally alters snapshot output, review and accept with
`cargo insta review`.
