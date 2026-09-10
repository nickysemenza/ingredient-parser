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

## Saved runs and model selection

`cookbook extract BOOK` saves automatically. On macOS the durable store is
`~/Library/Application Support/ingredient-parser/cookbook-runs/`; Linux uses
`$XDG_DATA_HOME` (or `~/.local/share`) and Windows uses `%LOCALAPPDATA%`.
`RECIPE_EPUB_RUNS_DIR` overrides the root for tests or portable installations.
Runs are grouped by EPUB SHA-256 and named with title, model, timestamp and a
unique suffix. `--out` remains available. Explicit paths and opened historical
files are registered in place. The summary index can be rebuilt for managed
runs; external files must be reopened if their index entries are lost.

```sh
food-cli cookbook models --format json
food-cli cookbook runs --book book.epub --format json
food-cli cookbook extract book.epub --allow-network --dry-run --format json
food-cli cookbook extract book.epub --allow-network --model gemini-2.5-flash
```

The desktop model dropdown and CLI use the same catalog and preflight. The
desktop Extract button authorizes that operation; browsing stays offline and
CLI network calls still require `--allow-network`. The default budget is $10.
Preflight reports reusable chunks, pending requests, an approximate cost range,
and a separate conservative reservation. Execution revalidates source, prompt,
cache and budget. Small-sample output estimates are uncertain, particularly for
reasoning models and long continuation groups.

In the library, expand a book's saved extractions or open Extraction history.
History includes completion, model/prompt, date and new estimated spend, with
Open, Compare, Export and Reveal actions. A new extraction creates another run;
Resume requires the same source/model/prompt. Refresh creates a child and retains
untouched chunk provenance. Mixed configurations are labeled in history. Export
also copies the separate review sidecar without overwriting existing files.

Each dispatched request retains usage, reported provider usage details (including
reasoning/cache fields), rate snapshot, retry fingerprint and any error. Unknown
usage stays unknown and leaves a reservation unresolved. Local cache reuse has
zero additional charge. Inherited spend remains in the lineage budget, separate
from the new-spend subtotal. These are token-based estimates, not billing records
or credit-purchase fees.

All providers use the existing `AI_GATEWAY_API_KEY` and
`CLOUDFLARE_AI_GATEWAY_BASE_URL` configuration. Kimi uses the Gateway's unified
`/compat/chat/completions` route with the `workers-ai/` prefix; it does not require
a separate environment token. Enable Unified Billing for Workers AI in the
Gateway configuration. The Gateway ID is also sent explicitly. Existing stored
provider keys remain usable. Native network requests bypass Gateway response
caching and disable Gateway retries: the shared local cache and bounded retry
policy own reuse and accounting.

## Portable cache boundary

`recipe_epub::cache_contract::{CacheIdentity, CacheEntry}` is available with
`default-features = false`. Identities contain exact serialized requests,
provider/model configuration, prompt/schema fingerprint, and extraction-contract
version. Entry reuse requires exact identity and supported entry version.
Native entries contain validated chunk outputs before assembly, so saved results
can be replayed with newer assembly/parser code. Filesystem persistence belongs
to the native feature; no filesystem or credential dependencies enter WASM.

Cubby's current `build_chunk_request` protocol remains unchanged. Compatibility
fixtures exercise that public request against the portable envelope and prove
it cannot collide with the indexed-source contract. Cubby can implement its own
storage adapter later; this change does not add Cubby persistence, import,
deployment or synchronization.

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
food-cli cookbook audit live.json --attribution --format json > source-audit.json
```

`cookbook extract` commands are cache-only by default and need no credentials.
A cache miss produces an incomplete saved run and exit 3. Network-enabled calls
require the existing gateway configuration. Completed chunk outputs are cached
using exact request content, model/provider configuration, prompt/schema fingerprint, and extraction contract. `--cache-dir` makes
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

### Gateway configuration for desktop launches

CLI and desktop share a native configuration fallback. Exported variables (and
local `.env` values loaded on startup) take precedence over the per-user
`gateway.env` file. On macOS this file is
`~/Library/Application Support/ingredient-parser/gateway.env`; on Linux it is
`$XDG_CONFIG_HOME/ingredient-parser/gateway.env` (default `~/.config`), and on
Windows `%APPDATA%/ingredient-parser/gateway.env`.

Use the same `CLOUDFLARE_AI_GATEWAY_BASE_URL` and `AI_GATEWAY_API_KEY` (or
`CF_AIG_TOKEN`) entries as the repository `.env`. Keep this credential file
private (`chmod 600` on Unix). Finder launches read this file without requiring
a repository working directory or shell environment. No provider-specific key
is needed when the Gateway supplies credentials or Unified Billing.

### Concurrent extraction and Cubby reuse

CLI and desktop keep at most four requests in flight. As each completes, its
result is checkpointed and a free slot admits the next affordable chunk; a slow
request no longer blocks a whole batch. Each request's budget reservation is
saved before sending it. Chunks that cannot fit the current budget are revisited
when known usage releases reservations. Unknown charges retain their reservation.

Cubby's current WASM consumer uses `extract_chunks_with` with eight slots and
already shares request building, response validation/retry policy, and recipe
assembly. The durable native workflow now separates transport construction and
attempt collection from scheduling, so its real checkpoint/budget path can be
exercised with a deterministic extractor. The next useful consolidation is a
portable budget-admission and usage-settlement component, with caller-owned
checkpoint hooks. Avoid routing Cubby's legacy requests through the indexed
native protocol: their request/cache contracts remain distinct. Filesystem
persistence and browser storage stay in their respective callers.

### Progress, stopping, and history

Desktop progress and CLI stderr show completed/total chunks, active requests,
failed requests, recipes found, elapsed seconds, estimated new charges, and the
conservative spent/reserved total. Updates occur at admission, completion, and
once per second while waiting. Estimates are token-based, not billing statements.

Use **Stop extraction** in desktop or **Ctrl-C** in CLI. Both request cooperative
cancellation through the same `ExtractionControl`: no more chunks are admitted,
active requests finish and save, and the result remains resumable. An active
request may take until its transport timeout to finish. CLI returns exit 130 and
still emits the saved result and path; JSON stays on stdout, progress on stderr.
A process killed before it drains is shown as interrupted when its lock is gone.
History checks the run lock to distinguish an active extraction from interruption.

Desktop history supports **Resume…** and selecting two versions of the same EPUB
for comparison. CLI equivalents:

```sh
food-cli cookbook runs
food-cli cookbook runs --book book.epub --format json
food-cli cookbook extract book.epub --resume --out /path/from/history.json \
  --model gemini-2.5-flash --allow-network
food-cli cookbook compare /path/before.json /path/after.json
food-cli cookbook compare /path/before.json /path/after.json --format json
```

Cancellation, interruption, failure, and incomplete scope are distinct states;
complete means the entire EPUB's chunks have outputs, not that source fidelity
has been manually verified. A successful selected-chunk sample can therefore
remain incomplete at the book level. Legacy metadata is not invented.

### Continuations and source review checks

Chunk boundaries now prefer explicit EPUB title styles (`ttl`, `recipe-title`,
`recipe_title`, `recipetitle`) and repeated heading markup. The text heuristic is
only a fallback. Long titles are preserved; yield and seasoning lines no longer
create false title boundaries in documents with title markup. A necessary hard
split retains the source title as a continuation hint. Indexed outputs are put
in source order, and only the first recipe can use that hint. Assembly retains
a head containing only introduction text until its adjacent continuation supplies
ingredients; headnotes on both sides of a split are preserved.

The current indexed prompt is `2026-09-09-indexed-source-v8`. Required freezing,
unmolding, finishing, and serving actions belong in instructions, including
paragraphs that also contain an optional aside. Explicit equipment lists can
include non-food wrappers. A new extraction uses the new prompt and boundaries;
older extractions remain readable and replayable, but cannot be resumed under a
different prompt fingerprint.

Desktop **Source checks** and CLI `cookbook audit RUN` expose the same shared
review signals: unextracted/failed chunks, unresolved continuations, missing
methods, possible method text in notes, possible equipment in ingredients, and
more source yield labels than extracted recipes. Each signal links to its source;
JSON includes stable kinds and recipe/chunk coordinates. These are review cues,
not proof that a recipe is missing or that an unflagged book is complete. The
summary index versions derived checks so old history entries can be refreshed.

### Terminal tables and detailed failures

Interactive human output uses responsive tables for extraction history, models,
preflight, comparisons, source checks, and name statistics. Narrow terminals use
stacked records. `cookbook runs --paths` includes full paths; redirected human
output and JSON retain paths. JSON/JSONL remain free of terminal formatting.
Progress stays on stderr; no color is required to understand status or unknown costs.

`cookbook audit RUN` returns lightweight quality checks, including the saved
failure reason. Add `--attribution` to compute source-block matches; the returned
`documents` field is present only with that option. `show RUN --chunk ID --format
json` includes the chunk source and failure. Run JSON `charges[].attempts[]`
retains usage and structured native failure details when available. Historical
missing details remain unknown. Source checks distinguish processing failures,
unprocessed source, and content review; a failed prose chunk is not automatically
a missing recipe.

`cookbook extract --concurrency 1` is useful for provider diagnostics; the default
and maximum are four. Transient timeouts, connection failures, 429s and server
errors share the existing two-attempt limit with payload repairs, so no third
hidden retry can exceed a chunk reservation. Retry-After seconds up to 60 are
honored; longer delays stop that dispatch rather than retrying early.

The v8 prompt preserves combined timing/category lines once in notes. Source
ownership remains strict for ingredients and instructions. Publisher `p.rt`
recipe headings now guide chunk boundaries. Source matching indexes normalized
blocks once per document, retaining ambiguity for repeated wording.


## Model-by-book results

`cookbook results [--book BOOK.epub] [--format json]` and desktop **Model results**
show the latest extraction separately for each book/model/prompt configuration.
Processing success counts completed versus completed-plus-failed chunks, including
cache reuse. Always read it beside whole-book completion and pending chunks: 100%
on a sample is not complete book coverage or verified recipe fidelity. Attempts,
failed attempts, review flags, estimated new spend, and unresolved reservations
are shown for that same run. Historical unknowns stay unknown. Filter the desktop
table by book/model/prompt and open the underlying extraction for source review.
