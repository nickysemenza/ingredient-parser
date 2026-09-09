# Cookbook review

`food-cli` is the supported headless entry point. `food-app` opens the same local
run files using shared library code. Build once with `cargo build -p food-cli`,
then call `target/debug/food-cli` repeatedly while investigating a book. Cargo
rebuilds are needed only after code changes.

The command groups are `ingredient`, `amount`, `text`, `recipe`, `cookbook`,
and `corpus`. Use each group's `--help` for its arguments. Results default to
human-readable output; request `--format json` for structured data.

The file-backed extraction and replay workflows live in
`recipe_epub::review::{extract_to_run, replay_to_run}`. Both tools use those
operations for validation, canonical run identity, parent/resume handling, and
checkpointing. They return typed errors and progress; the CLI owns terminal
formatting and exit statuses, and the desktop owns its Tauri transport and view
models. Lower-level extraction and saved-run interfaces remain available.

## Source first, then extraction

Keep EPUBs, full-book outputs, and review files outside the public repository.
Inspect source before writing expectations; model output is evidence to test,
not the source of desired labels.

```sh
food-cli cookbook inspect book.epub
food-cli cookbook extract book.epub --out baseline.json
food-cli cookbook extract book.epub --out live.json --allow-network --budget-usd 7
food-cli cookbook show live.json --summary
food-cli cookbook show live.json --recipe 0
food-cli cookbook audit live.json --format json > source-audit.json
```

`cookbook extract` commands are cache-only by default and need no credentials.
A cache miss produces an incomplete saved run and exit 3. Network-enabled calls
require the existing gateway configuration. Completed chunk outputs are cached
using the model, prompt/schema version, text, and title hint. `--cache-dir` makes
the cache location explicit; saved runs are durable regardless of cache eviction.

Use `--resume` with the same book, model, and output to recover interrupted work.
Before a network request, a conservative cost reservation is checkpointed. Known
successful usage replaces its reservation; interrupted or failed requests retain
it. Unpriced models and exhausted budgets stop network work. Budget figures use
provider token rates and are estimates, not a gateway billing statement. A task's
budget covers all its runs: carry the parent with `--from` or subtract earlier
spend before allocating another independent run.

## Offline iteration

```sh
food-cli cookbook replay live.json --out reparsed.json
food-cli cookbook diff live.json reparsed.json
food-cli cookbook evaluate reparsed.json --expectations expected.json
food-cli cookbook extract book.epub --from live.json --out refreshed.json \
  --allow-network --refresh --chunk CHUNK_ID --budget-usd 10
food-cli cookbook replay reparsed.json --image-text captions.json --out illustrated.json
cargo run -p food-app -- --review-run illustrated.json
```

Replay uses saved chunk outputs with current assembly and ingredient parsing. It
never invokes the model. New output paths preserve baseline evidence. Refresh is
explicit and can select one or more chunk IDs; all original slots are retained so
missing chunks remain continuation barriers. A changed prompt requires a new
extraction run; saved runs remain replayable offline.

The desktop Review view displays every source document, including those without
an extracted recipe, alongside assembled recipes and ingredient attribution.
Load the matching source EPUB to view images; its hash is checked. Review notes
are stored separately beside the run as `*.review.json`, with version and source hash.

Some EPUB photo captions exist only inside bitmaps. `replay --image-text` accepts
`SourceImageText` (source SHA-256, transcription/OCR method, and images with archive
paths and printed recipe captions). These source annotations are saved as run
inputs. Caption matches can associate one photo with several recipes; explicitly
annotated images without recipe captions are removed as accidental heroes. This
is opt-in source evidence, not an assumption that adjacent images depict a recipe.

## Expectations and exit codes

Exact expectations use the `recipe_epub::review::Expectations` JSON shape: EPUB
SHA-256, expected recipes, and optional ingredient labels with recipe/section/line
coordinates. Labels score name, amounts, modifier, optional, and usage against the
saved parse. Input mismatches fail rather than scoring a different line.

Source-only expectations use `kind: "source_coverage"` and the
`SourceExpectations` shape. These check recipe identity, ingredient order and
groups, description, yield, optional source-authored notes, and preservation of each method paragraph in methods
or notes. Coverage is separate from semantic ingredient accuracy and does not
prove the absence of fabricated method text. Optional `check_image`/`image` and
`references` expectations additionally check source-authored image and ingredient
link associations. Instruction and note hyperlinks remain in source provenance;
the existing public recipe reference format describes ingredient lines.

`extract` and `replay` emit compact summaries; `show` displays a compact overview or a
selected chunk/recipe (`--format json` returns the complete structured result). Native extraction uses numbered source assignments, validates
complete non-overlapping ownership, and copies text from the source. `audit` lists
source block matches and unassigned blocks, hyperlinks, and images. These are
review evidence; unassigned book prose is not automatically a missing recipe.

Cookbook commands emit results to stdout and progress/errors to stderr. Exit 0 means the command
completed, 1 is an operational error, 2 is invalid command syntax, 3 is incomplete
extraction, and 4 is an expectation mismatch. `inspect` and `show` are read-only
inspection commands: exit 0 does not certify the inspected run's completeness.

`corpus sample` emits source rows and a manifest without overwriting labels;
`corpus verify` checks them and the benchmark against local books. `corpus evaluate`
and `corpus compare` score and compare saved outputs. Frozen sampling and scoring
protocols are documented in the cookbook corpus README.

`cookbook scan DIRECTORY` inventories source EPUBs offline, in path order. Use
`--limit` to bound source inspection. It never extracts recipes or calls a model.
`cookbook diagnose BOOK` is an explicit network diagnostic that bypasses cache
and reports malformed extraction payloads; use `--raw` to include those payloads.

## Ingredient-name distribution

```sh
food-cli cookbook stats illustrated.json --limit 20
food-cli cookbook stats illustrated.json --name salt --examples 10
food-cli cookbook stats illustrated.json --max-count 1 --sort name --format jsonl
```

Statistics use the saved parsed names exactly, without reparsing, case folding,
merging synonyms, or calling a model. Replay first when you want counts from the
current parser. Empty names are retained so failures remain visible. Shape
mismatches between saved recipes and parses fail with exit 1 and a replay hint.
Incomplete runs can be inspected (exit 0); `complete: false` marks partial counts.

With `--format json`, the output includes book-wide occurrence, recipe, unique-name, and singleton totals,
plus filtered name records. `matching_names` counts matches before `--limit`;
book totals remain unchanged by filters. Name records contain occurrence counts,
distinct recipe counts, distinct original inputs, and original-line examples
with source URLs and recipe/section/line coordinates. `--examples` defaults to 3;
`--format jsonl` emits only name records, one per line. Sorting uses descending occurrences
(default), descending recipes, or exact name order, with name order breaking ties.

In desktop **Cookbook → Review**, open the saved run and select **Ingredient
names**. Search, sort, or select **Used once** to inspect the distribution's tail.
Bars are relative to the most frequent name; percentages use all ingredient
occurrences. Selecting a name shows every original occurrence. **Open source**
returns to its source document, clearing source filters so it remains visible.
The desktop caches the same shared statistics on opening the run.
