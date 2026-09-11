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

## Cookbooks workspace

The Cookbooks workspace is presentation over the `cookbook` crate; nothing in the
frontend calls a model.

- **Library** lists the EPUBs in the Calibre folder (or a folder you pick).
  Covers load when an entry scrolls into view, never all at once. Each entry
  shows its catalog cookbook hint and how many saved runs it has.
- **Book** opens offline: the outline, the structural cookbook verdict with its
  reasons, the runs of that exact file, and an estimate (chunks, cache hits,
  tokens, cost range, time range, ladder, assumptions). **Extract** is disabled,
  with the configuration file named, when gateway credentials are missing. A
  running extraction reports settled chunks, in-flight and failed calls, cache
  hits, recipes, cost, elapsed time, the ETA window, and the models answering;
  **Cancel** stops it. The saved run opens when it finishes.
- **Run** shows the chapter tree with per-item counts and kind badges, the
  selected recipe (sections, parsed ingredient lines with confidence, steps,
  notes, photos, provenance), and the ingredient inspector shared with the
  Parser workspace. Reference chips jump to the item they name. The
  **Diagnostics** tab carries the run summary and crosscheck, stage timings,
  usage by model, the chunk table (failed and flagged first), the full call log,
  unresolved references, the ETA trace, and the raw report.
- **Run history** lists every saved run, newest first, and can open or delete
  one. Deletion is the only write the workspace performs.

## Offline fixtures

```sh
cargo run -p food-app --example create_run_fixture -- /tmp/food-app-qa
```

The generator writes a real EPUB, extracts it through the production pipeline
with an in-process oracle transport (no paid model calls), saves the run under
`/tmp/food-app-qa/runs` by pointing the run store there with `COOKBOOK_RUNS_DIR`,
and writes `frontend-fixture.json`. The Playwright bridge answers every command
from that bundle: `book`, `estimate`, `extraction`, `runs`, `library`,
`gateway`, `ingredients`, `inspections`, `corpus`, `webRecipe`,
`scaledWebRecipe`.

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
cargo run -p food-app --example create_run_fixture -- /tmp/food-app-qa
pnpm --filter @ingredient-parser/desktop-ui exec playwright install webkit
pnpm --filter @ingredient-parser/desktop-ui test:e2e
cargo run -p food-app --example export_bindings -- --check
cargo nextest run -p food-app
cargo clippy -p food-app --all-targets -- -D warnings
```

Browser tests exercise interaction through a fixture-backed command bridge.
Native macOS smoke testing separately verifies the actual command boundary,
file dialogs, clipboard, and preference restoration. Imported source is rendered
as text and controlled elements rather than executable HTML.

Recipe scale controls and JSON presentation are shared with the WASM demo in
[`packages/recipe-ui`](../packages/recipe-ui/README.md). Execution remains native
Rust in this app; the shared package contains presentation only.

## Native shell

- **File → Open…** (Cmd–O) opens an EPUB anywhere on disk in the Cookbooks
  workspace. **View → Parser / Cookbooks** (Cmd–1 / Cmd–2) switches workspaces.
- **Run actions → Reveal in Finder** locates the saved run file.
- **Open original recipe in browser** is available in a loaded web recipe's
  Recipe & source presentation.
- The native titlebar tracks the open book and the theme. Quitting or closing
  while an extraction runs asks first; the bottom status bar shows extraction
  progress, cost so far, and the remaining ETA.
