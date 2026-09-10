# Dessert Person recovery and model comparison — 2026-09-09

## Completed recovery

The new Kimi K2.7 Code v8 run is complete: **59/59 chunks, 111 recipes,
zero failed or unresolved chunks**. The original v5 run remains unchanged
(33/64 chunks, 53 recipes, 31 failures). Different chunk totals reflect corrected
recipe boundaries, not skipped source. All 14 non-recipe chunks returned validated
empty output; no filename-based suppression was used.

The reviewed source inventory matches all 111 recipe titles, ordered ingredient
lists, required method paragraphs, and variations. All 210 headnote/combined
metadata checks pass. Buckwheat Blueberry Skillet Pancake has its ten ingredients
and six method paragraphs. The remaining `possible_method_in_notes` flag is
explained: Brioche Dough's hand-mixing alternative follows a printed `rvh1`
Variation heading and is preserved in notes; the primary method is complete.
The flag remains visible rather than being suppressed.

Saved run (registered in desktop history):
`~/Library/Application Support/ingredient-parser/cookbook-evaluation/2026-09-09/dessert-recovery/kimi-complete.json`.
Adjacent source inventories, score files, trial runs, and `budget.json` retain
the evidence. Recovery used four concurrent requests after successful serial and
four-request diagnostics. Its 54 new request groups required 58 attempts; five
exact-compatible diagnostic outputs were reused. The run's new estimated charge
is $0.772756; diagnostics are included in the Kimi group total below.

| Budget group | Known token estimate | Unresolved reservation | Remaining cap |
|---|---:|---:|---:|
| Kimi ($10) | $0.849814 | $0 | $9.150186 |
| GLM ($5) | $0.141321 | $0.056134 | $4.802545 |
| DeepSeek / Gemma ($5) | $0.105326 | $0.011057 | $4.883617 |

These totals include failures and retries. Unknown charges remain reserved;
no budget was transferred. No default model was changed.

## Method

The original Kimi v5 extraction retained 53 recipes with 33/64 chunks completed.
It had 24 request-send failures, three title errors, three duplicate metadata
assignments, and one malformed array. Historical transport causes were not recorded.

The source-only inventory was frozen before new model outputs: publisher `p.rt`
headings, `ril/rilf` ingredient paragraphs, and `rp/rpf` methods. Initial inventory
SHA-256: `9c1c86d39bf459b55aff8679604c7bea2a4ad0f1cb1a690781e7f1b9ed43d452`.
Explicit source-backed label corrections were retained alongside it:
`rvh1` headings distinguish variations from required methods, and `rt2` marks
Concord Grape Jam as a separate component recipe. The reviewed inventory contains
111 recipes after also separating four `p.RT` focaccia topping variations and
removing the printed VARIATION label from the Bialys title. The optional-topping
cross-reference is preserved as a note. These are agent-reviewed source labels, not human-adjudicated gold.

Comparison uses five v8 chunks: copyright prose, the first loaf-cake group,
a complete two-chunk continuation group, and the first foundation-recipe group.
There are 13 labeled recipes. Every candidate uses the same source and labels.
Held-out parser books were not used. Native source text is copied by index;
valid responses still require independent inventory/ingredient/method checks.

The three separate additional caps are Kimi $10, GLM $5 combined, and
DeepSeek/Gemma $5 combined. A file-locked ledger reserves each trial before
launch, counts failed calls, and retains unknown charges. No budget transfers.
All owned EPUB text, output files, labels, and the ledger remain outside the
repository under the per-user cookbook-evaluation directory (`dessert-recovery`).

## Candidate observations

| Trial | Completed groups | Known token estimate | Charged/reserved | Elapsed | Attempts |
|---|---:|---:|---:|---:|---:|
| kimi-serial | 1/1 | $0.018679 | $0.018679 | 51.8s | 1 |
| kimi-four | 4/4 | $0.058379 | $0.058379 | 150.6s | 4 |
| glm-47 | 1/5 | $0.005779 | $0.061913 | 838.0s | 9 |
| glm-53 | 5/5 | $0.106988 | $0.106988 | 117.1s | 5 |
| glm-53-flash | 5/5 | $0.011439 | $0.011439 | 161.0s | 5 |
| glm-53-flash-repeat | 5/5 | $0.017116 | $0.017116 | 181.8s | 6 |
| deepseek | 5/5 | $0.034963 | $0.034963 | 498.8s | 5 |
| deepseek-repeat | 5/5 | $0.039714 | $0.039714 | 162.2s | 5 |
| gemma | 5/5 | $0.013011 | $0.013011 | 212.3s | 6 |
| gemma-repeat | 4/5 | $0.017639 | $0.028696 | 286.3s | 8 |

Elapsed time includes local source processing and waiting, not just inference.
Concurrency differs between initial trials (one or four), and source lookup was
optimized during recovery; these timings do not establish a fair latency ranking.

Kimi's combined diagnostic sample, GLM-5.3, GLM-5.3-Flash (both runs), DeepSeek (both runs),
and Gemma's first run matched all 13 titles, ingredient lists, main methods, and
variations. GLM-4.7-Flash processed the prose group but failed all four recipe
groups (duplicate ownership and timeout). Gemma's repeat lost one group after
no structured payload; the remaining ten recipes matched.

GLM-5.3-Flash is the cheapest candidate with repeated clean sample evidence.
It remains a sample-backed recommendation, not a replacement for the default
Gemini baseline or proof of whole-library accuracy. Kimi remains the required
full-book recovery model. Original Gemini evaluation evidence is in
[cookbook-model-evaluation-2026-09-09.md](cookbook-model-evaluation-2026-09-09.md);
it is a historical baseline on different samples, not a same-sample head-to-head.

## Implementation

- Recognize publisher recipe-title markup before chunking; preserve combined
  timing/category source lines once in notes and keep content ownership strict.
- Persist structured native failure causes, status, request identifiers, and
  Retry-After when available. Bound transient and payload retries together to
  two attempts, with all unknown usage conservatively reserved.
- Index normalized source blocks once per document and avoid whole-book replay
  when admission changes only accounting. Ambiguous repeated text stays ambiguous.
- Lightweight audit returns the same checks as desktop; `--attribution` opts
  into source-block matches. The original Dessert Person run measured 35 ms for
  lightweight audit and 892 ms for full attribution after the change.
- Use responsive terminal tables while preserving plain pipes and machine JSON.

## Pricing sources

Verified 2026-09-09 against Cloudflare model pages:
[GLM 4.7 Flash](https://developers.cloudflare.com/workers-ai/models/glm-4.7-flash/),
[GLM 5.3 Flash](https://developers.cloudflare.com/workers-ai/models/glm-5.3-flash/),
[GLM 5.3](https://developers.cloudflare.com/workers-ai/models/glm-5.3/),
[DeepSeek V4 Flash](https://developers.cloudflare.com/workers-ai/models/deepseek-v4-flash-0731/),
[Gemma 4](https://developers.cloudflare.com/workers-ai/models/gemma-4-26b-a4b-it/).
Rates include cached-input discounts where documented. Estimates are not billing
statements and exclude credit-purchase fees.

## Validation

- Relevant Rust suite: 229 tests passed; focused transport, pending-source
  classification, and responsive-table regressions also passed after final edits.
- Clippy for recipe-epub, food-cli, and food-app (all targets, warnings denied),
  no-default-features WASM build, and generated-binding checks passed.
- Frontend build and 20 desktop browser tests passed.
- Native macOS smoke testing verified automatic library/history, specific shared
  failure details, the model dropdown and changing cache/cost previews. The
  completed Kimi extraction opens from history in the rebuilt app.
- PTY model tables fit 40, 100, and 160 columns; NO_COLOR output has no ANSI
  sequences. Plain redirected and JSON paths remain available.
