# ingredient-parser


[![crates.io](https://docs.rs/ingredient/badge.svg)](https://docs.rs/ingredient/latest/ingredient/)

This leverages [nom](https://github.com/Geal/nom) to parse ingredient line items from recipes into a common format.

# demo
[ingredient.nickysemenza.com](https://ingredient.nickysemenza.com)

As an example, `1¼  cups / 155.5 grams all-purpose flour, lightly sifted`  becomes
```rust
{
    name: "all-purpose flour",
    amounts: [
        Measure { unit: "cups", value: 1.25 },
        Measure { unit: "grams", value: 155.5 }
    ],
    modifier: Some("lightly sifted")
}
```
More examples listed in [the docs](https://docs.rs/ingredient/)
