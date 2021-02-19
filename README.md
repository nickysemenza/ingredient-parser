# ingredient-parser

[![codecov](https://codecov.io/gh/nickysemenza/ingredient-parser/branch/main/graph/badge.svg?token=5GJCVD15RH)](https://codecov.io/gh/nickysemenza/ingredient-parser)
![build + test](https://github.com/nickysemenza/ingredient-parser/workflows/build%20+%20test/badge.svg)
[![crates.io](https://docs.rs/ingredient/badge.svg)](https://docs.rs/ingredient/latest/ingredient/)

This leverages [nom](https://github.com/Geal/nom) to parse ingredient line items from recipes into a common format.

*wip*


example: `1¼  cups / 155.5 grams flour, lightly sifted` => `{ name: "flour", amounts: [Amount { unit: "cups", value: 1.25 }, Amount { unit: "grams", value: 155.5 }], modifier: Some("lightly sifted") }`
