# Refactor and Cleanup Design

## Overview

Comprehensive cleanup of the ingredient-parser workspace covering core library structure, project organization, and naming consistency.

## Quick Wins

### 1. Delete `recipe-epub/`
Empty directory with no code, not in workspace members. Remove entirely.

### 2. Rename `unit/lib.rs` → `unit/core.rs`
The name `lib.rs` inside a module is confusing. The file contains the `Unit` enum, mappings, and validation functions - `core.rs` is clearer.

Update `unit/mod.rs`:
```rust
// Before
pub(crate) mod lib;
pub use lib::*;

// After
pub(crate) mod core;
pub use core::*;
```

### 3. Extract `truncate_str` to shared utility
Currently duplicated between `food_ui/src/lib.rs` and `ingredient-parser/src/trace.rs`. Move to `ingredient-parser/src/util.rs` as public function and reuse.

### 4. Remove dead `wee_alloc` code
In `ingredient-wasm/src/lib.rs`, remove the `wee_alloc` feature and associated code. Modern WASM doesn't need custom allocators.

## Consolidate `food_web` into `food_ui`

### Current State
- `food_ui` (520 lines) - Contains `MyApp` struct and all egui UI logic
- `food_web` (43 lines) - Just a `main.rs` that wraps `food_ui::MyApp`
- `food_ui` also has its own `main.rs` (17 lines) for native-only

### Action
1. Move `food_web/src/main.rs` WASM + native logic into `food_ui/src/main.rs`
2. Delete `food_web` crate entirely
3. Rename `food_ui` → `food-app`

### Result
- One crate instead of two
- Clear that `food-app` is the runnable application
- Handles both native and WASM entry points

## Standardize Crate Naming

### Current Inconsistency
- Hyphens: `ingredient-parser`, `recipe-scraper`, `ingredient-wasm`
- Underscores: `food_ui`, `food_web`, `food_cli`, `recipe_scraper_fetcher`

### Rename to Hyphens
| Current | New |
|---------|-----|
| `food_ui` | `food-app` (merged with food_web) |
| `food_cli` | `food-cli` |
| `recipe_scraper_fetcher` | `recipe-scraper-fetcher` |

Note: The published crate `ingredient` keeps its name unchanged.

## Split Large Files in Core Library

### `lib.rs` (489 lines)
- Keep `lib.rs` as thin public API facade (~100 lines)
- Move `IngredientParser` struct + methods to `parser/config.rs`
- Move default units/adjectives lists to `parser/defaults.rs`

### `parser/measurement.rs` (729 lines)
Split into submodule:
- `parser/measurement/mod.rs` - `MeasurementParser` struct, core methods
- `parser/measurement/range.rs` - Range parsing (`1-2 cups`)
- `parser/measurement/composite.rs` - Plus expressions, parenthesized amounts
- `parser/measurement/number.rs` - Number/fraction parsing

### `trace.rs` (526 lines)
Split into submodule:
- `trace/mod.rs` - Public types (`ParseTrace`, `ParseWithTrace`)
- `trace/collector.rs` - Thread-local `TraceCollector`
- `trace/format.rs` - Tree formatting for display
- `trace/jaeger.rs` - Jaeger JSON export

## Fix Deprecated Code & Minor Cleanup

### Update deprecated `wasm-bindgen` patterns
In `ingredient-wasm/src/lib.rs`, migrate from deprecated `JsValue::from_serde()`:

```rust
// Before
JsValue::from_serde(&result).unwrap()

// After
serde_wasm_bindgen::to_value(&result).unwrap()
```

Add `serde-wasm-bindgen` dependency and remove `#[allow(deprecated)]`.

### Consolidate text parsing functions
`parser/helpers.rs` has `text()` and `rich_text.rs` has `text2()`. Either:
- Create a parameterized version accepting allowed characters
- Or document clearly why they differ

### Extract graph logic from `unit/measure.rs`
Move unit conversion graph code to `unit/conversion.rs`:
- `make_graph()`
- `convert_measure_via_mappings()`
- Related graph types

## Implementation Order

1. **Phase 1 - Quick wins** (no breaking changes)
   - Delete `recipe-epub/`
   - Rename `unit/lib.rs` → `unit/core.rs`
   - Extract `truncate_str` to util
   - Remove `wee_alloc`

2. **Phase 2 - Crate consolidation**
   - Merge `food_web` into `food_ui`
   - Rename to `food-app`
   - Rename `food_cli` → `food-cli`
   - Rename `recipe_scraper_fetcher` → `recipe-scraper-fetcher`

3. **Phase 3 - File splitting**
   - Split `lib.rs`
   - Split `parser/measurement.rs`
   - Split `trace.rs`

4. **Phase 4 - Cleanup**
   - Update deprecated wasm-bindgen
   - Consolidate text parsers
   - Extract graph logic
