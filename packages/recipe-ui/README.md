# Shared recipe presentation

`@ingredient-parser/recipe-ui` contains React presentation used by both
`demo-site` and `food-app/ui`. Both consume it through `workspace:*` in the root
pnpm workspace. Each frontend retains its own Vite configuration and build.
Shared source edits are available immediately without reinstalling snapshots.

`RecipeScale` supplies accessible preset/custom controls and accepts a numeric
callback. The demo adapter calls WASM; the desktop adapter calls native Rust
through Tauri. This package does not parse, scale amounts, load WASM, invoke IPC,
or access files. `JsonView` renders serialized values as escaped text. Callers
provide styling so the public demo and dense desktop retain their own appearance.

Run `pnpm install` from the repository root. This package owns its checks:

```sh
pnpm --filter @ingredient-parser/recipe-ui build
pnpm --filter @ingredient-parser/recipe-ui lint
pnpm --filter @ingredient-parser/recipe-ui test
```

Keep generated Rust and WASM types in their respective adapters. Add shared
components when both frontends need the same behavior.
