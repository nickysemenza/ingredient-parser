# Cross-book model evaluation — 2026-09-09

The shared extraction pipeline and model-results table are implemented in CLI and
desktop. Processing success is not a fidelity certificate: this evaluation found
fully processed runs with missing ingredients and misplaced methods. The default
model remains Gemini 2.5 Flash. No automatic model fallback was added.

## Selected book results

| Book | Completed chunks | Source checks passed | Selected extraction |
|---|---:|---:|---|
| Charred | 20/20 | 70/70 | `charred-glm.json` |
| Nopalito | 32/32 | 123/123 | `nopalito-glm-v2.json` |
| Simple Thai Food | 42/42 | 104/104 | `thai-kimi-v11-family.json` |
| Sunday Suppers | 28/28 | 100/100 | `sunday-kimi-v9-child.json` |
| Tacos | 35/35 | 103/103 | `tacos-gemini-v11-final.json` |

All filenames in this report are beneath
`~/Library/Application Support/ingredient-parser/cookbook-evaluation/2026-09-09/crossbook/`.
Original runs and failed attempts are preserved and registered for discovery.

Charred passes all 70 source checks with both GLM 5.3 Flash and Kimi K2.7 Code.
Nopalito's GLM run passes all 123 recipe/component checks. Sunday Suppers passes
all 100 source entries with both models; GLM groups one component, while a targeted
Kimi v9 correction preserves the three independent ice-cream recipes and yields.

The selected Thai run inherits GLM's other chunks and uses Kimi v11 for the curry
paste/stocks family. The selected Sunday run mixes Kimi prompt versions. The
selected Tacos run inherits validated GLM chunks and explicitly requests Gemini
for failed or incorrect chunks. These are recoveries with mixed provenance, not
independent full-book trials of the final model/prompt. Both interfaces label them
as mixed, including when the newly requested model has no successful outputs.

The wine-reference control uses three frozen chunks (welcome/navigation, wine
reference entries, copyright). GLM and Kimi both return valid empty recipe arrays.
This is a scoped non-recipe control, not a whole-book success claim.

## Remaining distinctions and model evidence

- Nopalito retains two reference-prose continuation warnings around masa shapes
  and ingredient descriptions. All inventory recipe ingredients and methods are
  present; the prose association remains visible for review.
- Thai retains one continuation hint for its pantry/equipment introduction.
  The reviewed source is reference prose, not a missing inventory recipe. The
  warning remains visible rather than being suppressed by filename.
- Sunday GLM's extra yield warning is explained by a component grouped inside
  Green Soup. The corrected Kimi child has no content-review flags.
- The complete Kimi Thai v10 run still misclassifies Pad Thai's bean sprouts and
  a Rice Soup section heading, plus two equipment/setup paragraphs. Processing
  completion alone therefore does not pass the source comparison.
- Kimi and DeepSeek each timed out twice on the same difficult Nopalito chunk
  in their final bounded trials. Further identical retries were not useful evidence.

GLM remains a promising low-cost option, not a universal recommendation. On the
completed paired Charred trial its known token-based estimate is $0.038710 versus
Kimi's $0.330897; unresolved reservations are separate. Kimi's corrected Thai
family demonstrates useful fidelity on a difficult case, but its broader failures
do not justify replacing the default. Gemini's targeted recovery is evaluated
separately from its older partial baseline evidence. Catalog descriptions reflect
these limits, including the unsuccessful cross-book DeepSeek trial.

## Budget

One **$20 additional combined cap** covers this follow-up. Earlier Dessert Person
budgets are separate. A locked ledger reserves each trial before dispatch; native
run accounting reserves before requests and counts retries and failures. Child
entries settle only new charges/reservations, so inherited spend is not counted
twice. Unknown charges are not assumed free.

| Book (all attempts and superseded trials) | Known token-based estimate | Other held reservation |
|---|---:|---:|
| charred | $0.369607 | $0.325696 |
| nopalito | $0.801161 | $1.312408 |
| sunday | $0.606335 | $0.206967 |
| tacos | $1.116637 | $1.163621 |
| thai | $0.839164 | $0.744268 |
| wine | $0.012957 | $0.000000 |
| **Total** | **$3.745862** | **$3.752960** |

Total allocated: **$7.498822**. Unallocated cap: **$12.501178**.
All ledger entries have finished. The held remainder is unresolved charges; no
future work is reserved. These are token-based
estimates and conservative reservations, not a billing statement, and exclude
credit-purchase fees. `budget.json` is authoritative.

## Source-first methodology

Problem books came from saved history: Charred, Nopalito and Simple Thai Food.
The deterministic random cohort ranks eligible Calibre cookbook-tagged EPUBs by
SHA-256 of `crossbook-2026-09-09-v1:<title>` and selects Sunday Suppers and Tacos,
with The Food Lover's Guide to Wine as a reference control. Dessert Person and the
held-out parser books (Baking at République, Bangkok, Bouchon, Burma Superstar)
were excluded. No paid extraction used those held-out books.

Publisher-markup inventories and source snapshots were frozen before paid calls.
Reviewed corrections remain separate: component heading classes, equipment versus
food, Charred's icon legend, all Nopalito `rhn-middle` paragraphs, three Sunday
`sub_headnote` paragraphs, and the Thai parent families. `manifest.json` retains
frozen inventory hashes. Full source material stays outside the repository.

The scorer checks ordered ingredients, authored methods, headnotes and equipment;
components may be separate recipes or named sections. Repeated names are matched
within their source document using ingredients. The Thai family also requires
shared preparation in an unnamed instruction section and each variation-specific
method in its own named section. The two procedural paragraphs styled as headnotes
are checked as methods. That reviewed correction exposed the initially misleading
104/104 preservation score: Gemini v10/v11 retained those methods as description,
and Kimi v10 put variant-only methods into the common section. Kimi v11 passes the
stronger check. Label corrections apply to every candidate, not just the winner.

These books were used to diagnose and repair the pipeline, so their final results
are development/regression evidence, not an unbiased estimate of accuracy on all
cookbooks. `tools/crossbook-score.py` reproduces the private source checks.

Initial trials overlapped at four requests per run. Recovery was subsequently
limited to at most four requests globally, usually one per model. End-to-end time
includes local assembly/parsing and is not an isolated provider latency benchmark.
The [Kimi](https://developers.cloudflare.com/workers-ai/models/kimi-k2.7-code/)
and [GLM](https://developers.cloudflare.com/workers-ai/models/glm-5.3-flash/) pages
expose generic reasoning parameters, but do not establish model-specific behavior
for an override. No undocumented reasoning setting was introduced. Reported
completion usage does not always include a separate reasoning-token breakdown.

## Shared fixes and verification

- Nested yield spans and decorative separators preserve separate source lines.
- Styled ingredient/method `div`s and `br` spacing are retained in source inspection.
- Numbered primary title styles participate in chunk boundaries. Full multiline
  headings are retained in continuation hints, fixing both Beet Tortillas and
  the distinct “Cochinita Pibil Tacos The Hard Way” continuation.
- Indexed non-adjacent translated titles and multiline section names retain exact
  source ownership. Duplicate ownership remains invalid.
- Recipe-level arrays and malformed responses produce precise retry feedback.
  V11 distinguishes common preparation from variant-only methods.
- Conservative offline enrichment restores source-proven preparation ingredients
  and exact duplicated headings. Ambiguous cases are not silently repaired.
- Source audits expose ingredients in notes or used as section headings. They do
  not rewrite uncertain content merely to reach zero warnings.
- Gemini uses Gateway's provider adapter on the existing compatible chat route,
  with the same Gateway token and unchanged stored keys. The prior HTTP 400 is
  configuration evidence, not a model-fidelity failure. See
  [Gateway chat completions](https://developers.cloudflare.com/ai-gateway/usage/chat-completion/)
  and [Unified Billing](https://developers.cloudflare.com/ai-gateway/features/unified-billing/).
- Explicit-path runs register their initial durable checkpoint, enabling desktop
  discovery of CLI work before completion. Summaries remain rebuildable.

`food-cli cookbook results [--book BOOK.epub] [--format json]` and desktop
**Cookbooks → Model results** share one Rust report. Rows select the latest run for
one book hash/model/prompt/configuration set. Success is completed divided by
completed plus failed chunks; pending chunks remain separately visible. New
spend, unresolved charges and inherited reservations are distinct. Unknown
historical attempts/costs stay unknown. Unreadable latest runs are reported, not
replaced with older successes. CLI tables wrap or stack; redirected/JSON contracts
remain intact. Desktop filtering includes inherited configurations and opens the
exact underlying extraction.

Validation includes Rust/CLI tests, all-target Clippy, non-native library tests,
the browser/WASM compatibility example, generated-binding checks, frontend unit
and WebKit tests, and native macOS smoke testing. The complete frontend suite
passed after retrying one page-startup timeout; focused model-results tests also
cover filtering, mixed provenance, partial coverage and opening the extraction.
Actual 50-column CLI output stacks without overflowing. Native table values agree
with the shared CLI report. The final non-native suite has 153 passing tests. No Cubby protocol change or deployment was performed.
