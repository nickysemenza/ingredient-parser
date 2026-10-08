# One frontend (ui/) serves the website and the desktop app.
build-wasm:
	pnpm --filter @ingredient-parser/ui wasm
build-site:
	pnpm install --frozen-lockfile
	pnpm --filter @ingredient-parser/ui wasm
	pnpm --filter @ingredient-parser/ui build
deploy-site:
	CLOUDFLARE_ACCOUNT_ID=9f10f078d35d86c78dedece2300a6b88 npx wrangler pages deploy ui/dist/ --project-name=ingredient

dev-web: build-wasm
	pnpm --filter @ingredient-parser/ui dev

dev-ui:
	pnpm --filter @ingredient-parser/ui desktop:dev

# Mirrors the `deny` job in .github/workflows/rust.yml: unmaintained-crate
# advisories are informational (see deny.toml), so they're allow-listed on
# the command line rather than failing local runs; a security vulnerability
# still fails.
deny:
	cargo deny check -A unmaintained
