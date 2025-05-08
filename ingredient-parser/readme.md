# ingredient-parser

[![crates.io](https://docs.rs/ingredient/badge.svg)](https://docs.rs/ingredient/latest/ingredient/)

**ingredient-parser** is a Rust library that uses [nom](https://github.com/Geal/nom) to parse ingredient lines from recipes into a structured, machine-readable format.

---

## Features

- Parses complex ingredient lines into structured data
- Supports multiple units and values per ingredient
- Extracts ingredient names and modifiers (e.g., "sifted", "chopped")
- Handles common recipe notation and edge cases

---

## Example

Given the input:

```
1¼ cups / 155.5 grams all-purpose flour, lightly sifted
```

The parser produces:

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

See more examples in the [documentation](https://docs.rs/ingredient/).

---

## Demo

Try it live: [ingredient.nickysemenza.com](https://ingredient.nickysemenza.com)

---


## Documentation

- [API Docs on docs.rs](https://docs.rs/ingredient/)

---

## Contributing

Contributions, issues, and feature requests are welcome! Please open an issue or pull request.

---

## License

MIT
