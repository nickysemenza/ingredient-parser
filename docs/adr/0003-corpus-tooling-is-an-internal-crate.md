# Corpus tooling lives in an internal crate, not in the published `ingredient`

The corpus row schema, its loader and its scoring rule were written five times
across three crates, each copy carrying a comment claiming to mirror
`tests/accuracy.rs` — which, living in a test target, none of them could call.
They now live in `ingredient-corpus`, a `publish = false` workspace member.

Two alternatives were rejected. Putting it behind a non-default feature on
`ingredient` would ship test infrastructure to crates.io and make the corpus
schema part of a published interface. Putting it in a new `food-cli` library
would invert the workspace, since `ingredient-parser`'s own integration tests
are the primary consumer.

## Consequences

`ingredient` dev-depends on `ingredient-corpus`, which depends on `ingredient` —
a cycle Cargo permits because it passes through a dev-dependency. Two things
keep that off crates.io, and both must hold:

- The workspace dependency entry is **path-only**. Cargo strips a `path`-only
  dev-dependency when packaging; adding a `version` key would publish a
  dependency on an unpublished crate.
- `ingredient`'s `[package]` sets `exclude = ["/tests"]`, because `cargo package`
  otherwise ships `tests/`, whose `use ingredient_corpus` would not resolve from
  the published tarball. `benches/` must stay packaged — `[[bench]]` names a file
  and packaging fails without it.

Verify both with:

```
cargo package -p ingredient --list | grep -c '^tests/'   # 0
```
