# WASM boundary checks

Run the browser suite with `wasm-pack test --headless --chrome ingredient-wasm` from the workspace root.

For a fast smoke check of the actual generated JavaScript exports and their serialization boundary:

```sh
make build-demo-wasm
node --experimental-wasm-modules ingredient-wasm/tests/boundary.mjs
```

The script imports the generated bundler package directly. It checks recipe sections, omitted optional fields, all instruction measurement alternatives, shared ingredient names, scaling, and Unicode source reconstruction. Rebuild the package after Rust changes; this smoke check complements the browser suite.
