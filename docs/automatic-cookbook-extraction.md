# Automatic cookbook extraction

Desktop Extract and `cookbook extract BOOK --allow-network` use one shared
source-verification policy. Explicit CLI `--model ID` or the desktop Advanced
model selector disables model escalation, but retains source verification.

The initial order is GLM 5.3 Flash, Gemini 2.5 Flash, GLM 5.3, then Kimi K2.7 Code.
OpenAI and Anthropic remain explicit experimental choices. No new paid evaluation
or automatic admission of catalog entries is performed.

## Acceptance

The source inventory is frozen before outputs. A spine document is the
conservative recovery group; adjacent documents with a matching continuation hint
are joined. Selected chunks expand to their group. Unselected, unverified groups
keep the book incomplete.

Indexed output must account for every line without duplicate ownership. The
verifier receives the assembled candidate and source context and classifies each
target chunk's lines. Verification is partitioned by chunk to bound output size,
but a group is accepted only after all its chunks pass. Source-styled ingredient
omissions and missing methods fail deterministic checks even after AI approval.
Proposed replacements are reassembled with previously accepted groups.

Gemini verifies other models; GLM 5.3 verifies Gemini. Invalid references,
malformed responses, ambiguous classifications, and source findings prevent
acceptance. AI agreement is not proof of accuracy.

Complete means **all automated checks passed**. It does not mean guaranteed
accuracy. Incomplete includes a concrete stop reason and preserves partial
results. Historical runs are Not assessed. There is no manual review queue or
approval step; existing review sidecars remain readable through compatibility
interfaces.

## Runtime and accounting contract

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

Checkpoints preserve candidate outputs, source references, AI findings,
verification responses, request keys, model provenance, rate snapshots, provider
usage, failures and reservations. Resume never clears prior spending. Explicit
limit changes adjust the ceiling while retaining charges. Children preserve
parent identity and inherited accounting.

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
