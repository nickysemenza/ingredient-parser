# Changelog

All notable changes to the `ingredient` crate are documented here. The format is
based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the
project aims to follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

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

- Adjective extraction now respects whole-word boundaries, so an adjective inside
  a hyphenated token is left alone: `3 tablespoons well-chopped parsley` keeps
  the name `well-chopped parsley` instead of corrupting it to `well-`.

### Internal

- Removed the stale `feature/nlp-enhancements` worktree/branch and the empty
  `tools/` directory.
- Grew the accuracy corpus with new committed rows for the cases above and new
  `xfail` rows tracking remaining gaps (plural container counts, `N and M/D`
  mixed numbers, `half a <unit>`).
