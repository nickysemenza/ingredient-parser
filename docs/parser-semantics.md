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
  Dimensions within a preparation phrase stay inside that phrase.
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

## Historical label corrections

[corpus-semantic-corrections.json](corpus-semantic-corrections.json) records each
changed regression row with its old and new expectation and the applicable rule.
The independent cookbook labels are separate evidence; see their
[sampling and labeling protocol](../ingredient-parser/tests/corpus/cookbooks/README.md).

Some presentation differences are intentional: extracted text no longer gains
lowercase characters solely because matching was case-insensitive, and leading
preparation appears before later descriptive text. Consumers should treat
Modifier as display text rather than a normalized classification key.
