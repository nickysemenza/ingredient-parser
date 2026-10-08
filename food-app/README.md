# Desktop maintainer toolkit

The macOS application uses Tauri 2 and the shared React frontend in [`ui/`](../ui).
Portable commands (parse, inspect, recipes, corpus, opening books and runs) go
through one `core` command into [`food-core`](../food-core), the same dispatch
table the website runs as WebAssembly. Native-only commands (library, run store,
model calls, export, Finder) live in `src/desktop.rs`. The desktop
shell requires macOS 13.3 or newer; the command service tests remain portable.

## Development

Install Rust, the Xcode command-line tools, Node.js, and pnpm, then:

```sh
pnpm install
pnpm --filter @ingredient-parser/ui wasm
pnpm --filter @ingredient-parser/ui desktop:dev
```

The Vite UI runs inside the native Tauri window. `pnpm --filter @ingredient-parser/ui dev`
serves the same frontend as the website, where desktop-only actions are hidden.

Build an unsigned local application bundle:

```sh
pnpm --filter @ingredient-parser/ui desktop:build
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
  notes, photos, provenance) or its **Source** lines from the EPUB, and the
  ingredient inspector shared with the Parser workspace. Reference chips jump to the item they name. The
  **Diagnostics** tab carries the run summary and crosscheck, stage timings,
  usage by model, the chunk table (failed and flagged first), the full call log,
  unresolved references, the ETA trace, and the raw report.
- **Export bundle** on a saved run packages the full extraction, all referenced
  images, and an offline HTML review page. It uses the same native exporter as
  `food-cli cookbook export`; see [the format](../docs/cookbook.md#portable-cookbook-bundles).
- **Run history** lists every saved run, newest first, and can open or delete
  one. Export never modifies a saved run or calls a model.

## End-to-end tests

```sh
cargo run -p food-app --example create_run_fixture -- /tmp/food-app-qa
pnpm --filter @ingredient-parser/ui exec playwright install chromium webkit
pnpm --filter @ingredient-parser/ui test:e2e
cargo run -p food-app --example e2e_artifact -- seal /tmp/food-app-qa ui/test-results
cargo run -p food-app --example e2e_artifact -- verify /tmp/food-app-qa/e2e-artifact
```

The generator writes a real EPUB, extracts it through the production pipeline
with an in-process oracle transport (no paid model calls), saves the run under
`/tmp/food-app-qa/runs` (via `COOKBOOK_RUNS_DIR`), exports its bundle, and writes
`frontend-fixture.json`: the answers to native-only commands (`library`, `runs`,
`estimate`, `gateway`, `bundle`, `recipeHtml`) plus the EPUB, run and corpus
files, base64-encoded under their native paths.

Playwright runs the production build twice:

- **web** (Chromium): the website, with every command answered by the real WASM
  worker. Expectations come from inputs, not from the implementation — the
  authored line must round-trip through its segments, and the uploaded corpus
  must score exactly as many cases as the file has rows.
- **desktop** (WebKit, as WKWebView): the desktop shell. `__FIXTURE_INVOKE__`
  answers native-only commands from the fixture and records each call;
  `__FIXTURE_FILES__` preloads the fixture files into the WASM worker so opening
  the book and the run, photos and source lines run real Rust.

Sealing copies the EPUB, saved run, exported `.cookbook.zip`, corpus, Playwright
report and screenshots into `e2e-artifact/` with a SHA-256 manifest
(`evidence.json`) and the replay command. Verification re-hashes every file,
checks each bundle asset, re-exports the retained inputs and compares the
archive bytes. CI uploads the directory as `ui-e2e-evidence`.

## Command bindings

Boundary types live in `food-core` (portable) and `src/backend.rs` (native).
After changing one, regenerate the checked-in `ui/src/api/generated.ts`:

```sh
cargo run -p food-app --example export_bindings
cargo run -p food-app --example export_bindings -- --check
```

## Checks

```sh
pnpm --filter @ingredient-parser/ui lint
pnpm --filter @ingredient-parser/ui test
pnpm --filter @ingredient-parser/ui build
cargo run -p food-app --example export_bindings -- --check
cargo nextest run -p food-app -p food-core
wasm-pack test --node food-wasm
cargo clippy -p food-app --all-targets -- -D warnings
```

Native macOS smoke testing separately verifies the Tauri command boundary, file
dialogs, clipboard, and preference restoration. Imported source is rendered as
text and controlled elements rather than executable HTML.

## Native shell

- **File → Open…** (Cmd–O) opens an EPUB anywhere on disk in the Cookbooks
  workspace. **View → Parser / Cookbooks** (Cmd–1 / Cmd–2) switches workspaces.
- **⌘K** opens the command palette: every navigation, appearance and workspace
  action.
- **Run actions → Reveal in Finder** locates the saved run file.
- **Original** in a loaded web recipe opens the page in the browser.
- The native titlebar tracks the open book and the theme. Quitting or closing
  while an extraction runs asks first; the bottom status bar shows extraction
  progress, cost so far, and the remaining ETA.
