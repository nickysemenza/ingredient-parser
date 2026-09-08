# Independent cookbook sample

This corpus measures behavior on source lines selected independently of parser
success or failure. It complements the fix-driven regression corpus; it is not a
claim that every natural-language ambiguity has one correct interpretation.

## Source selection

`sources.jsonl` contains 500 distinct-within-book ingredient lines: 50 from each
of ten development books. `manifest.json` records book identity, format hash,
publisher ingredient classes, candidate count, and split. The first six books
are the fixed 300-line benchmark cohort; the remaining four are development
extensions pending replacement holdout sampling.
Each line records the EPUB member and DOM element index/ID. Full books remain in
the user's Calibre library and are not copied into this repository.

Selection uses only publisher ingredient markup, nonempty text, exclusion of
colon-terminated section headings and publisher-styled underlined headings, and
deduplication. Whitespace is collapsed; other source wording/case is retained. A
frozen list of pre-redesign corpus inputs is excluded case-insensitively.
Candidates are ranked by SHA-256 of the fixed seed, book ID, and source text; the
first 50 form the sample. No numeric-line filter or parser confidence signal is
used. Generic ingredient lines may occur in multiple books; the split is by book,
not by unique phrase across all books.

Reproduce or verify with the Python standard library:

```sh
python3 scripts/sample_cookbooks.py '/path/to/Calibre' --verify
```

The same script verifies the 300-line development benchmark workload. The
benchmark file is kept under `benches/` so the published crate's benchmark does
not reference excluded test files.

## Labels and uncertainty

The per-book JSONL files pair each source ID and exact input with desired fields.
They were authored by agents without running the parser or inspecting its output.
These are reviewable annotations, not independently human-adjudicated gold
labels. Ambiguous cases and source defects should be corrected from source
evidence and the written contract, never to make an implementation's output
pass. The former holdout cohort is treated as development because its
contamination history is unknown; replacement holdout labels must come from
new books and remain isolated from implementation work.

Labels follow the conservative parsing contract: no inferred food ontology or
shared head noun; preserve ambiguous no-quantity alternatives; retain explicit
quantity alternatives in Modifier. Units/fractions are canonicalized, food text
keeps source case, extracted modifiers follow source order, dimensional cuts and
temperatures describe the food. `ground`, `dried`, `frozen`, `roasted`, `toasted`,
`crumbled`, and `shelled` can describe identity; the documented preparation
vocabulary governs extraction. Explicit post-comma preparation remains Modifier.

`label-corrections.json` records source locations, before/after labels, and contract
evidence for development-label adjudications. These are agent reviews, not human
adjudication. Compare implementations against the same label revision; report
results against the original labels separately when discussing label changes.

Known source defects are retained, including `AR BOR IO` / `ON ION` / `SMASH ED`
in Everyday Wok and parenthetical `( cup)` with a missing quantity in Charred.
We do not invent missing numbers or fix OCR spelling in the labels. Approximation
words do not change the numerical value, and descriptions of yield/origin such
as `(from 1 lemon)` are not equivalent measures of the named food.

## Evaluation

```sh
cargo run -p ingredient-corpus --example evaluate -- \
  ingredient-parser/tests/corpus/cookbooks development
cargo run -p ingredient-corpus --example evaluate -- \
  ingredient-parser/tests/corpus/cookbooks holdout
```

Save evaluator output to compare two frozen implementations without reopening
labels:

```sh
python3 scripts/compare_cookbook_eval.py before.json after.json
python3 scripts/compare_cookbook_eval.py before.json after.json \
  --book-id wok --book-id arabiyya --book-id charred \
  --book-id home-kitchen --book-id bakers-companion --book-id nopalito
```

The comparison validates identical row IDs and fields, then reports exact and
per-field before/after counts plus improved and regressed IDs. Use the cohort
filter when comparing the fixed 300-line benchmark separately from other
development books.

The evaluator prints exact matches, five per-field counts (name, amounts,
modifier, optional, usage), and row-level differences. It never updates labels.
The existing regression corpus continues rejecting each unapproved regression.
This independent sample intentionally includes unresolved cases: passing rows
and field counts are ratcheted separately from their desired labels.

Evaluate a replacement holdout only after a substantial implementation
milestone, report its results, and do not tune the same milestone against its
failures.
