# ingredient-parser

This leverages [nom](https://github.com/Geal/nom) to parse ingredient line items from recipes into a common format.

*wip*


example: `1¼  cups / 155.5 grams flour, lightly sifted` => `{ name: "flour", amounts: [Amount { unit: "cups", value: 1.25 }, Amount { unit: "grams", value: 155.5 }], modifier: Some("lightly sifted") }`