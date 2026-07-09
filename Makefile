build-demo-wasm:
	wasm-pack build ingredient-wasm
	mkdir -p demo-site/src/wasm
	cp -r ingredient-wasm/pkg demo-site/src/wasm
build-demo-site: build-demo-wasm
	cd demo-site && pnpm install --frozen-lockfile && pnpm run build
deploy-demo-site:
	CLOUDFLARE_ACCOUNT_ID=9f10f078d35d86c78dedece2300a6b88 npx wrangler pages publish demo-site/dist/ --project-name=ingredient

dev-ui:
	RUST_BACKTRACE=1 cargo watch -x 'run --bin food-app'

# Mirrors the `deny` job in .github/workflows/rust.yml: unmaintained-crate
# advisories are informational (see deny.toml), so they're allow-listed on
# the command line rather than failing local runs; a security vulnerability
# still fails.
deny:
	cargo deny check -A unmaintained
