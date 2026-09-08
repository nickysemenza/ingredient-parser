# Architecture cleanup validation

## Acceptance data

Baseline is commit `cbd43a5`. The existing corpus contained 413 rows, 410 distinct
inputs and one known gap. The replacement regression corpus contains 430 rows;
all expected fields pass. The former known gap (`peeled and deveined, large
shrimp`) is now an ordinary passing regression. Seventeen composition cases were
added. All 57 changed historical expectations are listed individually in
[corpus-semantic-corrections.json](corpus-semantic-corrections.json); a comparison
against the baseline found no undocumented historical label changes.

The separate cookbook sample uses six development and four held-out EPUBs,
50 independently sampled lines each. The selection seed, book hashes, publisher
markup, element locations and exact text are reproducible with
`scripts/sample_cookbooks.py --verify`. Source sampling excluded publisher section
headings and frozen original corpus inputs, never low-confidence parser outputs.
See the [sampling and annotation protocol](../ingredient-parser/tests/corpus/cookbooks/README.md).

Annotations were authored without parser output, with held-out books assigned to
a separate agent. They are reviewable agent labels, not human-adjudicated gold.
The held-out evaluation ran after the core test milestone; its failures were not
used to tune this implementation. Strict field equality includes unresolved
annotation/representation choices as well as parsing errors.

| Field | Development before | Development after | Held-out before | Held-out after |
|---|---:|---:|---:|---:|
| Exact | 223/300 | 272/300 | 127/200 | 154/200 |
| Name | 247/300 | 282/300 | 151/200 | 171/200 |
| Amounts | 274/300 | 286/300 | 189/200 | 189/200 |
| Modifier | 232/300 | 280/300 | 135/200 | 163/200 |
| Optional | 299/300 | 299/300 | 200/200 | 200/200 |
| Usage | 299/300 | 299/300 | 199/200 | 199/200 |

Development gained 49 exact matches; held-out gained 27. No previously correct
field regressed on either split. There remain 28 development and 46 held-out exact
mismatches; the labels retain the desired results rather than being changed to
fit the parser. `accepted-mismatches.json` freezes the failing field sets, so every
currently correct field is an individual regression ratchet. Machine-readable
counts, transitions and label hashes are in
[cookbook-evaluation.json](cookbook-evaluation.json).

Reevaluate without modifying labels:

```sh
cargo run -p ingredient-corpus --example evaluate -- \
  ingredient-parser/tests/corpus/cookbooks
```

## Compatibility and scope

Public ingredient fields and serialized shapes remain stable. Modifier text has
intentional source-order, case and punctuation changes; ingredient dimensions
and temperatures become descriptive text. Standalone amount and rich-text APIs
retain those measure kinds. Source/yield descriptions do not become equivalent
measures of another named ingredient.

The scraper retains recipe-parsing reexports. Recipe execution accepts the same
configured parser for ingredients and instructions, preserves every section and
instruction, and reports rich-text failures while retaining raw text. The WASM
batch operation is additive and accepts source recipe sections independently of
scraper metadata.

Native EPUB convenience APIs adapt over the new detailed result, which retains
the existing report's chunk failures, truncations and accounting. This does not
expand the underlying report into an audit trail of every intermediate attempt
in a successfully retried same-model call. HTTP initialization now propagates
errors while preserving bounded timeouts; legacy Fetcher constructors remain,
with additive eager `try_new`/`try_new_with_cache` entrypoints.

No release publication is part of this change.

## Performance

Criterion used the same frozen 300 development lines and synthetic ambiguous
lines with 10, 100 and 500 repeated clauses. Baseline binaries were built from
`cbd43a5` in an isolated worktree. The matched rechecks used 40 samples, 0.5-second
warmup and 2-second measurement periods, with other task builds stopped.

| Workload | Baseline mean | Final mean | Change |
|---|---:|---:|---:|
| 300 development lines | 21.763 ms | 21.841 ms | +0.4%, overlapping confidence intervals |
| 10 ambiguous clauses | 0.364 ms | 0.165 ms | -54.5% |
| 100 ambiguous clauses | 2.644 ms | 0.713 ms | -73.0% |
| 500 ambiguous clauses | 13.165 ms | 2.895 ms | -78.0% |

An initial pass suggested a faster cookbook batch but was not stable on recheck;
we use the matched repeated result above rather than claim that speedup. It also
revealed a reproducible long-input regression (26.879 ms for 500 clauses).
Investigation found the alternative detector repeatedly parsing each preceding
prefix and scanning every parenthetical before checking whether a quantity even
followed `or`. Moving that cheap, semantically identical guard first removed the
redundant work. Full estimates and confidence intervals, including the initial
and regressed runs, are retained in
[criterion-comparison.json](criterion-comparison.json).

```sh
cargo bench -p ingredient --features bench --bench parser_benchmarks -- \
  'long_ambiguous_lines|corpus_development_300' \
  --sample-size 40 --warm-up-time 0.5 --measurement-time 2
```

## Final local gates

All checks below passed on the implementation recorded in this branch.

| Gate | Result |
|---|---|
| `cargo nextest run --workspace` | 1,355 passed, zero skipped |
| `cargo test --workspace --doc` | 26 passed, two ignored |
| Formatting and workspace Clippy, all targets, warnings denied | Passed |
| EPUB without default features | Check and 110 library tests passed |
| Browser EPUB callback example | WASM compilation passed |
| Headless Chrome WASM suite | 29 passed |
| Fresh demo build and lint | Passed |
| Generated JavaScript/WASM boundary smoke | Passed |
| Rust 1.88 ingredient check | Passed |
| `cargo deny check -A unmaintained` | Passed |
| Nightly fuzz smoke | All four targets passed, 30-second budgets each |

Fuzz targets were `from_str`, `parse_amount`, `rich_text`, and `scrape`. The
ingredient parser smoke additionally used regression-corpus seeds and deeply
nested optional/trailing-measure and deleted-reference cases. Source verification
was rerun against the local EPUBs. A final cookbook evaluation exactly matched the
frozen evaluation after the performance guard reorder and lint fixes.

Local commit hooks also ran formatting and file hygiene checks. Their Cargo check
and Clippy hooks were skipped during commit creation because machine-level Cargo
patch settings append unused local patches to lockfiles; the explicit full
workspace checks above passed separately, and those machine-specific lockfile
entries were removed. Remote exact-head CI is reported in the pull request.
