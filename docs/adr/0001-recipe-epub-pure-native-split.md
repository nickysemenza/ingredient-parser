# `recipe-epub`'s pure/native split exists for an out-of-repo browser driver

`recipe-epub` gates `reqwest`/`tokio`/`futures`/`walkdir` behind a `native` feature so the
chunking and assembly half compiles to wasm without them. That half — `chunk_epub`,
`build_chunk_request`, `try_extract_chunk`, `assemble_recipes`, `epub_metadata`, `CallResult` —
has **zero callers inside this repo**, which makes it read like dead surface built for a
consumer that never arrived. It isn't: cubby's `recipebridge` crate depends on `recipe-epub`
with `default-features = false` and drives the whole extraction loop client-side, calling back
into JS only for the authenticated LLM hop. `try_extract_chunk`'s generic closure is how that
callback is injected, and it is the reason retry and salvage policy can't drift between the
browser and native paths.

## Consequences

Nothing in this repo exercises the pure half, so CI must check it explicitly — otherwise the
first thing to break cubby is a `native`-only import added without noticing. Verify with:

```
cargo check -p recipe-epub --no-default-features
```

Do not delete these exports, collapse `CallResult`/`DrivenChunk`/`ChunkOutcome`, or drop the
generic closure on the basis of a repo-local caller search. See ADR-0002 for why a break here
lands immediately.
