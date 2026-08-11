# Three `publish = false` crates are a public API surface

`recipe-scraper`, `recipe-epub` and `recipe-types` are marked `publish = false`, which normally
means "internal, change freely". They are not. Cubby's `recipebridge` crate depends on them —
and on the published `ingredient` — by git branch with **no rev pin**, and gitignores its
lockfile so CI re-resolves every run. Cubby's wasm cache key folds in the live upstream HEAD SHA;
that replaced an earlier rev pin which left stale builds outliving parser changes. Freshness was
chosen over insulation.

The manifests therefore say the opposite of the truth, which is the whole reason this is written
down. For `ingredient` ordinary semver caution would be enough; for the other three, nothing in
this repo suggests anyone is watching.

## Consequences

A breaking change to any of the four breaks cubby's CI on its next run, with no version gate to
absorb it. Narrowing is a two-repo change: prepare the cubby side first. Additive is safe and
preferred.

**A repo-local caller search is not evidence a `pub` item is unused.** Cubby is the only consumer
of much of the unit-conversion surface (`find_connected_components`, `make_graph`,
`convert_measure_with_graph_explained`, `ConversionStep`, `is_valid`,
`util::format_quantity_ascii`) and of `recipe-epub`'s entire non-`native` half.

The scope matters as much as the rule: `ingredient-corpus`, `food-cli` and `food-app` have **no**
external consumer, so for those a local search *is* evidence — which is what made it safe to
delete their dead exports.
