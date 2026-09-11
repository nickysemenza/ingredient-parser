# Cookbook model ladder, 2026-09-11

How `cookbook::models::DEFAULT_LADDER` and the catalog priors were chosen.
Method: `food-cli cookbook eval` over the six answer-key books (Dessert
Person, Nothing Fancy, Zuni Cafe, Bouchon Bakery, Pok Pok, Flour Water Salt
Yeast), live through Cloudflare AI Gateway with the chunk cache off.

## Single models (`--ladder <m> --no-second-opinion --no-escalation`)

| Model | Mean recall | Phantoms | Cost (6 books) | Max wall | Notes |
|---|---|---|---|---|---|
| gpt-5.6-luna | 95.0% | 2 | $0.65 | 53 s | Fastest and cheapest; a few chunks fail every attempt without a fallback |
| gemini-2.5-flash-lite | 52.8% (Nothing Fancy only) | 5 | — | — | Disabled: misses half the recipes |
| gemini-3.5-flash-lite, gemini-3.7-flash | — | — | — | — | Disabled: Google answers `400 Missing or invalid Authorization header` through the gateway (not covered by unified billing); unchanged after the credit top-up |
| claude-sonnet-5 | 99.1% (Nothing Fancy only) | 0 | $0.60 (one book) | 19 s | The most accurate and the fastest single reader, at four times Gemini's price. Its first probes were refused with `429 Wholesale Rate limited` (gateway error 2018): the account's gateway credit was empty, and once topped up the same code came back for a minute after a burst of 16 calls (the wholesale pool meters tokens per minute); the run now backs off 5/10/20/40 s on it instead of writing the model off. It also hands the tool input back as a JSON string under `items` about half the time, which the decoder now unwraps |

gemini-2.5-flash and claude-haiku-4-5 were measured as the head and fallback
of the previous default ladder rather than alone (below).

### Workers AI (Nothing Fancy only, 29 chunks, 180 s transport timeout)

| Model | Recall | Phantoms | Samples | Cost | Wall | Failures |
|---|---|---|---|---|---|---|
| @cf/zai-org/glm-5.3-flash | 98.1% | 0 | 88% | $0.07 | 277 s | 2 timeouts, 2 invalid |
| @cf/zai-org/glm-5.3 | 99.1% | 0 | 100% | $0.82 | 333 s | 3 timeouts, 2 invalid |
| @cf/moonshotai/kimi-k2.7-code | 95.4% | 0 | 100% | $0.52 | 272 s | 2 timeouts, 1 `402`, 1 invalid |
| @cf/deepseek-ai/deepseek-v4-flash-0731 | 100% | 0 | 100% | $0.22 | 278 s | 1 timeout, 1 undecodable answer, 1 leak (the Baked Potato Bar essay) |
| @cf/google/gemma-4-26b-a4b-it | 46.3% | 0 | 38% | $0.11 | 512 s | 6 timeouts, 28 of 49 answers invalid (no tool call, lines doubled) |
| @cf/zai-org/glm-4.7-flash | 0% | 0 | 0% | $0.08 | 464 s | 12 timeouts, 44 of 58 answers leave lines unassigned; one usable answer |

The `402 Insufficient wholesale credits` (gateway error 2021) that DeepSeek,
Gemma and GLM 4.7 Flash returned at first was the account's gateway credit
running out, not a billing exclusion; the rows above are from after the
top-up. The GLM and DeepSeek models read as accurately as the chosen ladder
(and produce no phantoms on this book), but they think before they answer:
50–60 s per chunk at the median, 57–75 visible output tokens/s, and a few
chunks per book outrun the 180 s timeout, so a book takes 4–6 minutes. Their
catalog priors fold the thinking time into the first-token latency because
the usage they report counts reasoning tokens as output. They stay disabled;
`glm-5.3-flash` and `deepseek-v4-flash` are the ones to revisit if Workers AI
gets faster, since they are the cheapest accurate readers measured.

## Priors (fitted `latency = ttft + output_tokens / tps` over every recorded call)

| Model | calls | ttft p50 | ttft p90 | output tok/s | retry rate |
|---|---|---|---|---|---|
| gemini-2.5-flash | 1204 | 13.9 s | 27 s | 148 | 0.23 |
| claude-haiku-4-5 | 367 | 1.5 s | 2.9 s | 290 | 0.35 (measured 0.61 as a fallback, where it only sees hard chunks) |
| gpt-5.6-luna | 314 | 0.6 s | 5.7 s | 100 | 0.20 |
| claude-sonnet-5 | 70 | 2.9 s | 4.1 s | 193 | 0.15 |
| gemini-2.5-flash-lite | 48 | 0.5 s | 1.5 s | 297 | 0.69 |
| @cf/zai-org/glm-5.3 | 28 | 6.5 s | 14.5 s | 75 | 0.15 |
| @cf/zai-org/glm-5.3-flash | 28 | 5.5 s | 10.1 s | 58 | 0.13 |
| @cf/moonshotai/kimi-k2.7-code | 27 | 11.0 s | 31.0 s | 57 | 0.13 |
| @cf/deepseek-ai/deepseek-v4-flash-0731 | 29 | 47 s (thinking folded in) | 88 s | 71 | 0.06 |
| @cf/google/gemma-4-26b-a4b-it | 15 | 115 s (thinking folded in) | 162 s | 74 | 0.69 |

Gemini 2.5 Flash spends most of its latency before the first token (it
thinks), so a book takes 100–170 s with it at the head even at concurrency
16; GPT 5.6 Luna answers in 8 s at the median.

## Full-policy ladders (second opinion and whole-book escalation on)

| Ladder | Mean recall | Phantoms | Leaks | Cost | Max wall | Gate |
|---|---|---|---|---|---|---|
| gemini-2.5-flash → claude-haiku-4-5 → claude-sonnet-5 (previous default) | 98.3% | 10 | 2 | $1.78 | 173 s | fail (phantoms, leaks, 2 books with a failed chunk) |
| gpt-5.6-luna → gemini-2.5-flash → claude-haiku-4-5 | 99.1% | 11 | 2 | $0.59 | 275 s | fail (phantoms, leaks, failed chunks in Bouchon and FWSY) |
| gpt-5.6-luna → claude-haiku-4-5 → gemini-2.5-flash | 98.3% | 10 | 2 | $1.48 | 221 s | fail (two books escalated) |
| **gemini-2.5-flash → gpt-5.6-luna → claude-haiku-4-5** (chosen) | **98.4%** | **8** | **1** | **$1.02** | **183 s** | fail (8 phantoms > 6; the Nothing Fancy leak; one failed chunk in Pok Pok) |
| gemini-2.5-flash → claude-sonnet-5 → gpt-5.6-luna | 97.9% | 12 | 4 | $2.33 | 258 s | fail; a second reader only sees flagged chunks, so Gemini's first pass still decides the result, and Sonnet's price shows on every retry |
| claude-sonnet-5 → gemini-2.5-flash → gpt-5.6-luna | 97.3% | 6 | 1 | $7.92 | 230 s | fail; Sonnet reads Bouchon's tables badly (26 of 60 answers invalid), four books end with the ladder exhausted, and the wholesale pool's per-minute metering stretches a book to 2–4 min at concurrency 16 |

The extraction rules were hardened between the first row and the rest
(chunk-boundary protection, label and caption rules, h2 recipe titles, prose
variations), so the first row is not directly comparable.

## Decision

`DEFAULT_LADDER = ["gemini-2.5-flash", "gpt-5.6-luna", "claude-haiku-4-5"]`.
Re-checked after the gateway credit top-up made Claude Sonnet 5 available:
it is the best single reader on Nothing Fancy but earns no slot, at the
head (eight times the cost, rate-limited in bursts, worse on tables) or as
the second reader (no gain in phantoms or leaks for twice the cost).
Gemini 2.5 Flash reads a chunk best on the first pass; GPT 5.6 Luna is the
fast, cheap second reader for retries and second opinions; Haiku 4.5 is the
last resort. Every candidate is priced, so the estimate and the run report
carry real costs.

## Addendum: Gemini's thinking (2026-09-11, later)

The 14 s first-token latency was Gemini 2.5 Flash's default dynamic
thinking, not the gateway: one real chunk request took 11.8 s at the median
on the compat route, 3.5 s with `reasoning_effort: "none"`, and the native
route reported ~2,300 thought tokens per call that the compat usage never
showed (so the crate's Gemini cost had been undercounting). Six-book eval
by setting, full policy, `--no-cache`:

| Gemini reasoning | Mean recall | Phantoms | Leaks | Cost | Max wall | Gemini p50 latency |
|---|---|---|---|---|---|---|
| default (dynamic) | 98.4% | 8 | 1 | $1.02 | 183 s | 15.4 s |
| low (1,024-token budget) | **98.4%** | **7** | 1 | $1.14 | **122 s** | **7.4 s** |
| none (also applied to Luna) | 94.6% | 12 | 4 | $3.07 | 67 s | 3.5 s |

`gemini-2.5-flash` now carries `reasoning: Low` in the catalog; the ladder is
unchanged. The native `generateContent` route was tried and produced
malformed function calls two rounds in three, so the knob rides the compat
route's `reasoning_effort`.

## What still misses the gate, and why

The gate (`mean recall ≥ 97%`, `≤ 1 phantom per book`, no leaks, full
coverage) is met on recall and nearly on phantoms; the remaining misses are
judgment calls between the answer keys and the extraction, not lost text:

- **Formula and example tables** (Flour Water Salt Yeast): the keys treat the
  baker's-formula examples in the "make your own dough" chapter as prose;
  the extractor keeps ones that carry a method. Formula tables with no yield
  and at most two explanatory paragraphs are already demoted to prose.
- **Essay heading over a recipe** (Nothing Fancy's "My Favorite Bar Is a
  Baked Potato Bar" over "Baked Potato Bar"): the recipe is found, but the
  model sometimes titles it by the essay line; the assembly retitles it when
  the real title is the last title-like line before the ingredients.
- **Zuni's typographic titles**: wine pairings and "TO CORKSCREW A LEG of
  LAMB" sub-headings occasionally come out as items; the keys list some
  sub-recipes ("THE BASIC BRINE") that the model folds into their parent.
- **Speed**: Gemini 2.5 Flash spends 14 s before its first token, so a
  150-recipe book takes 100–180 s at concurrency 16 rather than the 90 s
  target. A GPT-headed ladder is twice as fast when nothing escalates but
  escalates more often.

Re-run with `cargo run -p food-cli -- cookbook eval --format json`; the
answer keys live outside the repo (see docs/cookbook.md).
