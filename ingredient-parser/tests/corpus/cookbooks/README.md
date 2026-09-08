# Independent cookbook sample

This corpus measures behavior on source lines selected independently of parser
success or failure. It complements the fix-driven regression corpus; it is not a
claim that every natural-language ambiguity has one correct interpretation.

## Source selection

`sources.jsonl` contains 500 distinct-within-book ingredient lines: 50 from each
of six development books and four held-out books. `manifest.json` records book
identity, format hash, publisher ingredient classes, candidate count, and split.
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
The four holdout books were labeled by a separate agent. These are reviewable
annotations, not independently human-adjudicated gold labels. Ambiguous cases and
source defects should be corrected from source evidence and the written contract,
never to make an implementation's output pass.

Labels follow the conservative parsing contract: no inferred food ontology or
shared head noun; preserve ambiguous no-quantity alternatives; retain explicit
quantity alternatives in Modifier. Units/fractions are canonicalized, food text
keeps source case, extracted modifiers follow source order, dimensional cuts and
temperatures describe the food. `ground`, `dried`, `frozen`, `roasted`, `toasted`,
`crumbled`, and `shelled` can describe identity; the documented preparation
vocabulary governs extraction. Explicit post-comma preparation remains Modifier.

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

The evaluator prints exact matches, five per-field counts (name, amounts,
modifier, optional, usage), and row-level differences. It never updates labels.
The existing regression corpus continues rejecting each unapproved regression.
This independent sample intentionally includes unresolved cases: passing rows
and field counts are ratcheted separately from their desired labels.

Evaluate holdout after a substantial implementation milestone, report its results,
and do not tune the same milestone against its failures. If its cases later guide
fixes, it becomes development data and new books must supply fresh holdout data.
