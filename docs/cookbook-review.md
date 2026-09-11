# Cookbook review

Desktop and CLI extraction now default to [automatic extraction and AI feedback](automatic-cookbook-extraction.md).
Manual review queues and approvals are no longer part of the app. Historical
review sidecars and the inspection/evaluation APIs below remain compatible;
historical processing success is not automated source verification.


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
# The hybrid contract is opt-in while its quality gate is evaluated.
food-cli cookbook extract book.epub --strategy hybrid --allow-network
```

The desktop model dropdown and CLI use the same catalog and preflight. The
desktop Extract button authorizes that operation; browsing stays offline and
CLI network calls still require `--allow-network`. The default budget is $10.
Preflight reports reusable chunks, pending requests, an approximate cost range,
and a separate conservative reservation. Execution revalidates source, prompt,
cache and budget. Small-sample output estimates are uncertain, particularly for
reasoning models and long continuation groups.

Inspection binds the current EPUB source documents to the run before recovery
planning, cache credit, or provider dispatch. Each document has a canonical
identity used by source enrichment and verification. If inspection finds a
changed document set or document identity, prior acceptance is cleared and
whole-book assembly caches are dropped; saved outputs, findings, attempts,
reservations, spend, and verifier responses remain auditable and are reassessed
under the current source. The workflow also hashes the EPUB again after taking
the output lock before dispatch, so a replacement during preparation cannot be
silently processed.

Indexed DOM metadata is provenance evidence, not a semantic role assignment.
Exact cleaned-line coordinates retain the source document, document line,
contributing element ordinals, tags, classes, ancestors, anchors, links,
images, and transformed-table state. The additive `ReviewRun.source_line_provenance`
payload is used when available; the single-pass `chunk_epub_indexed` inspection path
preserves legacy chunks while supplying the provenance. Legacy runs continue
through their existing chunk format and source-link fallback without guessed
DOM offsets. Recovery `State` binds the indexed source identity before planning
and again before execution. A changed identity invalidates source-backed
acceptance and cache credit while preserving outputs, findings, attempts,
reservations, spend, and verifier responses for reassessment.

Raw DOM metadata supports evidence lookup and ownership diagnostics. It does
not by itself label narrative prose as a method or make a candidate correct;
semantic roles still require extraction and verification.

The v4 frozen automatic c4 evaluation took 77.6 seconds and accepted 0/8
groups. It matched 27/27 recipe owners and 305/305 ingredients, preserved
126/134 required methods, and produced four phantom recipes from publisher
captions. Verifier false alarms were recorded with the supported findings.
This result remains a bounded evaluation record rather than a full-book quality
or throughput claim.

In the library, expand a book's saved extractions or open Extraction history.
History includes completion, model/prompt, date and new estimated spend, with
Open, Compare, Export and Reveal actions. A new extraction creates another run;
Resume requires the same source/model/prompt. Refresh creates a child and retains
untouched chunk provenance. Mixed configurations are labeled in history. Export
also copies the separate review sidecar without overwriting existing files.

Explicit child-audit preparation rebuilds inconsistent hybrid section layouts
from keyed raw extraction and the current source identity. It retains canonical
assignments and source-supported corrections, while preserving the original
output as separate migration evidence. The original parent remains untouched;
the child starts with prior acceptance invalidated. Missing or ambiguous source,
assignment, or provenance evidence stops migration instead of guessing. Legacy
indexed runs are unaffected. Hybrid lowering and source enrichment preserve
canonical fields, including explicit empty sections and text overrides; source
enrichment adds references without applying legacy title or layout repairs.
Older hybrid assembly acceptance requires fresh review.

The 36.4-second audit diagnostic accepted 5/8 groups and used $0.092 of reported
usage-priced spending. It received provider prompt-cache hits and predates the
source-enrichment fix, so it does not establish a cold speed improvement or a
quality admission.

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
# Paid AI review writes a child run and requires both explicit network and output controls.
food-cli cookbook audit live.json --ai --out audited.json --allow-network --budget-usd 4
food-cli cookbook audit live.json --ai --correct --out corrected.json --allow-network --budget-usd 4
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
AI audit corrections are child-run evidence and preserve the candidate revision,
source identity, original values, and unresolved findings. Human review decisions
remain in the review sidecar.

## Frozen extraction experiments

The native experiment commands replace the historical private Python execution
path for reusable preparation, dispatch, accounting, and reporting. Preparation
freezes the saved recovery state, source identity, contract expectations, and
request/reservation bounds without configuring a provider or touching the ledger.
Extraction preparation requires `state` and `expectations`; `--indexed-source`
is optional when the saved state already contains the bound source. Audit
preparation uses `--parent-run` and frozen `expectations` instead.

```sh
food-cli epub experiment prepare \
  --state /private/experiment/state.json \
  --expectations /private/experiment/frozen-expectations.json \
  --indexed-source /private/experiment/indexed-source.json \
  --output-dir /private/experiment/prepared
```

Execution consumes that manifest and the same frozen expectations. Select one
contract (`legacy`, `indexed`, or `hybrid`) and a new evidence directory. Omitting
`--dispatch` performs the execution planning step offline; adding `--dispatch`
is the explicit paid-provider authorization and applies the existing ledger
reservations before calls.

```sh
food-cli epub experiment run \
  --manifest /private/experiment/prepared/manifest.json \
  --indexed-source /private/experiment/indexed-source.json \
  --contract hybrid \
  --output-dir /private/experiment/hybrid-evidence \
  --ledger /private/experiment/evaluation-ledger.json \
  --expectations /private/experiment/frozen-expectations.json \
  --dispatch
```

Reporting is offline and does not admit AI quality or completion by itself. It
combines the frozen manifest, any evidence produced so far, and ledger state.
When the frozen expectations use the supported `exact` or `source_coverage`
shape, report replays the saved output and runs the shared Rust evaluator
against the copied expectations and hash recorded in the manifest. Private
provenance-only expectation formats are reported as unscored. For an audit
operation, the same supported expectations can be scored offline on incomplete
children; quality acceptance requires a completed AI audit and passing
expectations. A `frozen_source_roles_v1` bundle is also supported for audit
evaluation; it freezes the exact raw UTF-8 v1/v3 expectation bytes together
with an explicit source mapping. Singular private v1 or v3 files remain
unscored unless combined into this bundle. An extraction-only result remains
execution/accounting evidence, and report does not supply fully audited
completion time.

Build that bundle from a full native saved run without provider calls or input
state mutation:

```sh
MACOSX_DEPLOYMENT_TARGET=13.3 cargo run -p recipe-epub --example evaluate_frozen_audit -- \
  /private/experiment/audit-state.json \
  /private/experiment/expectations-v1.json \
  /private/experiment/expectations-v3.json \
  /private/experiment/source-mapping.json \
  /private/experiment/frozen-audit-result.json \
  --bundle-out /private/experiment/frozen-source-roles-v1.json
```

For a historical raw recovery state, also pass `--source-plan PLAN` to this
offline example. The companion plan binds the state's source hash and EPUB
identity before evaluation; missing or mismatched bindings are rejected.

The mapping is an explicit JSON array of `source_index` and
`original_chunk_index` entries. Use the resulting
`frozen-source-roles-v1.json` as `--expectations` in the existing audit
experiment `prepare` command. Evaluation is scoped to mapped per-chunk owner
and field preservation; it does not prove component membership. `source_roles`
remain original extraction evidence and are not rewritten by hybrid corrections;
actual output field findings are reported separately. Hybrid quality acceptance
uses current canonical assignment roles against the same frozen expectations,
with missing, conflicting, or invalid assignments blocking acceptance. Original
role mismatches remain historical diagnostics; indexed evaluation continues to
use its stored source roles.

Manifest-based audit execution is available for freezing a parent candidate and
complete groups, running bounded correction and re-audit under the evaluation
ledger, and reporting findings and history. The explicit `audit` operation uses
the same `prepare`, `run`, and `report` commands and has no separate
audit-prepare command. Audit reports currently provide AI evidence and
accounting plus offline scores for supported generic expectations and
`frozen_source_roles_v1` bundles; unsupported expectation shapes remain
unscored. The native
`paid_throughput_evaluation --audit-state` example remains a diagnostic path,
and standalone `epub audit --ai --correct` remains available for ordinary child
runs.

Prepare an audited child without configuring a provider or touching the ledger:

```sh
food-cli epub experiment prepare \
  --operation audit \
  --parent-run /private/experiment/parent.json \
  --expectations /private/experiment/frozen-expectations.json \
  --group 5 --group 8 \
  --reviewer-model gemini-2.5-flash \
  --budget-usd 4 \
  --correct \
  --output-dir /private/experiment/audit-prepared
```

Run consumes the frozen audit manifest. It requires explicit parent and child
paths; `--dispatch` is still the only provider authorization and uses the
manifest's frozen budget and existing ledger controls.

```sh
food-cli epub experiment run \
  --manifest /private/experiment/audit-prepared/manifest.json \
  --parent-run /private/experiment/parent.json \
  --child-run /private/experiment/audit-child.json \
  --output-dir /private/experiment/audit-evidence \
  --ledger /private/experiment/evaluation-ledger.json \
  --dispatch
```

```sh
food-cli epub experiment report \
  --manifest /private/experiment/prepared/manifest.json \
  --evidence-dir /private/experiment/hybrid-evidence \
  --ledger /private/experiment/evaluation-ledger.json
food-cli epub experiment ledger status \
  --ledger /private/experiment/evaluation-ledger.json
```

The verifier and throughput evaluation buckets remain capped at $4 and $6,
including prior spend and existing unknown holds. Unknown provider usage keeps
its reservation and is never released automatically. Preparation and reporting
do not move money; dispatch must checkpoint reservations before calls and settle
or retain unknown charges durably.

AI audit correction is bounded to one correction pass followed by one re-audit.
Source-supported changes are written to a child run with source identity,
candidate revision, before/after values, and reasons. Audit proposals remain
separate from applied correction history; unsupported quantity, unit, or cooking
meaning changes stay as unresolved findings for review.

The provider-facing audit response uses compact assignment IDs and source-span
references. Its `move_assignment` record may include an optional `spans` list;
native Rust resolves the ID against the frozen candidate before validation, so
the request does not repeat complete assignment objects. Omitting `spans` moves
the whole assignment. Durable child-run history stores the typed `move_spans`
correction, its before/after assignments and spans, and the reason, so compact
wire IDs are never the only record available for later review.

Target source rows include `[line, text, assignment_ids]`, exposing every frozen
claim for that line. An empty ID list marks an ownership omission; multiple IDs
expose overlapping claims. Context-only rows remain `[line, text]`. Selected
move spans can split a broader stored span without repeating its remaining text.

The assignment table supplies field, recipe, section, and owner metadata only;
the assignment IDs on source rows are the sole membership evidence. An omitted
reference is therefore not a declaration that its source is irrelevant or
nonrecipe.

For `move_assignment`, the ID identifies the source assignment; selected spans
must belong to that ID. Moving lines from several assignments requires separate
moves. For `restore_span`, the ID instead identifies the receiving field, and
every restored line must be unassigned. Overlap with any existing assignment,
including a larger span in the same field or an earlier restore in the batch,
rejects the batch atomically. Existing ownership must be corrected with a move.
Neither operation permits invented text or guessing an uncertain owner.

Canonical assignments carry an `owner_chunk` as well as the chunk-local recipe
and section indexes. Empty fields retain that identity so a correction can fill
the intended field without guessing from its text. Section splits use the
source chunk in `at`; section merges identify their source chunk explicitly.
Legacy assignments with no owner remain readable. Child-audit migration infers
ownership only from consistent source spans or a single-chunk group; ambiguous
empty fields remain unresolved and cannot receive automatic corrections.

Hybrid owners also carry assembly placement derived inside the actual assembly
pass: either the retained final recipe and section offset or a dropped reason.
Source correction coordinates remain chunk-local. Assembly placement describes
where the assembled result landed and does not claim that assignment metadata
survived assembly.

Optional wording does not itself make a preparation action a note. Equipment
or workspace readiness for a later recipe operation belongs in instructions.
This rule guides the AI audit; source-span validation alone cannot certify the
semantic placement of a line.

A move can resolve duplicate ownership when the destination already claims
some or all selected lines. Rust removes the source claim and adds only lines
the destination does not already own, preserving its existing segmentation.
Overlapping selections within a move and reuse of a consumed claim still fail.
The whole patch remains atomic and requires re-audit before acceptance.

New audit responses place five operation-specific correction lists at the root,
alongside coverage, findings, and an optional context-expansion request.
Each record requires its own arguments and an `order` number; the combined
numbers must be consecutive from zero. Rust restores that order before applying
the atomic patch, preserving interleaved operations. Empty lists explicitly
mean no correction of that kind. Rust binds source identity, candidate revision,
and group from the exact current request rather than asking the model to copy
them. A changed source, revision, or request rejects the response before any
correction is applied. Older echoed-identity, grouped compact, flat compact, and
full-assignment responses remain decodable for their saved contracts; the
durable correction history format is unchanged.

An audit may request one bounded source-context expansion when the compact
context is insufficient. That response records the source identity, candidate
revision, group, request key, and reason, and must contain no coverage,
findings, or corrections; an expansion is evidence about context selection,
never acceptance. A correction invalidates acceptance and triggers one bounded
final re-audit of the full corrected group, retaining every target line and
relevant linked region. That final response returns source-grounded findings
with all operation-specific correction lists empty: no second correction pass is
available. A prior explicit expansion remains full; corrected re-audits cannot
request another expansion. Historical extra patches remain readable as
evidence but cannot create acceptance.
This expansion path is opt-in within an audit and does not change the indexed
default extraction strategy.

Navigation documents are excluded only when EPUB semantics establish that role.
Targets, continuations, outgoing links, and exact reciprocal links remain in
the compact audit context. An ambiguous incoming link that is reciprocal-only
is deferred with compact reference evidence; its linked source is expanded
only when the reviewer or audit needs it to resolve ownership. The reviewer
must expand that context before applying a correction that depends on it.
Omission from the compact context does not classify the source as irrelevant or
nonrecipe.

An outgoing link to a unique indexed `div`, `section`, or `article` fragment
can supply its complete source subtree instead of unrelated sibling content.
Selection follows DOM coordinates across source chunks, not CSS class names.
Heading-only targets and missing, conflicting, or ambiguous provenance retain
compact reference evidence until expansion is needed. Every target line remains
present, and the existing full-context expansion restores omitted surroundings.

For exact indexed evidence, the hybrid audit sends DOM entries referenced by
the selected source lines, including anchor-only entries and every ancestor.
Entries used only by omitted lines remain in the local source index and return
when context expands. Source coordinates, links, images, and transformed-row
evidence are preserved; legacy ambiguous provenance keeps its existing form.

Runs record the extraction strategy (`indexed` or the opt-in `hybrid` contract),
so comparisons and replay retain the contract that produced each result. The
desktop extraction dialog exposes the same selector and defaults to indexed.

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

CLI and desktop default to four requests in flight and support one through eight.
The default remains four until matched quality, cost, and timing trials justify
increasing it. As each completes, its
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
is four and the maximum is eight. Transient timeouts, connection failures, 429s and server
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

## Offline source-layout experiment

`chunk_epub_indexed` exposes the existing cleaned chunks with one source-coordinate
record per emitted line. Element IDs use the same namespace as source inspection;
ancestor metadata comes from the same parsed DOM. Rendered table rows are explicitly
marked transformed. The ordinary `chunk_epub` API projects these same chunks.

Two offline examples support development measurements without provider calls:

```sh
MACOSX_DEPLOYMENT_TARGET=13.3 cargo run -p recipe-epub --example benchmark_source_index -- \
  book.epub previous-inspection.json /private/path/source-index.json
MACOSX_DEPLOYMENT_TARGET=13.3 cargo run -p recipe-epub --example source_layout_candidate -- \
  --epub book.epub --profile /private/path/layout-profile.json \
  --out /private/path/candidates.json \
  --role-assignments /private/path/residual-role-assignments.json \
  --residual-out /private/path/residual-roles.json
python3 tools/audit_source_layout.py /private/path/candidates.json \
  /private/path/frozen-source-inventory.json
```

Capture `previous-inspection.json` with `cookbook inspect --format json` before
changing the indexing implementation. The benchmark requires exact legacy chunk
serialization and complete coordinate coverage. Its timing is indexing only;
it does not measure extraction or control filesystem caches.

The candidate profile has a pinned `epub_sha256`, `schema_version: 1`, a
`recipe_container` selector, field `roles`, selector-count `required` guards,
and `defer_if` selectors. Selectors are explicit `{tag, class}` objects, with
either field optional. Each requirement has `selector`, `min`, and optional
`max`. Unknown profile fields are rejected. Rules belong to the pinned book,
not to a global interpretation of publisher class names.

`roles.ingredient_section_name` is an optional selector for headings that
partition ingredient components only. It never claims following method rows:
shared method rows before every component or after all components stay together
in an unnamed recipe-level section in source order. A method interleaved with a
component ingredient span remains deferred. Use `roles.section_name` for
headings that really bound both ingredients and methods, such as a method or
variant section. A region containing both heading kinds is deferred because
their ownership cannot be established from these selectors alone.

`roles.ignored` is an optional selector for non-recipe lines such as photo
markers inside a recipe container. Explicit `ignored` assignments use the
existing indexed payload's ignored-line list, preserving source coordinates
without adding the text to recipe notes. Conflicting roles still defer, and
ignored content remains subject to the existing validators and verification.

`--role-assignments` is optional private future-model input, never a source
expectation file. Its strict schema has `schema_version: 1`, `epub_sha256`,
`profile_sha256`, `provenance`, and `assignments`; each assignment supplies
`original_chunk_index`, `line_index`, `document_line`, and a snake-case role.
Set `role` to `null` to record an explicit unresolved decision; it remains
deferred and is preserved as assignment evidence.
The example rejects stale hashes or coordinates, duplicates, and attempts to
replace an already unambiguous inferred role. It records the assignment-file
hash, provenance, supplied entries, and applied coordinates in candidate
output. `--residual-out` writes only structurally guard-eligible unresolved
regions, with exact source context and explicitly `NONAUTHORITATIVE` proposed
roles; it is not acceptance evidence.

Candidates pass indexed lowering and the existing literal-content validator
through `indexed::parse_indexed_recipes`. They remain **unverified**. Unknown,
conflicting, transformed, or structurally ambiguous recipe regions are deferred;
outside-container prose remains unassessed. These examples do not populate
recovery caches or change extraction defaults. Independent semantic assessment,
whole-book assembly, and verification remain necessary. Inventory agreement on
ingredients and methods does not certify headnotes, variations, ownership, or
the whole book. Keep profiles, source output, and evaluation evidence outside
the repository.
