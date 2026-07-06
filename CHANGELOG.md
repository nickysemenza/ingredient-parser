# Changelog

All notable changes to the `ingredient` crate are documented here. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the
project aims to follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Removed

- `serde-derive` feature: this no-op feature was deprecated and is no longer supported.

### Added

- `IngredientParser::parse_with_diagnostics` returning non-failing parse
  diagnostics (`Diagnostics { confidence, fell_back, unparsed_digit }` with the
  `Confidence` enum) — surfaces whether a line parsed cleanly or quietly fell
  back to a name-only ingredient.
- `ParseTrace::stages` returning a structured `StageReport` (normalize rewrites,
  recognizer attempts, grammar outcome, refine passes, result preview) — the
  data behind the `--explain` stage view, for programmatic consumers.
- Spelled-out word numbers `two`–`twelve` and `dozen` (e.g. `two eggs` →
  `2 whole`). Previously only `one`/`a`/`an` were recognized. Numeric words are
  matched only on word boundaries, so `ten` is not matched inside `tenderloin`.
- `"X of N item"` constructions such as `Juice of 1 lemon` and `Zest of 2 limes`
  now parse to the item as the name with the leading phrase as the modifier.
- Leading preparation words (`packed`, `firmly packed`, `loosely packed`,
  `lightly packed`, `sifted`) are now extracted to the modifier
  (e.g. `1 cup packed brown sugar` → name `brown sugar`, modifier `packed`).
- `impl FromStr for Ingredient` (error type `Infallible`), enabling
  `"2 cups flour".parse::<Ingredient>()` alongside the existing `From<&str>`.
- `IngredientParser::clear_units` and `clear_adjectives` builder methods to drop
  the defaults and recognize only a custom set.
- Runnable examples under `ingredient-parser/examples/` (`parse`,
  `custom_parser`, `rich_text`).
- `CONTRIBUTING.md` and this `CHANGELOG.md`.

### Changed

- `Measure` stores quantities as **exact rationals** internally (`⅓ == ⅓`
  exactly; same-unit addition is exact). The public API and JSON wire format
  are unchanged: `value()`/`upper_value()` still return `f64`, serde still
  reads/writes plain numbers.
- `Display` renders cooking fractions as vulgar glyphs (`½ cup` instead of
  `0.5 cup`, mixed `1¼`), with a decimal fallback for non-fraction values; the
  denormalized value now always pairs with the denormalized **unit** (48 tsp
  displays `1 cup`, not `1 tsp`), and second/hour/day pluralize like minute.
- `serde` is now a required dependency: the optional `serde-derive` feature
  never compiled when disabled (unit/measure types used serde unconditionally)
  and `serde_json` was required anyway. The `serde-derive` feature name remains
  as an empty no-op for compatibility.
- Range endpoints compare by canonical unit, not spelling: `2 tsp to 3
  teaspoons` folds into one ranged measure, `1g-2G` parses as `1-2 g`.
- More count units recognize their plurals (`packets`, `heads`, `bunches`,
  `cans`, `packages`, `tins`, `strands`, `pinches`), and sibilant `-es` plurals
  singularize correctly (`bunches` → `bunch`, not `bunche`).
- `RichParser` ingredient-name extraction is order-independent and matches
  repeated names (earliest-match scan instead of one pass per name).
- `RichParser::new` now accepts any `IntoIterator<Item: Into<String>>` instead of
  only `Vec<String>`, so callers can pass `["flour", "sugar"]` without
  `.to_string()`. Existing `Vec<String>` callers are unaffected; empty literals
  now need an element type (e.g. `Vec::<String>::new()`).
- `"plus"`/`"+"` expressions with incompatible units now keep **both** measures
  instead of silently dropping the second
  (e.g. `1 cup plus 100 grams flour` → `[1 cup, 100 g]`). Compatible units are
  still summed (`1 tbsp plus 1 tsp` → `4 tsp`).
- `Unit::normalize` now promotes an `Other` that holds a known alias to its
  built-in variant (e.g. `Unit::Other("cup")` → `Unit::Cup`); genuinely-unknown
  units are still lowercased and singularized.
- Trailing parenthesized modifiers are unwrapped (`1 cup flour (sifted)` →
  modifier `sifted`).

### Fixed

- Fluid-ounce conversions were 3× off: the fl oz → tsp factor was 2.0 (the
  tablespoons-per-fl-oz value); 1 fl oz now correctly normalizes to 6 tsp.
- `Measure::add` silently kept only the left operand for same-kind custom
  units (`1 clove + 2 cloves` returned `1 clove`; bare counts too). Identical
  custom kinds now sum.
- `from_str` could panic on inputs whose lowercase form changes byte length
  (e.g. `İ`); it now always falls back gracefully.
- `inf`/`nan` no longer parse as quantities anywhere (one number parser had
  bypassed the finite guard), so `nan lb = $5` is rejected instead of becoming
  a 0-lb mapping.
- Prep adjectives are no longer stolen out of `or` alternatives:
  `basil or chopped parsley` keeps `chopped` with the alternative.
- The mixed-number `and` separator works with vulgar fractions (`1 and ½`).
- Deprecated `IngredientError::ParseError`/`Generic` (never produced).
- Adjective extraction now respects whole-word boundaries, so an adjective inside
  a hyphenated token is left alone: `3 tablespoons well-chopped parsley` keeps
  the name `well-chopped parsley` instead of corrupting it to `well-`.

### Internal

- Removed the stale `feature/nlp-enhancements` worktree/branch and the empty
  `tools/` directory.
- Grew the accuracy corpus with new committed rows for the cases above and new
  `xfail` rows tracking remaining gaps (plural container counts, `N and M/D`
  mixed numbers, `half a <unit>`).
