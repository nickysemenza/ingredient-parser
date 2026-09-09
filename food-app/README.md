# Desktop maintainer toolkit

The macOS application uses Tauri 2 and React. Its Rust command services call the
same parser, corpus, recipe, and durable cookbook-run APIs as the CLI. The desktop
shell requires macOS 13.3 or newer; the command service tests remain portable.

## Development

Install Rust, the Xcode command-line tools, Node.js, and pnpm, then:

```sh
pnpm install
pnpm --filter @ingredient-parser/desktop-ui desktop:dev
```

The Vite UI runs inside the native Tauri window. `pnpm --filter @ingredient-parser/desktop-ui dev`
starts only the browser frontend, which requires its test bridge for native
operations. It is not a separate browser-hosted product.

Build an unsigned local application bundle:

```sh
pnpm --filter @ingredient-parser/desktop-ui desktop:build
```

The bundle is written beneath `target/release/bundle/macos/`. No signing,
notarization, or updater credentials are required for local development.

## Offline fixtures

```sh
cargo run -p food-app --example create_review_fixture -- /tmp/food-app-qa
pnpm --filter @ingredient-parser/desktop-ui desktop:build --debug
"target/debug/bundle/macos/Ingredient Parser.app/Contents/MacOS/food-app" --review-run /tmp/food-app-qa/cookbook-run.json
```

The fixture generator creates a real EPUB using `recipe-epub-fixtures`, supplies
an in-process mock extractor, and writes a durable run and review sidecar through
the production APIs. No paid model calls are used.

## Command bindings

Application boundary types live in `src/backend.rs`. After changing a DTO,
regenerate the checked-in frontend declarations:

```sh
cargo run -p food-app --example export_bindings
cargo run -p food-app --example export_bindings -- --check
```

## Checks

```sh
pnpm --filter @ingredient-parser/desktop-ui lint
pnpm --filter @ingredient-parser/desktop-ui test
pnpm --filter @ingredient-parser/desktop-ui build
pnpm --filter @ingredient-parser/desktop-ui exec playwright install webkit
cargo run -p food-app --example create_review_fixture -- /tmp/food-app-qa
pnpm --filter @ingredient-parser/desktop-ui test:e2e
cargo test -p food-app
cargo clippy -p food-app --all-targets -- -D warnings
```

Browser tests exercise interaction through a fixture-backed command bridge.
Native macOS smoke testing separately verifies the actual command boundary,
file dialogs, clipboard, startup run, and preference restoration. Imported source
is rendered as text and controlled elements rather than executable HTML.

Recipe scale controls and JSON presentation are shared with the WASM demo in
[`packages/recipe-ui`](../packages/recipe-ui/README.md). Execution remains native
Rust in this app; the shared package contains presentation only.

## Daily review controls

- **Open → Recent runs** reopens one of the last eight saved runs. Restoring the
  list does not open files, extract content, or start network requests.
- **Run tools → Reveal … in Finder** locates the source EPUB, saved run, or review
  sidecar. Save a review first to create its sidecar.
- **Open original recipe in browser** is available in a loaded web recipe's
  Recipe & source presentation.
- **Review** in the macOS menu bar, or **Review actions** beside the review status,
  exposes Cmd–Option–A (accepted), Cmd–Option–I (incorrect), Cmd–Option–U
  (uncertain), and Cmd–Shift–Return (save and next unreviewed).
- Save-and-next follows source order, wraps once, and clears filters when moving.
  A failed save keeps the current document, decision, and notes intact.

The native titlebar tracks the selected document and theme. The bottom status
bar shows saved/unsaved state, extraction progress, and remaining review count.
