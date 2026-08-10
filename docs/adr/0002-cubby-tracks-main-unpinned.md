# Cubby tracks `main` unpinned, so the public interface is a two-repo contract

Cubby's `recipebridge` crate depends on `ingredient`, `recipe-scraper` and `recipe-epub` by git
branch with no rev pin, and gitignores its lockfile so CI re-resolves every run. This is
deliberate: cubby's wasm cache key folds in the live upstream HEAD SHA, which replaced an
earlier rev pin that left stale builds outliving parser changes. The trade-off accepted is
freshness over insulation — cubby is never behind, and in exchange there is no version gate
between the two repos.

## Consequences

Any breaking change to the public interface of `ingredient`, `recipe-scraper` or the non-`native`
subset of `recipe-epub` breaks cubby's CI on its next run. There is no semver step to absorb it,
so a narrowing change is a two-repo change: prepare the cubby side first, then merge here.

Additive changes are safe and are the preferred shape for anything cubby will consume.

A repo-local search is not evidence that a public item is unused — cubby is the second adapter
for much of the unit-conversion surface (`find_connected_components`, `make_graph`,
`convert_measure_with_graph_explained`, `ConversionStep`, `is_valid`, `util::format_quantity_ascii`).
