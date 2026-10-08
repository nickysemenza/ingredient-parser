# ingredient-parser

[![codecov](https://codecov.io/gh/nickysemenza/ingredient-parser/branch/main/graph/badge.svg?token=5GJCVD15RH)](https://codecov.io/gh/nickysemenza/ingredient-parser)
[![build + test](https://github.com/nickysemenza/ingredient-parser/actions/workflows/rust.yml/badge.svg)](https://github.com/nickysemenza/ingredient-parser/actions/workflows/rust.yml)
[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/nickysemenza/ingredient-parser)

**ingredient-parser** is a Rust project for parsing recipe ingredient lines into structured data. This repository contains the core library and related tooling.

For full documentation, usage examples, and features, see the detailed [ingredient-parser/readme.md](ingredient-parser/readme.md).

## Maintainer toolkit

Run `pnpm --filter @ingredient-parser/ui desktop:dev` for the macOS Parser and Cookbooks
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

### Frontend

`ui/` is the one frontend: the public website (landing page plus the Parser and
Cookbooks workbench) and the desktop window are the same build. Both run the
same Rust command layer, [`food-core`](food-core): natively in the desktop app,
compiled to WebAssembly ([`food-wasm`](food-wasm)) in a worker on the web. The
TypeScript contract `ui/src/api/generated.ts` is generated from Rust.

Run `pnpm install --frozen-lockfile` at the repository root, then:

- `pnpm dev`: build the WASM package and start the website at http://localhost:1420.
- `pnpm --filter @ingredient-parser/ui desktop:dev`: start the macOS app.
- `pnpm build`: build the WASM package and the frontend (`ui/dist`).
- `pnpm lint` / `pnpm test`: lint and unit-test the frontend.
- `pnpm --filter @ingredient-parser/ui test:e2e`: Playwright against the production
  build — the website in Chromium with real WASM, and the desktop shell in WebKit
  against a recorded fixture (see [food-app](food-app/README.md#end-to-end-tests)).
- `cargo run -p food-app --example export_bindings`: regenerate the TypeScript contract.

The WASM build needs the `wasm32-unknown-unknown` target and `wasm-pack`.

The website is a Cloudflare Worker serving `ui/dist` as static assets
([`ui/wrangler.jsonc`](ui/wrangler.jsonc)). CI deploys it to
https://ingredient.nickysemenza.com on every push to `main`, and uploads a
Worker Preview named `pr-<number>` for each pull request from this repository.
