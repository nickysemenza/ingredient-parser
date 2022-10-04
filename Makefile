build-demo-wasm:
	wasm-pack build ingredient-wasm
	mkdir -p demo-site/src/wasm
	cp -r ingredient-wasm/pkg demo-site/src/wasm
build-demo-site: build-demo-wasm
	cd demo-site && yarn && yarn run build
deploy-demo-site:
	CLOUDFLARE_ACCOUNT_ID=9f10f078d35d86c78dedece2300a6b88 npx wrangler pages publish demo-site/build/ --project-name=ingredient

dev-ui:
	RUST_BACKTRACE=1 RUST_LOG=html5ever=info,selectors=info,debug cargo watch -x 'run --bin food_ui'