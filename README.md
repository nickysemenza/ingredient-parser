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
`--format json` for structured output; `ingredient batch` also supports
`--format jsonl`. Progress, errors, and parser traces go to stderr.
`corpus table` writes HTML and does not support a structured format.

```sh
cargo run -p food-cli -- ingredient parse "1 cup flour, sifted"
cargo run -p food-cli -- ingredient parse "1 cup flour" --explain --format json
printf '1 cup flour\n2 tbsp sugar\n' | cargo run -p food-cli -- ingredient batch - --format jsonl
cargo run -p food-cli -- amount parse "2 cups" --format json
cargo run -p food-cli -- cookbook inspect book.epub
cargo run -p food-cli -- cookbook estimate book.epub
cargo run -p food-cli -- cookbook extract book.epub --out run.json
cargo run -p food-cli -- cookbook explain run.json --title "piecrust"
```

Ingredient batches retain every input line, including empty or review-needed
lines, with a line number and an `error` field in structured output. A successful
batch means all lines were processed; review flags remain visible per record.

The `cookbook` family reads an EPUB offline (`inspect`, `explain`), prices a run
before spending anything (`estimate`), extracts through the AI gateway
(`extract`), and re-runs a recorded dump with no credentials (`replay`). `eval`
scores extractions against hand-authored answer keys, `sample` draws random
recipes or books, and `runs` and `models` list saved runs and the model catalog.
See [cookbook extraction](docs/cookbook.md), or
`cargo run -p food-cli -- cookbook --help`.
Exit codes are 1 for operational errors, 2 for invalid invocations, 3 for
incomplete extraction, and 4 for evaluation mismatches.

## Generated EPUB test fixtures

[`cookbook-fixtures`](cookbook-fixtures) builds deterministic EPUB bytes in
memory — `cookbook_epub()` plus fixtures covering real publisher layouts
(`split_spine`, `epub3_nav_pagebreaks`, `typographic_variations`). They need no
network and no extraction model. To write one out for manual inspection:

```sh
cargo run -p cookbook-fixtures --example write_fixture -- cookbook /tmp/synthetic-cookbook.epub
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
