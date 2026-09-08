# Cookbook label review

The four holdout books were labeled blind from `sources.jsonl`; parser output and parser implementation were not consulted.

- The sampler now excludes publisher-styled section headings before ranking. The removed Honey heading caused IDs 32–49 to shift and added a new ingredient at ID 50; all shifted labels were regenerated.
- `honey-co-14` contains a trailing backslash from source extraction. The exact `input` and the corresponding preparation modifier retain it.
- Parenthetical quantities were hoisted only when they measure the same ingredient. Per-item weights and source/yield quantities remain modifier text; pure page and note references were discarded.
- Unquantified alternatives remain opaque names. Alternatives with their own explicit quantity remain modifier text because the schema cannot associate separate quantities with separate ingredient names.
- Derived components use the underlying counted ingredient: `honey-co-01` records one whole lemon and keeps the authored derivation plus surplus wording in Modifier.
- `for the pan` maps to `pan_grease`. Serving and topping phrases remain ordinary modifiers unless they match a defined usage category.
