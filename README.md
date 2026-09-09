# ingredient-parser

[![codecov](https://codecov.io/gh/nickysemenza/ingredient-parser/branch/main/graph/badge.svg?token=5GJCVD15RH)](https://codecov.io/gh/nickysemenza/ingredient-parser)
[![build + test](https://github.com/nickysemenza/ingredient-parser/actions/workflows/rust.yml/badge.svg)](https://github.com/nickysemenza/ingredient-parser/actions/workflows/rust.yml)
[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/nickysemenza/ingredient-parser)

**ingredient-parser** is a Rust project for parsing recipe ingredient lines into structured data. This repository contains the core library and related tooling.

For full documentation, usage examples, and features, see the detailed [ingredient-parser/readme.md](ingredient-parser/readme.md).

## Maintainer toolkit

Run `pnpm --filter @ingredient-parser/desktop-ui desktop:dev` for the macOS Parser and Cookbooks
desktop workspaces. See [desktop setup and testing](food-app/README.md).
The terminal interface has six command families: `ingredient`, `amount`, `text`,
`recipe`, `cookbook`, and `corpus`. Use `--help` on a family or command to see its
inputs. Results default to human-readable output, including when piped. Select
`--format json` for structured output; `ingredient batch` and `cookbook stats`
also support `--format jsonl`. Progress, errors, and parser traces go to stderr.
`corpus table` writes HTML and does not support a structured format.

```sh
cargo run -p food-cli -- ingredient parse "1 cup flour, sifted"
cargo run -p food-cli -- ingredient parse "1 cup flour" --explain --format json
printf '1 cup flour\n2 tbsp sugar\n' | cargo run -p food-cli -- ingredient batch - --format jsonl
cargo run -p food-cli -- amount parse "2 cups" --format json
cargo run -p food-cli -- cookbook inspect book.epub
cargo run -p food-cli -- cookbook extract book.epub --out run.json
cargo run -p food-cli -- cookbook show run.json --summary
cargo run -p food-cli -- cookbook stats run.json --limit 20
```

Ingredient batches retain every input line, including empty or review-needed
lines, with a line number and an `error` field in structured output. A successful
batch means all lines were processed; review flags remain visible per record.
Cookbook runs are shared with the desktop app and can be inspected, replayed,
audited, and compared offline. Extraction is cache-only unless
`--allow-network` is supplied. See [cookbook review](docs/cookbook-review.md).
Exit codes are 1 for operational errors, 2 for invalid invocations, 3 for
incomplete extraction, and 4 for evaluation mismatches.

## Generated EPUB test fixture

[`recipe-epub-fixtures`](recipe-epub-fixtures/README.md) provides
`cookbook_epub()`, which returns deterministic EPUB bytes for downstream import
tests. It embeds three synthetic recipes and needs no network or extraction model.
To write a copy for manual inspection:

```sh
cargo run -p recipe-epub-fixtures --example write_cookbook -- /tmp/synthetic-cookbook.epub
```

### Frontend workspace

Run `pnpm install --frozen-lockfile` at the repository root. The pnpm workspace
contains `demo-site`, `food-app/ui`, and `packages/recipe-ui`, with one lockfile
and package-manager version. Each app retains its own build configuration.

- `pnpm build`: regenerate the demo WASM bindings and build all frontend packages.
- `pnpm lint`: lint all frontend packages.
- `pnpm test`: run shared presentation and desktop interaction tests.
- `pnpm --filter demo-site dev`: start the demo (run `make build-demo-wasm` first).
- `pnpm --filter @ingredient-parser/desktop-ui desktop:dev`: start the macOS app.

Shared UI code uses `workspace:*` dependencies; source edits do not require a
reinstall. Use `pnpm --filter <package> <command>` to run package-specific checks.
