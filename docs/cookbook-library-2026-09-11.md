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

## New answer keys

Four of the sample's worst books, chosen for shapes the six did not cover,
were labelled from their HTML by Opus subagents (the run only seeded the
skeleton's candidates; every count came from the markup):

| Key | Shape | Recipes | What the markup taught |
|---|---|---|---|
| Thai Food Made Easy | calibre-page-split | 100 | one `div.chapter` per recipe cut into three files (photo, recipe, sidebar); `p.serves` used for yield, group label and step alike; dot leaders and bare "preparation"/"cooking" labels on every recipe |
| Bi-Rite Market's Eat Good Food | publisher-div-based | 89 | one `div.recipe` with a fixed child sequence; the contents interleave product sections with recipes, so a contents-only recall would count "Flour" and "Beef" as missing recipes |
| The Little Paris Kitchen | publisher-classes | 117 + 3 variants | each ingredient list is one paragraph with `•` between the items, which no quantity heuristic could see; the contents list chapters only |
| The Slanted Door | publisher-div-based | 109 | no `<p>` at all; the c04 contents anchors are off by one (the "Chicken Stock" entry points at the chapter header), and three dessert recipes sit right after two stacked non-recipe headings |

Scored from the cache before any further change: Bi-Rite 100% recall, no
phantoms, samples 50% (photo counts, where the key counts the next recipe's
lead photo inside the block); Slanted Door 100% recall with two phantoms
("GINGER SYRUP" and "HONEY SYRUP", sub-recipes with their own lists inside
cocktails, a judgment call the key treats as sections); Thai Food 78% with
three chunks failing on bare timing labels and two "phantoms" that are the
same recipe with its subtitle joined on ("Pineapple with caramelised chilli
sauce" for the heading "Pineapple"); The Little Paris Kitchen 39%, the
bullet-paragraph shape.

Three changes followed: bare timing labels ("preparation", "cooking",
"3 minutes (per batch)") are furniture; the cleaner splits a paragraph of
three or more bulleted, mostly quantity-like items into one line per item;
and title matching pairs exact titles first and then lets a one-word title
take a joined subtitle of two or more words. Re-scored with a two-model
ladder while Luna was unavailable: **Thai Food 78% → 99%** (one missing,
two label phantoms), **The Little Paris Kitchen 39% → 94%** (seven missing,
no phantoms). The bullet split changes the chunk text of every book that
prints bullets, so those books cost a real re-run; a partial re-sweep
under a $3 ceiling (14 of the 25 books, two-model ladder, before the last
label rule) reached 98.9% mean contents recall on the nine scorable books
against 94.6% before, and is not otherwise comparable.

The six earlier keys had their judgment calls resolved from the HTML too:
Flour Water Salt Yeast's three worked-example formulas are `not_recipes`
(tables with commentary and no method) while "FEEDING YOUR LEVAIN", which
has a bulleted list and a method, is a recipe; Zuni's "HOUSE-CURED PORK
CHOP & TENDERLOIN…" is a recipe and "THE BASIC BRINE" its ingredient
section; Bouchon's "Eclairs" is a family head like "Macarons". Nothing
Fancy and Pok Pok were already right.

## What the phase found about the providers

GPT 5.6 Luna is billed through the account's own OpenAI key, not the
gateway's unified billing, and that key ran out of credit mid-phase: every
Luna call answered 429 "You have no credits remaining", and the run burned
three backoff retries per chunk before falling to Haiku, which is why the
second live six-book eval came in at 97.2% recall and 280 s for Pok Pok.
That message now exhausts the model for the run at the first refusal. The
clean gate number for this phase needs the key topped up (or a two-model
ladder), so the last measurements below use `gemini-2.5-flash,
claude-haiku-4-5`.

## Where it stands

Live, `--no-cache`, default ladder (Gemini at low reasoning, Luna, Haiku), the six original keys and the four new ones, after every commit above:

| Book | Recall | Missing | Phantoms | Leaks | Samples | Wall | Cost |
|---|---|---|---|---|---|---|---|
| Bi-Rite Market's Eat Good Food | 100.0% | 0 | 0 | 0 | 62% | 84 s | $0.24 |
| Bouchon Bakery | 97.9% | 3 | 1 | 1 | 88% | 119 s | $0.19 |
| Dessert Person | 100.0% | 0 | 0 | 0 | 100% | 45 s | $0.15 |
| Flour Water Salt Yeast | 97.4% | 1 | 0 | 0 | 88% | 87 s | $0.10 |
| The Little Paris Kitchen | 94.0% | 7 | 0 | 0 | 38% | 48 s | $0.11 |
| Nothing Fancy | 100.0% | 0 | 0 | 0 | 100% | 41 s | $0.09 |
| Pok Pok_ Food and Stories From the | 98.9% | 1 | 0 | 0 | 50% | 108 s | $0.22 |
| The Slanted Door | 100.0% | 0 | 2 | 0 | 62% | 51 s | $0.08 |
| Thai Food Made Easy | 99.0% | 1 | 4 | 4 | 0% | 81 s | $0.44 |
| The Zuni Cafe Cookbook_ A Compendi | 99.0% | 2 | 0 | 0 | 75% | 62 s | $0.24 |

Mean contents recall **98.6%** over ten books (98.4% over six before this pass), 7 phantoms (8 before, over six), max wall **119 s** (183 s before), $1.88 for ten books. The gate still fails on five leaks, all judgment calls between the keys and the extractor: Bouchon's "Eclairs" family head, which the extractor gives the éclair recipe's ingredients, and Thai Food Made Easy's four "6 ways with" entries, prose paragraphs with inline quantities that the model reads as recipes. Slanted Door's two phantoms are cocktail sub-syrups with their own lists.

## Budget

| Step | Cost |
|---|---|
| A0 probes (15 calls) | < $0.10 |
| eval, reasoning none | $3.07 |
| eval, reasoning low | $1.14 |
| classify calls (dry runs) | < $0.10 |
| 25-book sample sweep | $5.63 |
| live six-book eval, Luna unavailable | $0.82 |
| Thai Food and Little Paris Kitchen re-scores | $0.75 |
| partial re-sweep (14 books) | $3.06 |
| final live ten-book eval | $1.88 |
| **total** | **≈ $17** of the $25 ceiling |
