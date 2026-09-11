# Desktop interface

React and TypeScript render the Parser and Cookbooks workspaces inside the Tauri shell. The Cookbooks workspace lives in `src/cookbooks/` (library, book and estimate, run tree, diagnostics, run history). Application DTOs are generated from Rust into `src/generated.ts`; edit their Rust definitions rather than this file. The typed invocation boundary is `src/bridge.ts`.

```sh
pnpm install # installs the root workspace
pnpm desktop:dev
pnpm build
pnpm lint
pnpm test
```

Browser-only development uses `pnpm dev`. Local application commands require the desktop shell; browser interaction tests inject a fixture bridge built from actual Rust DTOs.

```sh
pnpm exec playwright install webkit
UI_FIXTURE_PATH=/path/to/frontend-fixture.json pnpm test:e2e
```

The repository's `create_run_fixture` Rust example produces `frontend-fixture.json`. Tests exercise both desktop sizes, appearances, the library scan, a book's offline estimate, a held extraction's progress, the saved run's tree and diagnostics, run history, and ingredient inspection. They never call a paid backend.

Preferences store only idle view choices, inputs, and pane proportions in local storage. Scanning the library, opening a book or a saved run, restoring a workspace, or changing its appearance never starts an extraction. Deleting a saved run is the only write the interface performs; every parse and extraction stays in Rust.

The root `pnpm-workspace.yaml`, `package.json`, and `pnpm-lock.yaml` own dependency
installation and the pnpm version. This package owns its Vite and Tauri commands.
Shared presentation tests live in `packages/recipe-ui`; run `pnpm test` at the
repository root to test both packages.
