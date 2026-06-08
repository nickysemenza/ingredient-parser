// WASM is loaded once here via top-level await, so every consumer imports the
// resolved `wasm` module and calls it synchronously — no provider, no nullable
// context, no per-call guards. The dynamic import is resolved before React
// mounts (see main.tsx), so by the time any component runs `wasm` is ready.
// The bundler-target entry runs `__wbindgen_start()` on import, so no `init()`
// call is needed.
export const wasm = await import("./wasm/pkg/ingredient_wasm");

// The boundary types are generated from the Rust structs via `#[derive(Tsify)]`
// (ingredient-wasm), so we import them directly instead of re-declaring shapes.
// Aliased to the demo's domain names.
export type {
  RichItem,
  WAmount as Measure,
  WScrapedRecipe as ScrapedRecipe,
} from "./wasm/pkg/ingredient_wasm";
