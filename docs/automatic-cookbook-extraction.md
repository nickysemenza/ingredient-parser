# Automatic cookbook extraction

Desktop Extract and `cookbook extract BOOK --allow-network` use one shared
source-verification policy. Explicit CLI `--model ID` or the desktop Advanced
model selector disables model escalation, but retains source verification.

The initial order is GLM 5.3 Flash, Gemini 2.5 Flash, GLM 5.3, then Kimi K2.7 Code.
OpenAI and Anthropic remain explicit experimental choices. No new paid evaluation
or automatic admission of catalog entries is performed.

## Acceptance

The source inventory is frozen before outputs. Recovery groups split at
source-supported recipe boundaries and retain ambiguous sections and linked
continuations. Selected chunks expand to their group. Unselected, unverified
groups keep the book incomplete.

Inspection also captures the EPUB's source documents and a SHA-256 identity for
each document. New recovery state binds that inspected document set during
preparation. Recovery preflight binds the current set on its planning clone
before estimating work or crediting recovery-cache entries; execution binds it
again before recovery-cache lookup or provider dispatch. A post-lock EPUB hash
check closes the replacement window between preparation and dispatch.

If the document set or any document identity changes, source-backed acceptance
is cleared and assembly caches are discarded. Outputs, findings, attempts,
reservations, spend, and prior verifier responses remain available as audit
evidence, but the changed source must be verified again before acceptance.

The indexed DOM record is provenance evidence, not a semantic label. Tags,
classes, ancestor coordinates, anchors, links, images, and transformed-table
flags explain which source elements contributed to an exact cleaned
`document_line`; they do not decide whether prose is a title, ingredient,
method, note, or description. `ReviewRun.source_line_provenance` and the recovery
`State` binding are additive to the legacy chunk payload. The
single-pass `chunk_epub_indexed` inspection path preserves legacy chunks while
supplying exact chunk/line/document coordinates for source links and
validation. Older runs without indexed provenance retain their legacy chunk
references and use the legacy fallback rather than inventing DOM coordinates.
Recovery state binds the same source identity before planning, cache credit, or
dispatch, and migration preserves historical evidence while requiring current
verification.

Explicit child-audit preparation rebuilds inconsistent hybrid section layouts
from the keyed raw extraction and current source identity. It retains canonical
assignments and source-supported corrections, while recording the original
output in separate migration evidence. The original parent is never overwritten;
its prior acceptance is invalidated on the child. Missing or ambiguous source,
assignment, or provenance evidence stops migration rather than guessing. Legacy
indexed runs are unaffected. Hybrid source enrichment adds reference links
without changing canonical titles or section fields. A separate hybrid assembly
contract invalidates acceptance produced before that behavior.

The 36.4-second, 5/8-group audit diagnostic cost about $0.092 and received
provider prompt-cache hits. It predates the source-enrichment fix and does not
establish a cold speed improvement or a quality admission.

Indexed output must account for every line without duplicate ownership. The
verifier receives the assembled candidate and source context and classifies each
target chunk's lines. Verification is partitioned by chunk to bound output size,
but a group is accepted only after all its chunks pass. Source-styled ingredient
omissions and missing methods fail deterministic checks even after AI approval.
Proposed replacements are reassembled with previously accepted groups.


The current verifier request contract is `source-verification-v9`. Extraction
and verification share source-role precedence: establish recipe ownership first,
then preserve required procedures in instructions regardless of headnote,
note, or variation labels. Non-procedural background remains metadata. Titles
include their subtitles and translations. The verifier checks actual candidate
roles and instruction preservation independently of non-authoritative hints.
A v1–v8 checkpoint must create a child with `--from`; its sources, candidates,
findings, and accounting are preserved, but earlier acceptance is invalidated
and verification evidence retained as obsolete for reassessment. Complete decoded candidates also move old rejection feedback into obsolete
history, so that feedback does not prevent current-contract re-verification.
Incomplete extraction failures remain active.

Verification context retains target rows, continuations, outgoing references,
and exact reciprocal references. Relative and same-document fragment links
resolve against their origin document. The assignment table carries field and
owner metadata only; source-row assignment IDs are the sole membership claims.
An ambiguous incoming reference that is reciprocal-only is deferred initially
with compact reference evidence. Its omitted source is not declared irrelevant
or nonrecipe. A reviewer or audit must request the existing full-context
expansion before applying a correction when that evidence is needed. Exact
indexed anchors still retain their matching destination and continuation group;
missing or legacy anchor evidence retains the existing conservative fallback.
Adjacent chunks remain across ambiguous source boundaries, while neighboring
complete recipes are excluded only when the source-boundary predicate separates
them. Source hints and semantic role rules are unchanged.

Hybrid owner placement is derived inside the actual assembly pass and records
either the retained final recipe and section offset or a dropped reason. Source
correction coordinates remain chunk-local; placement describes the assembled
result and does not claim assignment metadata survived assembly.


Gemini verifies other models; GLM 5.3 verifies Gemini. Invalid references,
malformed responses, ambiguous classifications, and source findings prevent
acceptance. AI agreement is not proof of accuracy.

Cheaper verifier trials are opt-in evaluation work. They do not change the
production verifier policy without the frozen defect and false-alarm admission
checks. Each candidate retains separate target/stage evidence; a pending or
interrupted verifier request is never acceptance evidence.

AI correction is bounded to one correction pass followed by a final re-audit of
the full corrected group. The final re-audit returns source-grounded findings
with empty operation-specific correction lists because no second correction
pass remains. Historical extra patches stay readable as evidence and cannot
create acceptance. Corrections continue to invalidate prior acceptance, and
the indexed default and existing spending/retry limits are unchanged.

Complete means **all automated checks passed**. It does not mean guaranteed
accuracy. Incomplete includes a concrete stop reason and preserves partial
results. Historical runs are Not assessed. There is no manual review queue or
approval step; existing review sidecars remain readable through compatibility
interfaces.

### Frozen automatic c4 result

The v4 source-bound Luna c4 evaluation completed in 77.6 seconds with
0/8 groups accepted. It matched 27/27 expected recipe owners and 305/305
ingredients, but preserved 126/134 required methods and produced four phantom
recipes from publisher-caption titles. The verifier also recorded false alarms
alongside supported findings. This is evaluation evidence for the current
quality and acceptance gates, not a claim of full-book completion or a reason to
change the default policy.

## Runtime and accounting contract

CLI preflight and desktop previews share `CostEstimate`: an optional USD tuple
`[low, high]`, its sample basis, and calibration status. Extraction and
verification each carry this one value; TypeScript is generated from the same
Rust definition. Unknown ranges have neither endpoint. Existing total low/high
fields remain compatibility projections.

`recipe_epub::recovery::State`, `Action`, `Reply`, `Adapter` and `run` are
available without native features. The same driver runs native and browser-local,
non-Send callbacks. Adapters supply transport, exact-key cache storage, durable
checkpointing, cancellation, time and waiting. They do not choose recovery order,
retry eligibility, budget allocation or acceptance.

The native adapter uses the existing Gateway transports and stored credentials.
No new Cubby protocol is selected. Both the existing browser callback and new
recovery callback examples compile for WASM.

The default $10 ceiling is in Advanced: 80% extraction/recovery, 20% verification.
Requests reserve before dispatch; each exact request permits at most two
attempts, including interrupted requests. Transient retries respect provider
delays up to 60 seconds. Other failures advance the model stage. Unknown prices
prevent dispatch; validated cache reuse does not spend money.

Concurrency supports one through eight requests and defaults to four. Provider
backoff does not occupy an active request slot. Increasing the default requires
matched cold-cache trials meeting the speed, quality, failure, and cost gates.
Operation output limits are bounded by the catalog and included in request
identity and reservations. Truncated responses cannot pass verification.

Checkpoints preserve candidate outputs, source references, AI findings,
verification responses, request keys, model provenance, rate snapshots, provider
usage, failures and reservations. Resume never clears prior spending. Explicit
limit changes adjust the ceiling while retaining charges. Children preserve
parent identity and inherited accounting.

Request telemetry records byte counts, identifiers, operation/model, timings,
and reported reasoning usage without copying source prose or credentials into
telemetry. Reasoning tokens remain part of reported output usage, not an added
charge. Unavailable timing remains unknown.

The recovery cache is separate from legacy indexed/legacy-string caches, keyed
by exact request, model, provider, transport, output limit and policy version.
Atomic native cache writes are validated again on reads. Bump the policy version
when changing acceptance semantics; incompatible checkpoints require a new run.

## Inspecting results

- Desktop: Extraction feedback shows verification results, resolved and unresolved
  findings, source links, and extraction/verification estimates.
- CLI: `cookbook feedback RUN` shows the same stored evidence; append
  `--format json` for full structured state.
- `cookbook extract BOOK --dry-run --allow-network` previews initial extraction
  and verification; subsequent recovery cost is conditional.
- `cookbook extract BOOK --resume --out RUN --allow-network` resumes Automatic.
  Supply the original `--model` when resuming a manual extraction.

Fixtures and mocked transports validate the policy and native cache adapter.
They are not new evidence about live provider reliability or universal cookbook
accuracy. Cubby integration, persistence adapters and deployment remain deferred.

## Offline source-role experiment

`tools/plan_source_roles.py` converts the experimental source-layout planner's
residual output into bounded requests. It does not call providers or change
Automatic extraction. Keep its inputs and outputs in private evaluation storage;
requests contain cookbook prose.

```sh
python3 tools/plan_source_roles.py plan --residual PRIVATE_RESIDUAL.json --out PRIVATE_PLAN.json
python3 tools/plan_source_roles.py import --plan PRIVATE_PLAN.json --pack-id p0 --response PRIVATE_RESPONSE.json --out PRIVATE_ASSIGNMENTS.json
```

The default limits are 96 KiB per provider-neutral payload and 64 unknown lines.
Each request preserves whole owning recipe regions; oversized regions are
explicitly deferred. Send the exported `provider_payload`, including its request
hash. Transport-specific wrapping and token limits still require measurement.
Import requires every requested ID exactly once, a matching request hash, and a
valid role or the exact string `unresolved`, which imports as null. This keeps
the provider role field a string enum while preserving unresolved evidence.
Target paragraphs are partitioned into exact source fragments of at most 240
Unicode characters. Responses select an `evidence_id` belonging to that target;
the importer resolves it to original text and records its coordinate provenance.
Fragments concatenate to the original paragraph without loss and do not split
its role or recipe ownership. Selecting valid evidence does not prove the role
is semantically correct.
The request also binds the source-coordinate map. Imported
schema-1 assignments remain unverified; null retains unresolved source evidence.

This experiment still requires a source-layout profile and independent recipe
verification. Fixture round trips establish mapping compatibility, not model
quality or cold full-book completion time.

The native `paid_source_roles_evaluation` example prepares one catalog-priced
request by default; `--dispatch` explicitly sends it once through the existing
backend. It checks compatibility with the current importer before dispatch,
reserves in the independent throughput ledger, persists returned usage before
import, and retains unknown charges. It has no retries, cache, or acceptance path.
Its provider timing measures source-role classification only, not recipe
verification or full-book completion.

`tools/adjudicate_source_roles.py build --plan PRIVATE_PLAN.json --assignments
PRIVATE_ASSIGNMENTS.json --out PRIVATE_REVIEW_PLAN.json` prepares a separate
experimental review of every proposed method target. It retains whole owner
contexts and binds the first-stage assignments. Use the same native harness with
`--planner-tool tools/adjudicate_source_roles.py`; preparation remains offline.
Its importer merges only reviewed coordinates and preserves unresolved results.
This review can test method overreach, but cannot discover methods that the first
stage labeled as background. Evaluate the combined output against all frozen
targets, including required-method recall; neither stage accepts recipes.
The initial Luna review trial failed this gate: it retained the narrative
overreach and demoted two required preparation constraints. This stage is an
evaluation tool, not an admitted correction path for Automatic extraction.

The frozen source-role holdout also rejected the one-pass Luna classifier:
29 of 32 roles matched, but only two of four required method paragraphs were
identified. Exact source preservation and fast classification did not establish
semantic fidelity. Preserve this holdout result as evaluation evidence; later
tuning requires fresh independent evaluation before making quality claims.

The source-layout example can also export an unverified recovery checkpoint with
`--recovery-state-out PRIVATE_STATE.json --recovery-model MODEL`. It maps region
selections back to original chunk coordinates and seeds only complete recovery
groups. Unassessed source remains scheduled for ordinary extraction. This is
an offline evaluation entry point, not a new desktop extraction default.
The model selects the recovery/verifier policy; artifact hashes retain the
proposal's provenance without claiming a provider created it.

The portable `State::install_unverified_seed` boundary checks source identity,
group coverage, indexed content, and provenance before changing an untouched
group. It creates no paid attempt, extraction-cache hit, or acceptance evidence.
Seeded candidates still require the established verifier; rejection starts the
ordinary extraction model sequence. The checkpoint's budget does not authorize
an evaluation call or replace the independent paid-evaluation ledger.

The `paid_throughput_evaluation` example prepares automatic, unseeded cohorts
offline by default. Supply a source inspection run, ordered `--chunk` selections,
and `--prepare-out` to record fresh EPUB/document identities and exact immediate
actions without credentials or ledger changes. Only explicit `--dispatch` enables
provider calls, with a separate ledger, new checkpoint, and empty cold-cache
directory. Every action reserves before dispatch; an exhausted ledger leaves an
incomplete result. The report separates source preparation from scheduler time.

`tools/report_automatic_cohort_envelope.py` compares this manifest with the
throughput ledger. Immediate wave reservations are exact; later verification
requests depend on generated candidates and are not a full-book cost bound.
Neither a funded first wave nor a manually prepared seed demonstrates automatic
full-book completion time.

The v5 development trial with exact DOM evidence removed the four caption
pseudo-recipes, but did not establish quality: it preserved all 27 expected
recipe owners and 305 ingredients while retaining only 127 of 134 frozen method
passages in instructions and omitting both critical notes. Four of eight groups
were accepted in 70.7 seconds, including groups with those omissions. These are
verifier false negatives, not evidence of verified-quality completion. The
contracts differ between this trial and the earlier v4 trial, so they do not
advance the matched concurrency gate. The under-one-minute full-book goal remains
unproven.

The v6 shared-rule development trial took 83.6 seconds and accepted 2/8 groups.
It made eight extraction calls and seven verifier calls: one extraction failed
strict source-line coverage and could not proceed to verification. An accepted
recipe group still placed two background headnotes in instructions rather than
their frozen description/notes fields. Their text was preserved verbatim. Usable proposals covered only
22/27 owners, 244/305 ingredients, and 114/134 required methods. One of two
critical notes was preserved. These aggregate counts include the missing
extraction group and must not be interpreted as purely semantic regressions.
Verifier latency dominated, and five of its seventeen findings were false alarms
against frozen labels (two more lacked an auditable frozen coordinate). Neither
quality nor sub-minute completion is established. Default concurrency remains four.

On the six recipe-bearing groups with usable outputs in both v5 and v6,
ordinary method preservation improved from 110/112 to 112/112 and required
headnote methods from 0/4 to 2/4. Non-procedural headnote placement in the allowed metadata fields fell from
47/49 to 40/49; critical notes improved from 0/2 to 1/2. Thus the shared rules
changed the error distribution without establishing complete source fidelity.

A subsequent raw-payload trace distinguished wrong-field placement from text
loss: all nine retained v6 non-procedural headnote field failures were selected
exactly once as instructions and lowered verbatim. No omitted-text or lowerer
bug was found in those rows. Deterministic source coverage cannot establish
whether such advice is a required procedure; that semantic distinction remains
an extraction and verification failure.

The v7 context-trimming trial took 77.8 seconds at concurrency four, cost
$0.05876230, and accepted 6/8 groups. All proposals preserved 27/27 owners and
305/305 ingredients, with 132/134 methods and both critical methods/notes in
allowed fields. Seven wrong-field placements remained, three in accepted
groups; none of those placement failures lost the source text. The verifier
reported two supported findings and seven false alarms. On matched targets,
median verifier context bytes fell from 69,583 to 23,378 and median provider
time from 35.183 to 27.071 seconds. This single changed-contract experiment is
not a concurrency promotion result or full-book completion proof.

Review after that trial found a pre-existing context-link bug: raw relative
EPUB hrefs were compared directly with archive paths, while fragment-only links
were dropped. The v7 timings precede the correction and do not validate complete
linked-source context. Frozen trial evidence remains unchanged.

For saved-candidate verification planning, `benchmark_recovery_plan STATE.json`
(the `recipe-epub` example) measures 100 read-only plans after warm-up and checks
stable action keys. Its report includes request counts and context sizes, not
source prose. This deliberately measures saved-state cloning, migration, and
request planning; it does not replace the separate native inspection/preview
benchmark or include UI debounce.

V8's offline saved-candidate benchmark over the eight-chunk development state
measured p50 449.050 ms and p95 535.411 ms, with stable action keys and unchanged
input state. Six targets required six context chunks after resolving relative
links and conservatively handling unresolved anchors. Thus the earlier v7
request-size savings do not carry forward unchanged, and the saved-candidate
measurement is not evidence that native preview meets its separate p95 target.
Further paid sweeps are paused while source-link boundaries, duplicate verifier
input, independent verifier accuracy, and the full-book call budget are reassessed.

The throughput harness now retains every inspected source chunk as verifier
context and enables only the selected complete recovery groups. A selection
that cuts through a group is rejected before dispatch. Preparation manifest v2
reports selected execution chunks separately from `context_chunk_count`, and
binds both source hashes. Checkpoint source indices remain original book
indices; the ledger mapping records the selected indices separately. Earlier
cohort artifacts used truncated source context and remain historical evidence.

Non-text fragment anchors retain their original element coordinates separately from
text contributors. Anchors between emitted lines retain both adjacent locations;
verifier link resolution includes every matching chunk. Inspection v3 refreshes
this provenance, and verification v9 requires fresh acceptance evidence while
preserving historical candidates and spending.
