# Cookbook library sweep, 2026-09-11

The second measurement pass over the `cookbook` crate: why Gemini was slow,
a sample of the library extracted and ranked, and what the ranking says to
fix. Method and commands are in [cookbook.md](cookbook.md) under "Library
sweep and tuning loop".

## Why Gemini took 14 s per chunk

Over the 1,759 Gemini calls saved before this pass, latency had a 15 s
median and a 7 s floor that did not move with chunk size (R² 0.05 against
input and output tokens), while Haiku and GPT 5.6 Luna through the same
gateway answered in 3–7 s. One real chunk request (Nothing Fancy k003) sent
by hand, three times each, with the gateway cache skipped:

| Probe | Route | Body | Median | Notes |
|---|---|---|---|---|
| P1 | `/compat/chat/completions` | as the crate sends it | 11.8 s | 545 output tokens, no thought tokens reported |
| P2 | same | `reasoning_effort: "none"` | 3.5 s | same output; the gateway forwards the field |
| P3 | native `generateContent` | default | 12.6 s (one good round) | `thoughtsTokenCount` 2,324; two rounds in three were `MALFORMED_FUNCTION_CALL` |
| P4 | native `generateContent` | `thinkingBudget: 0` | 7.6–16 s | one round malformed; unusable |
| P5 | `/anthropic/v1/messages`, Haiku | as the crate sends it | 3.8 s | control |

So the time was Gemini 2.5 Flash's default dynamic thinking, about 2,300
tokens per chunk that the compat route's usage never showed and that Google
bills as output: the crate's Gemini cost had been undercounting by roughly a
factor of three. The native route cannot carry this crate's function schema
reliably, so the knob rides the compat route's `reasoning_effort`.

## Reasoning setting, six-book eval

Full policy, `--no-cache`, `--reasoning` applied to every model in the run
except where noted:

| Gemini reasoning | Mean recall | Phantoms | Leaks | Samples | Cost | Max wall | Gemini p50 |
|---|---|---|---|---|---|---|---|
| default (dynamic thinking) | 98.4% | 8 | 1 | 77% | $1.02 | 183 s | 15.4 s |
| **low** (Luna at its default) | **98.4%** | **7** | 1 | 79% | $1.14 | **122 s** | **7.4 s** |
| none (Luna too) | 94.6% | 12 | 4 | 65% | $3.07 | 67 s | 3.5 s |

Without thinking, Gemini lost four points of recall on the table-heavy and
formula-heavy books and three of them escalated, which is where the extra
cost came from. Low keeps the recall at half the latency, so
`gemini-2.5-flash` carries `reasoning: Low` in the catalog and the ladder is
unchanged. Should Gemini be replaced? Not on this evidence: at low reasoning
it is the fastest accurate reader per dollar measured (Sonnet 5 is a shade
more accurate at eight times the price and rate-limited in bursts; Luna at
the head produced 11 phantoms).

## Classifying the library

The structural classifier counted quantity-like lines and two-line runs,
which chapter numbers, addresses, rosters and code listings satisfy: a Rust
book scored 313 "ingredient runs" and a business book 194 "quantity lines".
Runs of three or more quantity-like lines separate them:

| Book | Solid runs | Two-line runs | Contents recipes | Verdict |
|---|---|---|---|---|
| Dining In | 167 | 239 | 115 | cookbook |
| Zaitoun | 131 | 187 | 2 | cookbook |
| Burma Superstar | 91 | 116 | 84 | cookbook |
| Taste (Tucci) | 160 | 289 | 40 | cookbook, and it does hold recipes |
| Database Design for Mere Mortals | 21 | 73 | 23 | model decides |
| The Rust Programming Language | 14 | 313 | 51 | model decides |
| Outliers | 10 | 16 | 5 | model decides |
| The Little Paris Kitchen | 2 | 27 | 8 | model decides; a real cookbook whose ingredient lines the quantity heuristic does not recognise |
| The Omnivore's Dilemma | 0 | 13 | 0 | not a cookbook |

Forty or more solid runs is a cookbook, none with fewer than five contents
recipes is not, and the cheap cached model call decides the rest with
thinking off (its old 400-token budget was being eaten by Gemini's low
reasoning, so every ambiguous book had silently stayed ambiguous). Over the
191-book library that gives 138 cookbooks and 53 others, with the model
settling about forty in between; the previous rule had called a Rust book,
a database textbook and two Gladwells cookbooks and sent 29 books, mostly
novels and business books, to a model call that never resolved.

## The sample sweep

The sample: 25 of the 138 cookbooks, drawn with seed 1 across authors, two
books at a time at chunk concurrency 8, under an $8 ceiling. It cost
$5.63 against a $2.50–$3.44 projection, because 7 books escalated to a
whole-book re-read with Haiku and the estimate excludes escalation by
design. Mean contents recall was 94.6% over the 19 books whose
contents name enough recipes to score, with no phantom titles at all,
7 books incomplete (a chunk that failed every model) and 6 failed
chunks in total. The worst twelve, by the report's problem score before
the fixes below:

| Book | Contents recall | Missing | Failed chunks | Flagged | Parse rate | Cost | Wall | Note |
|---|---|---|---|---|---|---|---|---|
| The Cook's Illustrated How-to-Cook Library | 79% | 31 | 2 | 45/129 | 94% | $1.09 | 523 s | 129 chunks; contents entries are section heads; escalated |
| South | 100% | 0 | 0 | 1/42 | 95% | $0.17 | 96 s | clean, but 333 page references unresolved |
| The Mission Chinese Food Cookbook | 98% | 1 | 1 | 4/35 | 89% | $0.21 | 105 s | a pantry glossary chunk failed on paragraph titles |
| Thai Food Made Easy | 86% | 14 | 2 | 11/17 | 90% | $0.36 | 114 s | dot leaders and prep-time labels unclaimed; escalated |
| The Slanted Door | 91% | 10 | 1 | 4/20 | 92% | $0.10 | 67 s | a wine-note chunk failed on the label rule |
| Simple Fare | 91% | 6 | 0 | 5/14 | 79% | $0.19 | 147 s | escalated |
| Bi-Rite Market's Eat Good Food | 86% | 14 | 0 | 14/55 | 88% | $0.32 | 133 s | market guide; product sections read as missing recipes; escalated |
| The Little Paris Kitchen | n/a | 4 | 0 | 19/22 | 98% | $0.46 | 130 s | ingredient lines not recognised as quantities; escalated |
| Indian Food Cookbook:The Taste of Southern I | n/a | 0 | 0 | 5/8 | 79% | $0.10 | 65 s |  |
| Where Cooking Begins | 93% | 6 | 0 | 16/21 | 48% | $0.50 | 147 s | lists without quantities by design; parse rate 48%; escalated |
| Maangchi's Real Korean Cooking | 100% | 0 | 0 | 2/31 | 94% | $0.15 | 89 s |  |
| Van Leeuwen Artisan Ice Cream Book | 94% | 6 | 0 | 5/30 | 99% | $0.11 | 62 s |  |

What the worst books had in common, and what changed:

- **Page furniture the models rightly skip.** Dot leaders, bare prep
  times ("10 minutes") and symbol table cells ("* 6 * PLACE") stayed
  unclaimed and failed the chunk after every retry. They are now
  auto-ignored like structural labels.
- **Contents entries that are sections.** "DIPS AND SALSAS", "5. The
  Butcher Counter", "Flour", "Beef": entries with entries nested under them
  are chapters or sections, never recipes, and front-matter entries never
  count. This was inflating "missing titles" on every book with a deep
  contents.
- **Essays titled by a label or a paragraph.** "WINE BY CHAYLEE PRIETE
  WINE DIRECTOR" and a pantry glossary whose entries begin "Doubanjiang:
  This spicy fermented bean paste…" failed as label and prose titles. Only
  a recipe titled that way is wrong now; a prose-titled essay keeps the
  paragraph as text under a short title.
- **Escalation fed by formatting.** Eight books escalated, mostly on
  `prose_ingredients` and `low_amount_parse_rate` flags, which describe how
  a book prints its lists (Where Cooking Begins lists ingredients without
  quantities on purpose) and which a stronger model cannot change. Those
  flags no longer count towards escalation.
- **Continuations and split recipes** (from the six-book replays):
  continuations now merge into variations and headnote essays, a quoted
  sentence never retitles a recipe, and a headnote essay followed by a
  sidebar-titled recipe is one recipe.
- **The problem score** weighed unresolved page references at 0.5 each, so
  South (100% recall, 333 references) outranked books that lost recipes;
  now 0.1.

## Budget

| Step | Cost |
|---|---|
| A0 probes (15 calls) | < $0.10 |
| eval, reasoning none | $3.07 |
| eval, reasoning low | $1.14 |
| classify calls (dry runs) | < $0.10 |
