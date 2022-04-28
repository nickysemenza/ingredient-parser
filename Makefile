build-demo-wasm:
	cd ingredient-wasm && wasm-pack build
	mkdir -p demo-site/src/wasm
	cp -r ingredient-wasm/pkg/ demo-site/src/wasm
build-demo-site: build-demo-wasm
	cd demo-site && yarn && yarn run build
