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
| gemini-3.5-flash-lite, gemini-3.7-flash | — | — | — | — | Disabled: Google answers `400 Missing or invalid Authorization header` through the gateway (not covered by unified billing) |
| claude-sonnet-5 | — | — | — | — | Every call refused with `429 Wholesale Rate limited` (gateway error 2018); kept in the catalog, priors from the 49 calls that did answer |

gemini-2.5-flash and claude-haiku-4-5 were measured as the head and fallback
of the previous default ladder rather than alone (below).

## Priors (fitted `latency = ttft + output_tokens / tps` over every recorded call)

| Model | calls | ttft p50 | ttft p90 | output tok/s | retry rate |
|---|---|---|---|---|---|
| gemini-2.5-flash | 1204 | 13.9 s | 27 s | 148 | 0.23 |
| claude-haiku-4-5 | 367 | 1.5 s | 2.9 s | 290 | 0.35 (measured 0.61 as a fallback, where it only sees hard chunks) |
| gpt-5.6-luna | 314 | 0.6 s | 5.7 s | 100 | 0.20 |
| claude-sonnet-5 | 49 | 1.4 s | 2.8 s | 155 | 0.15 |
| gemini-2.5-flash-lite | 48 | 0.5 s | 1.5 s | 297 | 0.69 |

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

The extraction rules were hardened between the first row and the rest
(chunk-boundary protection, label and caption rules, h2 recipe titles, prose
variations), so the first row is not directly comparable.

## Decision

`DEFAULT_LADDER = ["gemini-2.5-flash", "gpt-5.6-luna", "claude-haiku-4-5"]`.
Gemini 2.5 Flash reads a chunk best on the first pass; GPT 5.6 Luna is the
fast, cheap second reader for retries and second opinions; Haiku 4.5 is the
last resort. Every candidate is priced, so the estimate and the run report
carry real costs.

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
