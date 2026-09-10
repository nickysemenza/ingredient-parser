# Model catalog

Edit `src/model_catalog.rs` to change model choices, exact IDs, routing metadata,
limits, evaluation notes or prices. This is static Rust data shared by native and
WASM consumers. No build generator, runtime file or deserialization is needed.

Rates are USD per million tokens: input, output, cache-read and cache-write.
`None` means unknown pricing, never free usage. Listed entries appear in model
pickers; unlisted entries preserve exact legacy IDs without recommending them.
IDs are never matched by prefix or substring.

Keep pricing sources and verification dates with each entry. Existing cache
estimates were migrated unchanged, including previous 10% cache-read and 125%
cache-write assumptions where no model-specific rate existed. This refactor
does not constitute fresh pricing verification. Saved snapshots stay unchanged.

Metadata and rate edits do not invalidate extraction caches; transport changes
still do. The default stays explicitly selected, independent of evaluation notes.
