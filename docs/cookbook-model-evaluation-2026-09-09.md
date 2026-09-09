# Cookbook model evaluation — 2026-09-09

Keep Gemini 2.5 Flash as the default. GPT-5.6 Luna is a promising inexpensive
alternative on ordinary recipes; Gemini 3.7 Flash recovered more recipes from
the difficult continuation group. Neither consistently met all frozen source
expectations. The evidence is too small and inconsistent to select a replacement.
Kimi K2.7 Code is available as an experimental choice. K2.6 remains disabled in
the dropdown after recipe-request timeouts, although authentication succeeded.

## Method and budget

One ledger capped the entire evaluation at **$20**, including candidates,
retries, failures and unresolved requests. Its final charged-or-reserved total
is **$3.05043668**. Saved per-request token estimates sum to **$0.32429168**.
The difference is conservative reservation, not asserted billing. Some early
repeat calls may have used Gateway response caching; their estimates remain in
the cap ledger, and their latency is excluded from conclusions. Later trials
explicitly set `cf-aig-skip-cache: true` and `cf-aig-max-attempts: 1`.
No billing-statement access or credit-purchase fees are included.

Source labels were frozen before the corresponding outputs. Development-book
selection used seed 20260909 and excluded the held-out parser books. The small
set covers non-recipe dedication prose, two ordinary Thai recipes, four Nopalito
popsicles, and a complete contiguous three-chunk group from Charred with eleven
expected recipes and continuation boundaries. Source expectations check matching
titles, exact ordered ingredient lines, and preservation of method paragraphs
in methods or notes. These checks do not establish full-book quality or semantic
ingredient-parser accuracy. Whole-run exit 3 is expected when extracting selected
chunks; quality conclusions inspect the selected outputs and errors instead.

Frozen manifest SHA-256 values:

- Ordinary/non-recipe samples: `8e5466f4f3611428c2ccf095890852e8a68adc78bda2527aac78a9c46b834322`
- Continuation group: `6b7a015e27bff0c2fcb3139e13e65a6b465a5a7d9b652c2a315cfd48fc0f87b2`

Books, authored source labels, full run files, scoring output, and the budget
ledger remain outside the public repository under the per-user
`ingredient-parser/cookbook-evaluation/2026-09-09` data directory.

## Results

Ordinary-sample costs below include retained reservations on failures. Times are
elapsed across the two sampled runs, not per-request latency. Multiple continuation
scores show independent trials with Gateway caching bypassed.

| Exact model ID | Ordinary titles | Ordinary cost/reservation | Elapsed | Continuation scores |
|---|---:|---:|---:|---|
| `gemini-2.5-flash` | 6/6 | $0.00385 | 43.0s | 6/11 titles, 6/11 ingredients, 6/11 methods |
| `gemini-3.5-flash-lite` | 6/6 | $0.00482 | 6.9s | 6/11 titles, 6/11 ingredients, 6/11 methods |
| `gemini-3.7-flash` | 6/6 | $0.00534 | 15.8s | 10/11 titles, 8/11 ingredients, 9/11 methods; 10/11 titles, 8/11 ingredients, 9/11 methods |
| `claude-haiku-4-5` | 0/6 | $0.36726 | 14.4s | Not advanced after ordinary-sample failures |
| `claude-sonnet-5` | 0/6 | $0.73610 | 17.5s | Not advanced after ordinary-sample failures |
| `gpt-5.6-luna` | 6/6 | $0.00306 | 21.0s | 6/11 titles, 6/11 ingredients, 6/11 methods; 10/11 titles, 9/11 ingredients, 9/11 methods |
| `@cf/moonshotai/kimi-k2.6` | 0/6 | $0.30086 | 360.6s | Not advanced after ordinary-sample failures |
| `@cf/moonshotai/kimi-k2.7-code` | 6/6 | $0.03213 | 122.0s | 6/11 titles, 6/11 ingredients, 6/11 methods |

All six ordinary recipes from the baseline, Gemini alternatives, Luna and Kimi
K2.7 preserved their labeled ingredients and method paragraphs. Haiku 4.5 and
Sonnet 5 failed indexed-source validation through duplicate line ownership;
those failures do not imply that the models are generally unsuitable for other
prompts. Kimi K2.6 authenticated and completed the non-recipe call, but recipe
calls timed out at 180 seconds. K2.7 passed the ordinary sample and timed out on
part of the continuation group. Early direct-Workers attempts were rejected
locally for a token requirement; that requirement was corrected to use the
existing Gateway token and those attempts remain conservatively reserved.

The first cost-leading pair (Flash-Lite and Luna) was repeated. After adding the
continuation evidence, Gemini 3.7 and Luna were repeated again. Luna's final
continuation run emitted eleven recipe objects but only ten matched the frozen
titles, with nine exact ingredient lists and nine preserved method sets. More
objects alone are not higher fidelity. Gemini 3.7 repeated ten matching titles,
eight exact ingredient lists and nine method sets. No candidate consistently
matches all seventeen expected recipes, so the baseline remains unchanged.

## Verified transports and pricing sources

- Gemini uses Google AI Studio's Chat Completions compatibility endpoint.
  [Google pricing](https://ai.google.dev/gemini-api/docs/pricing): 2.5 Flash
  $0.30/$2.50; 3.5 Flash-Lite $0.30/$2.50; 3.7 Flash $0.75/$3.75 per million
  input/output tokens at the checked date. The 3.7 rate is introductory.
- Claude uses Messages with forced tools.
  [Anthropic pricing](https://platform.claude.com/docs/en/about-claude/pricing):
  Haiku 4.5 $1/$5 and Sonnet 5 $2/$10.
- Luna uses Responses with a forced function and `store: false`.
  [OpenAI model documentation](https://developers.openai.com/api/docs/models/gpt-5.6-luna):
  $0.20/$1.20, with cached input priced separately. Output usage includes
  reasoning tokens; the implementation does not add them a second time.
- Kimi uses `workers-ai/@cf/moonshotai/...` through the Gateway unified
  Chat Completions endpoint, authenticated with the existing Gateway token.
  [Gateway authentication examples](https://developers.cloudflare.com/ai-gateway/usage/chat-completion/),
  [Workers AI billing configuration](https://developers.cloudflare.com/ai-gateway/usage/providers/workersai/),
  [K2.6](https://developers.cloudflare.com/workers-ai/models/kimi-k2.6/) and
  [K2.7 Code](https://developers.cloudflare.com/workers-ai/models/kimi-k2.7-code/):
  $0.95/$4, cached input $0.16/$0.19 respectively. The Gateway ID is explicit.
  The token could perform inference but could not read Gateway management
  settings, so the account's billing-mode setting was not independently inspected.

All figures are dated estimates. Existing stored-provider-key configuration is
preserved. No additional API credential, fallback model, or billing setting was
created. The model catalog includes capability status so transport availability
is not mistaken for extraction quality.

## Workflow validation follow-up

The progress/cancellation/history changes were checked against the same frozen
Simple Thai Food, Nopalito, and Charred samples. Missing current-contract cache
entries were fetched with Gemini 2.5 Flash; two Charred chunk outputs were reused.
No held-out books were added and expectations were not changed after generation.

- Simple Thai Food returned both expected recipes with exact ingredients and
  methods. Both titles also included source-language subtitles, so strict frozen
  title equality was 0/2 despite identifiable source-grounded recipes.
- Nopalito returned all four expected titles and ingredient lists. Some expected
  method text was assigned to notes: the text was preserved, but strict method
  placement did not match the frozen labels.
- Charred retained six of eleven expected recipes with matching ingredients and
  methods. The remaining request failed indexed-source validation after producing
  a titleless continuation. This remains an extraction-quality limitation.

New token estimates were $0.0024269, $0.0024892, and $0.0049149 respectively,
within the relevant request previews (excluding reused chunks). New estimates
sum to $0.009831; the shared evaluation ledger now totals $3.14742718 charged or
reserved, including retained failure reservations. These are not billed totals.
Detailed results remain under the existing per-user evaluation directory in
`workflow-validation/results.json`.

A CLI smoke test against a local fake transport sent Ctrl-C with four active
chunks out of six selected. It returned 130, saved all four terminal outcomes,
left no pending requests, and admitted neither remaining chunk. Shared tests
also cover resuming only unfinished work, budget release, and persisted
cancelled/interrupted/failed states. Desktop WebKit tests cover progress, stopping,
resume controls, and same-book comparison selection.

## Continuation repair and final recheck

Source inspection located the Charred failure before model execution: the text
heuristic treated “Serves 4” and unquantified seasoning as title boundaries while
ignoring a long recipe title. The new boundary regression reproduces that failure
with synthetic source markup. Title markup now drives boundaries, and hard-split
regressions cover title hints, ingredient/method tails, and an introduction split
before its first ingredient. The indexed contract restricts continuation hints
to the first source-ordered recipe.

The same frozen expectations were reused with source chunks remapped before
viewing new model output. Chunk boundaries changed, so the Thai selection also
contains one neighboring recipe; it is outside the original two-recipe score.
The non-recipe sample was retained. The final manifest SHA-256 is
`d96c3ddad2322d14d417738fd39c19f404e2e330e468cabc0ee95ab548449f59`.

| Frozen sample | Matched source recipes | Exact ingredient lists | Exact methods in instructions |
|---|---:|---:|---:|
| Simple Thai Food | 2/2 | 2/2 | 2/2 |
| Nopalito | 4/4 | 4/4 | 4/4 |
| Charred | 11/11 | 11/11 | 11/11 |

Strict title equality remains 0/2 for the Thai recipes because their complete
source titles include translations absent from the frozen title strings. The
table associates those two by the labeled title plus its source-authored subtitle;
it does not change the gold strings or count them as exact title matches. The
other fifteen titles match exactly.

The first revised-prompt trial recovered all eleven Charred recipes but placed
corn husks from an equipment list in ingredients. The final v7 prompt clarified
non-food wrappers and passed the frozen ingredient expectations. Nopalito's
freezing/unmolding paragraphs now remain in instructions. This is evidence on a
small development sample, not whole-library accuracy or a new model ranking.

Both trial rounds cost $0.0251428 in token-based estimates. The single evaluation
ledger totals $3.17256998 charged or reserved, including earlier retained failure
reservations, below the $20 cap. Raw runs, frozen selections and scores remain
outside the repository under `cookbook-evaluation/2026-09-09/continuation-fix/`;
`results-v7.json` records the final scores.
