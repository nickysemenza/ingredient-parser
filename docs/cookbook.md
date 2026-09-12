# Cookbook extraction

The `cookbook` crate turns an EPUB into a book tree: chapters of recipes,
techniques, and essays, with sub-recipe dependency edges, photos, and
provenance back to the source lines. Ingredient lines are parsed by the core
`ingredient` parser. Models are reached through Cloudflare AI Gateway; the
crate never chooses a model per user, it walks a ladder chosen by the
evaluation harness below.

## How it works

1. **Open**: `META-INF/container.xml` → OPF → spine, metadata, cover, contents
   (`nav.xhtml` and/or `toc.ncx`, DOCTYPEs allowed).
2. **Clean**: every spine document becomes lines, one per block element,
   each carrying its tag, classes, element ids, links, images, heading level,
   figure context, and page-break markers. Self-closing non-void tags are
   expanded first so html5ever cannot let a `<span …/>` swallow a title.
3. **Lines**: the whole book is one line stream; document and page
   boundaries are provenance, not walls.
4. **Chunk**: ~12k-character windows that break at contents targets,
   headings, or title-like lines, never between a title and its ingredient
   run. Guessed boundaries carry the last title as a continuation hint.
5. **Extract**: each chunk goes to the ladder's first model, which answers
   with *line numbers* per field (title, description, yield, times,
   equipment, sections of ingredients and steps, notes, photos), plus
   captions, chapter headings, and ignored lines. Rust copies the text and
   enforces that every line is claimed exactly once. A rejected answer is
   retried once with the fault as feedback, then handed to the next model.
   Each catalog model carries a `reasoning` setting (`--reasoning` overrides
   it for a run): Gemini 2.5 Flash reads at `low`, because its default
   thinking doubled every call's latency for the same recall.
6. **Cross-check**: contents recipe titles vs extracted titles. Missing
   titles flag their chunk; thin unlisted recipes are phantoms.
7. **Second opinion**: flagged chunks are re-read by the next model; the
   better answer wins by flags, contents matches, then parsable ingredients.
8. **Escalation**: if more than a quarter of chunks are flagged or recall is
   under 85%, the whole book is re-run with the next model up.
9. **Assemble, refs, names**: continuations merge, variations attach to
   their parent, photos bind (hero above the title, captioned figures to the
   recipe their caption names), references resolve by anchor → page → title,
   and every item gets a name that is unique within the book.

Every call, cached or live, is a `CallRecord` in the `RunReport`, with model,
tokens, cost, latency, and outcome.

## CLI

All commands take `--format json`.

```sh
cargo run -p food-cli -- cookbook inspect BOOK.epub [--chunks] [--nav] [--pages] [--chunk k003] [--lines 120..160]
cargo run -p food-cli -- cookbook estimate BOOK.epub
cargo run -p food-cli -- cookbook extract BOOK.epub [--dump DIR] [--out RUN.json] [--ladder a,b,c] [--concurrency 16]
cargo run -p food-cli -- cookbook replay --dump DIR            # re-run the pipeline from a dump, no credentials
cargo run -p food-cli -- cookbook explain RUN.json --title "Sour Cherry" [--book BOOK.epub]
cargo run -p food-cli -- cookbook runs
cargo run -p food-cli -- cookbook sample RUN.json --n 5 --seed 1
cargo run -p food-cli -- cookbook sample --library ~/Calibre --n 3
cargo run -p food-cli -- cookbook models
cargo run -p food-cli -- cookbook expect BOOK.epub [--from-run RUN.json] --out KEY.json
cargo run -p food-cli -- cookbook eval [--book slug …] [--replay DIR] [--dump DIR] [--out REPORT.json] [--reasoning low]
cargo run -p food-cli -- cookbook library ~/Calibre --dry-run                      # classify and estimate, spend nothing
cargo run -p food-cli -- cookbook library ~/Calibre --sample 25 --seed 1 --max-cost 8 [--books 2] [--only "zuni"] [--force]
cargo run -p food-cli -- cookbook report --library ~/Calibre [--out REPORT]       # worst-first, from the run store
```

Exit codes: 0 ok, 1 error, 3 some chunk failed every model, 4 the evaluation
gate failed.

`extract --dump DIR` records every request and response; `replay --dump DIR`
answers from it, so changes to validation, assembly, or references can be
re-tested for free. A changed prompt or chunking invalidates a dump.

## Credentials and paths

- `CLOUDFLARE_AI_GATEWAY_BASE_URL` (the gateway root,
  `https://gateway.ai.cloudflare.com/v1/<account>/<gateway>`) and
  `AI_GATEWAY_API_KEY`, from the environment, a repo `.env`, or
  `~/Library/Application Support/ingredient-parser/gateway.env`.
- Chunk cache: `~/Library/Caches/ingredient-parser/cookbook/`. AI Gateway also
  keeps every answer for 30 days (`cf-aig-cache-ttl`), so a browser re-extraction
  of the same book, which has no local cache, is free and instant; a gateway hit
  is recorded as a cached call with no cost. `--no-cache` bypasses both, for
  measurement only.
- Runs: `~/Library/Application Support/ingredient-parser/cookbook/runs/`
  (`COOKBOOK_RUNS_DIR` overrides), with `index.json` of run summaries kept
  up to date by `save` and reconciled on every listing.
- Answer keys: `~/Library/Application Support/ingredient-parser/cookbook/expectations/<slug>.json`.
- Library: `~/Library/Application Support/ingredient-parser/cookbook/library/`
  holds `sha-cache.json` (each EPUB's sha256 by size and mtime) and the
  sweep reports `library-<stamp>.{json,md}`.

## Evaluation gate

Answer keys are authored from the source EPUB (contents and XHTML), never
from an extraction: `cookbook expect` writes a skeleton with the contents'
recipe titles; a person curates `titles`, `variants`, `not_recipes`, and
`samples` (ingredient and step counts, section names, note labels,
cross-references). `cookbook eval` scores title recall, phantoms, non-recipe
leaks, sample accuracy (±1 line), line coverage, cost, wall time, and ETA
error, and passes when mean recall ≥ 97%, phantoms ≤ 1 per book, no leaks,
and full coverage.

To choose the ladder: run `eval --ladder <model> --no-second-opinion
--no-escalation` per candidate to rank models by recall and measure their
throughput priors, then combine the top few cheap→strong and take the first
ladder that passes the gate; paste the ladder and priors into
`cookbook/src/models.rs`.

### Authoring an answer key

`cookbook expect BOOK.epub --from-run RUN.json --out KEY.json` writes the
contents titles, the run's techniques and essays as `not_recipes`
candidates, and eight empty `samples` stubs, and prints the run's missing
and phantom titles as hints. The labelling itself never reads the run:
unzip the EPUB, read the contents (`nav.xhtml` or `toc.ncx`) and the OPF
spine, and walk the XHTML in spine order.

- `titles`: every contents title confirmed as a heading followed by an
  ingredient list, plus recipes the contents omit.
- `variants`: titled variations with their own ingredient list → `{title, of}`.
- `not_recipes`: each candidate read in its XHTML; one with an ingredient
  list moves to `titles`; add essay headings that sit directly over a recipe.
- `samples`: ingredient and step counts taken from the HTML elements, the
  section names (`null` for the unnamed one), one `notes_contain` label, refs
  from `<a href>` links to other recipes, and the photo count.
- `shape`: from the markup (`publisher-epub3`, `publisher-classes`,
  `publisher-tables`, `publisher-div-based`, `calibre-page-split`,
  `calibre-typographic`).

An Opus subagent can do the walk, one book per agent; spot-check two
samples per book by hand.

## Library sweep and tuning loop

`cookbook library DIR` hashes and deduplicates the library, classifies every
book (the cheap cached model call decides only the ambiguous ones), skips
books the store already has a run for, takes a seeded sample spread across
authors when asked, and extracts a few books at a time under `--max-cost`.
Every book ends up as a row in the report, extracted or not. `cookbook
report` rebuilds the report from the run store without extracting, so after
a tuning change only the touched books are re-run:

```sh
cookbook library ~/Calibre --dry-run                                  # classify everything, project the cost
cookbook library ~/Calibre --sample 25 --seed 1 --max-cost 8          # the sample sweep
cookbook report --library ~/Calibre                                   # worst first
cookbook explain RUN.json --title "…" --book BOOK.epub                # unclaimed lines, calls, flags
cookbook inspect BOOK.epub --lines A..B
# fix one deterministic rule with an rstest case; cargo nextest run -p cookbook
cookbook library ~/Calibre --only "<title>" --force                   # free when the request bodies did not change
cookbook eval --replay DUMPS/<setting>                                # the gate must not regress
```

Both caches hash the full request body, so changes in `validate`,
`assemble`, `refs`, `names`, and `crosscheck` re-run for free, while changes
in `chunk.rs` or `lines.rs` alter the chunk text and cost a real re-run of
the affected books; a changed validation feedback wording only affects
retry bodies.

| Report signal | Look in |
|---|---|
| missing titles, `missing_nav_title` | `chunk.rs` (a title cut from its ingredient run), `lines.rs` title detection, `crosscheck.rs` title matching and nav resolution |
| phantoms, `phantom_title`, `caption_as_title` | `validate.rs` (labels, captions, section headings), `crosscheck.rs` thin-recipe rule, `assemble.rs` (variation attach, retitle) |
| failed chunks, incomplete, transport errors | `contract.rs` lowering faults, `validate.rs` hard faults, chunk budget, timeouts and rate limits in `run.rs` |
| `unassigned_lines`, `ingredient_like_ignored`, `low_amount_parse_rate` | `validate.rs` thresholds, `lines.rs` quantity detection, parser gaps (corpus) |
| `prose_ingredients`, `recipe_without_steps` | `validate.rs`, `assemble.rs` |
| unresolved references | `refs.rs` (anchor → page → title), `lines.rs` link extraction |
| ingredient parse rate | `parse.rs` and the `ingredient` crate |
| eta error | `eta.rs`, catalog priors in `models.rs` |

## Consumers

- `food-cli` (this document).
- The Tauri desktop app (`food-app`) drives the same `Book` API natively.
- Cubby consumes `cookbook::wasm` through its `recipebridge` crate; its
  server is a signed forwarder that adds the gateway key to the request the
  crate built.

## Local subscription backends

The CLI and native desktop app can run the **unmodified, installed Claude Code
or Codex CLI**, signed in through that CLI's own subscription login. Credentials
are never copied into this application. Finder-launched Tauri also checks
`~/.local/bin`, `/opt/homebrew/bin`, and `/usr/local/bin` for the executables.
`cookbook models --format json` retains its model array and includes local model
entries with readiness information. Availability still depends on your account.

```sh
food-cli cookbook extract BOOK.epub --backend claude-cli --model opus
food-cli cookbook extract BOOK.epub --backend codex-cli --model gpt-5.6-sol
food-cli cookbook extract BOOK.epub --backend codex-cli --model gpt-6-astra
food-cli cookbook estimate BOOK.epub --backend claude-cli
```

Gateway remains the default. Local backends use one selected model, high effort,
and one concurrent call by default. Validation can request a corrected answer
from that same model. **Reported allowance exhaustion terminates the job; it
never switches models, accounts, credentials, or backends, purchases credits,
consumes resets, or waits for a quota reset.** Completed response-cache entries
remain reusable on manual retry. Model/CLI errors surface as job errors.
Subscription limits reported as throttling are handled conservatively as terminal.

Subscription usage is not a `$0` API call. Token counts and the actual model
(where exposed by the harness) are recorded; dollar costs are unknown. Provider
account settings can allow paid credits before a harness reports exhaustion.
This application does not change those settings or promise to prevent such
provider-managed billing. Use `--max-books`, not `--max-cost`, for subscription
library sweeps. CLI configuration and unrelated tools are isolated per call;
Claude's `--bare` is deliberately avoided because it disables subscription login.

New executor dumps include the neutral request and structured response under
`executor-calls/`; old HTTP dumps and cache entries remain readable. Replay must
use the original backend/model and extraction settings, and never authenticates
or runs a live model.

## Structural catalogs (experimental)

A catalog is a full-source structural map: recipe boundaries, variations,
continuations, chapter references, non-recipe regions, reference candidates,
layout observations, and unresolved concerns. It does not replace extraction,
train the ingredient parser, or become an evaluation answer key.

```sh
food-cli cookbook catalog BOOK.epub                         # Opus, no auditor
food-cli cookbook catalog ~/Calibre --dry-run              # no model calls
food-cli cookbook catalog ~/Calibre --only dessert --max-books 3
food-cli cookbook catalog BOOK.epub --auditor codex-cli/gpt-5.6-sol
food-cli cookbook extract BOOK.epub --backend claude-cli --use-catalog
```

Tauri exposes the same actions on an opened book and on books selected in the
library. Opus is the default reader. Sol audits uncertain regions only when
selected. Astra is never chosen automatically. Both required logins are checked
before an audited job starts. If the auditor exhausts its allowance, the job
fails and retains its primary-reader checkpoints; it is not silently published
as unaudited.

Maps are saved beside `runs/` in `catalogs/<epub-sha>/`, with cleaned-source and
contract fingerprints. Every successful window is checkpointed atomically.
Repeating the same operation resumes it; `--force` archives the prior map and
rebuilds from reusable request caches. Model observations retain source line
references; conflicting candidates remain visible as uncertain. The desktop
map inspector opens nearby source text for each mapped item.

Catalog use is **opt-in pending measured quality improvement**. A compatible
finished map can guide chunk boundaries and provide per-chunk evidence across
any backend. Every source line is still processed and the source takes
precedence over catalog hints. Stale, failed, and missing maps never trigger
automatic regeneration; extraction proceeds without them. Run reports record
the catalog ID and missing unambiguous catalog titles independently of the
existing contents-based recall score.

### Reproducible backend comparison

Generate three independently authored fixture books and answer keys:

```sh
cargo run -p cookbook --example catalog_fixtures -- /tmp/catalog-eval
COOKBOOK_RUNS_DIR=/tmp/catalog-eval/runs food-cli cookbook eval \
  --expectations /tmp/catalog-eval/expectations --backend claude-cli \
  --out /tmp/catalog-eval/baseline.json
COOKBOOK_RUNS_DIR=/tmp/catalog-eval/runs food-cli cookbook catalog /tmp/catalog-eval
COOKBOOK_RUNS_DIR=/tmp/catalog-eval/runs food-cli cookbook eval \
  --expectations /tmp/catalog-eval/expectations --backend claude-cli --use-catalog \
  --out /tmp/catalog-eval/guided.json
```

Compare recall, phantoms, leaks, ingredient/step counts, coverage, and elapsed
time. Include the catalog windows' usage and construction time when judging
upfront cost. Passing synthetic fixtures does not establish quality across the
full real-book evaluation library.

### Initial live validation (2026-09-12)

On the independently authored `split-spine` fixture, Sol extraction passed with
and without an Opus catalog: 100% title recall, sample accuracy, and line
coverage, with no phantoms or leaks. The successful baseline took 18.7 seconds;
the guided run took 15.3 seconds, plus 24.6 seconds to build the Opus catalog.
These single runs establish functionality, not a quality or speed improvement.
The catalog retained its ambiguous sidebar classification as uncertain. A saved
Sol executor dump also replayed successfully without model calls.

Native macOS QA verified CLI readiness, the saved catalog's resume action, and
source-line inspection. Opus cataloging succeeded, but direct index-based
recipe extraction received an Opus 5 provider refusal (`reasoning_extraction`)
on this fixture. The application surfaced the refusal and kept Claude selected;
it did not switch to Sol or Gateway. Catalog guidance remains opt-in, and Opus
extraction availability must not be inferred from successful cataloging.

The optional Sol audit also completed on the uncertain window, reusing the
primary Opus response from cache and preserving the map's unresolved concerns.
