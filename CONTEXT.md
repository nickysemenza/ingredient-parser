# Ingredient Parser

Turns a written ingredient line ("2 cups flour, sifted") into structured data, and the recipes
that contain them into structured recipes. The parser is the product; the CLI, desktop app and
demo site are surfaces onto it.

## Language

### The parsed line

**Ingredient line**:
One line of a recipe's ingredient list, exactly as written by a human.
_Avoid_: item, row, entry

**Name**:
The identity of the food itself, kept as opaque text. Deliberately not decomposed into
head noun and descriptor — that split is food-ontology identity, not grammar.
_Avoid_: ingredient (ambiguous with the whole parsed line), food, product

**Modifier**:
Free-form text carrying preparation and qualification ("sifted", "plus more for dusting").
Not a composition of typed buckets.
_Avoid_: prep, note, annotation

**Usage**:
The role an ingredient plays in the recipe — garnish, frying medium, seasoning, marinade,
dredging, pan grease, or normal.
_Avoid_: purpose, function, category

**Measure**:
A quantity paired with a unit, optionally a range. The canonical domain term; the wasm and
TypeScript surfaces spell it `amount`, which is a boundary spelling, not a second concept.
_Avoid_: quantity (that's the number alone), amount (boundary only)

**Measure kind**:
The dimension a measure belongs to — weight, volume, money, calories, time, temperature,
length, nutrient, or other.
_Avoid_: unit type, dimension, category

**Scalable kind**:
A measure kind that multiplies when a recipe is scaled. Weight, volume and open-ended
`other` units scale; time, temperature and length do not.
_Avoid_: multipliable, adjustable

**Parse notes**:
Diagnostics describing how much to trust a parse — whether it fell back to a name-only
result, and whether a digit in the source produced no measure.
_Avoid_: warnings, errors, flags

### The pipeline

**Stage**:
One of the five ordered phases a line passes through: normalize, recognize, grammar,
segment, refine.
_Avoid_: step, phase, layer

**Clause**:
A delimiter-separated span of an ingredient line, as identified by the segment stage.
_Avoid_: fragment, part, chunk (reserved for EPUB text)

**Pass**:
A single named transformation inside a stage, applied in a defined order relative to its
siblings. A pass that runs during assembly is a **repair**.
_Avoid_: rule, transform, handler

**Decomposition**:
The span-level view of which region of the source line became which parsed field.
_Avoid_: breakdown, mapping, trace (that's the execution record, not the spans)

### The corpus

**Corpus row**:
A committed record pairing an ingredient line with its expected parse. Together the rows
form the regression ratchet that governs whether a parser change ships.
_Avoid_: fixture, test case, sample

**xfail row**:
A corpus row whose labels describe the parse we *want*, not the parse we currently get.
A known gap, recorded rather than hidden.
_Avoid_: pending, skipped, todo

### Recipe ingestion

**Chunk**:
A slice of an EPUB's text sent to the model as a single extraction request.
_Avoid_: section, segment, page

**Title hint**:
The section title carried forward onto a continuation chunk, so a recipe split across a
chunk seam is re-emitted under the same title and can be merged back together.
_Avoid_: continuation marker, carryover
