# Ingredient parsing contract

`Name` is opaque food-bearing text, `Modifier` is free-form authored preparation or descriptive
text, and measures retain exact rational values and range bounds. The parser does
not infer a food ontology or expose its private structural representation.

- Preserve ambiguous coordination without inventing a shared food name:
  `red or white onion` and `canola, vegetable, or melted coconut oil` remain whole
  names. An explicit quantity alternative such as `garlic or 1 teaspoon garlic
  powder` retains the alternative in the modifier. Supported preparation
  alternatives, such as `grated or finely chopped lemon zest`, still separate
  preparation from the food.
- Ingredient dimensions and temperatures describe the ingredient. For example,
  `1 (9-inch) pie crust` has one whole crust and modifier `9-inch`. Standalone
  amounts and instruction rich text still recognize dimensions and temperatures.
  Dimensions within a preparation phrase stay inside that phrase. In instruction
  rich text, explicit temperature spellings such as `365 degrees F` and metric
  dimensions such as `3cm` retain non-scalable measurement kinds. Recipe scaling
  changes ingredient quantities and their alternatives, not temperatures,
  dimensions, or cooking times; source strings and parsed source quantities stay
  unchanged. Length aliases retain their existing unit wire representation.
- Leading and trailing measures receive the same preparation and name handling.
  Optional wrappers compose with trailing measures and derived components such
  as `Juice of 1 lemon`. Counts retain fractions and both range bounds when their
  unit is interpreted from a food name.
- Preserve authored case in extracted text. Render separate modifier parts in
  source order, separated consistently by commas; retain punctuation inside a
  descriptive phrase. Aliases and secondary amounts are interpreted before final
  fields are assigned. A source/yield description such as `lemon juice (from
  about 3 lemons)` remains descriptive text; it is not an equivalent measure of
  the named ingredient. Parenthetical amounts within an explicit alternative
  stay with that alternative.
- Decomposition attributes actual consumed source occurrences. Text normalization
  carries mappings to the authored line. Discarded reference numbers have no
  amount attribution; identical words in different positions are not
  interchangeable evidence.
- Ingredient, usage, notes, decomposition, and diagnostics must agree across
  observation settings and come from the same resolution. The configured unit
  and preparation vocabulary applies across recipe ingredients and instructions.
- Compatible `plus` measures sum exactly in the existing normalized unit (for
  example, `¼ cup plus 1 tablespoon` is 15 teaspoons); this differs from separate
  equivalent measures or a quantified ingredient alternative. Their source
  attribution includes both authored terms.
- Measure qualifiers such as `generous`, `scant`, and sizes before vague/container
  units do not change the quantity and are omitted from structured text. `big`
  and `loose` before a bunch/handful follow this same rule. Sizes describing the
  food itself retain the existing count-unit interpretation.
- `such as` examples and `preferably` descriptions belong in Modifier. Food
  choices whose preparation cannot be attached to one branch in the public
  representation remain opaque, including their internal punctuation and prep.
- Indefinite quantities such as `a little` do not imply a count of one. A
  parenthetical containing only a recognized unit, with no quantity, remains
  descriptive text; the parser does not supply the missing number.
- A package count owns its container in both `2 (200g) blocks` and
  `2 × 200g blocks`. The latter does not introduce a second implicit count;
  ASCII `x` retains its existing arithmetic-multiplier meaning.
- A count noun with no following food, such as `4 cloves, toasted`, remains the
  ingredient name with a whole count. This also applies to configured count
  units; standalone amount parsing still recognizes `4 cloves` as a clove measure.

## Historical label corrections

[corpus-semantic-corrections.json](corpus-semantic-corrections.json) records each
changed regression row with its old and new expectation and the applicable rule.
The independent cookbook labels are separate evidence; see their
[sampling and labeling protocol](../ingredient-parser/tests/corpus/cookbooks/README.md).

Some presentation differences are intentional: extracted text no longer gains
lowercase characters solely because matching was case-insensitive, and leading
preparation appears before later descriptive text. Consumers should treat
Modifier as display text rather than a normalized classification key.
