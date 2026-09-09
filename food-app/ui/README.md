# Desktop interface

React and TypeScript render the Parser and Cookbooks workspaces inside the Tauri shell. Application DTOs are generated from Rust into `src/generated.ts`; edit their Rust definitions rather than this file. The typed invocation boundary is `src/bridge.ts`.

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

The repository's `create_review_fixture` Rust example produces `frontend-fixture.json`. Tests exercise both desktop sizes, appearances, source/result inspection, review persistence commands, unsaved-change protection, and explicitly requested cache-only extraction. They never call a paid backend.

Preferences store only idle view choices, inputs, and pane proportions in local storage. Opening a source, restoring a workspace, or changing its appearance does not start extraction. Review files and all parser calculations remain owned by Rust.

The root `pnpm-workspace.yaml`, `package.json`, and `pnpm-lock.yaml` own dependency
installation and the pnpm version. This package owns its Vite and Tauri commands.
Shared presentation tests live in `packages/recipe-ui`; run `pnpm test` at the
repository root to test both packages.
