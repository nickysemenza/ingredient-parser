# Cookbook review

`food-cli` is the supported headless entry point. `food-app` opens the same local
run files using shared library code. Build once with `cargo build -p food-cli`,
then call `target/debug/food-cli` repeatedly while investigating a book. Cargo
rebuilds are needed only after code changes.

The command groups are `ingredient`, `amount`, `text`, `recipe`, `epub`, `library`,
and `corpus`. Existing flat commands remain compatible. Use each group's `--help`
for its arguments.

## Source first, then extraction

Keep EPUBs, full-book outputs, and review files outside the public repository.
Inspect source before writing expectations; model output is evidence to test,
not the source of desired labels.

```sh
food-cli epub inspect book.epub
food-cli epub extract book.epub --out baseline.json
food-cli epub extract book.epub --out live.json --allow-network --budget-usd 7
food-cli epub show live.json --summary
food-cli epub show live.json --recipe 0
food-cli epub audit live.json > source-audit.json
```

New `epub extract` commands are cache-only by default and need no credentials.
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
food-cli epub replay live.json --out reparsed.json
food-cli epub diff live.json reparsed.json
food-cli epub evaluate reparsed.json --expectations expected.json
food-cli epub extract book.epub --from live.json --out refreshed.json \
  --allow-network --refresh --chunk CHUNK_ID --budget-usd 10
food-cli epub replay reparsed.json --image-text captions.json --out illustrated.json
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

`extract` and `replay` emit compact summaries; `show` returns the complete run or a
selected chunk/recipe. Native extraction uses numbered source assignments, validates
complete non-overlapping ownership, and copies text from the source. `audit` lists
source block matches and unassigned blocks, hyperlinks, and images. These are
review evidence; unassigned book prose is not automatically a missing recipe.

EPUB commands emit JSON to stdout and errors to stderr. Exit 0 means the command
completed, 1 is an operational error, 2 is invalid command syntax, 3 is incomplete
extraction, and 4 is an expectation mismatch. `inspect` and `show` are read-only
inspection commands: exit 0 does not certify the inspected run's completeness.

`corpus sample` emits source rows and a manifest without overwriting labels;
`corpus verify` checks them and the benchmark against local books. `corpus evaluate`
and `corpus compare` replace the former Rust example and Python scripts. Frozen
sampling and scoring protocols remain in the cookbook corpus README.

## Ingredient-name distribution

```sh
food-cli ingredient stats illustrated.json --limit 20
food-cli ingredient stats illustrated.json --name salt --examples 10
food-cli ingredient stats illustrated.json --max-count 1 --sort name --jsonl
```

Statistics use the saved parsed names exactly, without reparsing, case folding,
merging synonyms, or calling a model. Replay first when you want counts from the
current parser. Empty names are retained so failures remain visible. Shape
mismatches between saved recipes and parses fail with exit 1 and a replay hint.
Incomplete runs can be inspected (exit 0); `complete: false` marks partial counts.

JSON includes book-wide occurrence, recipe, unique-name, and singleton totals,
plus filtered name records. `matching_names` counts matches before `--limit`;
book totals remain unchanged by filters. Name records contain occurrence counts,
distinct recipe counts, distinct original inputs, and original-line examples
with source URLs and recipe/section/line coordinates. `--examples` defaults to 3;
`--jsonl` emits only name records, one per line. Sorting uses descending occurrences
(default), descending recipes, or exact name order, with name order breaking ties.

In desktop **Cookbook → Review**, open the saved run and select **Ingredient
names**. Search, sort, or select **Used once** to inspect the distribution's tail.
Bars are relative to the most frequent name; percentages use all ingredient
occurrences. Selecting a name shows every original occurrence. **Open source**
returns to its source document, clearing source filters so it remains visible.
The desktop caches the same shared statistics on opening the run.
