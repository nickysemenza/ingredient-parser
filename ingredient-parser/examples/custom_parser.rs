//! Customizing the parser with extra units and adjectives, or restricting it to
//! a specific set.
//!
//! Run with: `cargo run -p ingredient --example custom_parser`

use ingredient::IngredientParser;

fn main() {
    // Add domain-specific units and preparation modifiers.
    let parser = IngredientParser::new()
        .with_units(&["sprig", "sprigs", "knob", "knobs"])
        .with_adjectives(&["roughly chopped"]);

    println!("{}", parser.from_str("3 sprigs thyme, roughly chopped"));
    println!("{}", parser.from_str("1 knob butter"));

    // Or recognize ONLY a custom set: clear the built-in custom units (clove,
    // packet, bunch, ...) first, then add just what you want. Built-in units
    // like cup/gram/tbsp are always recognized.
    let strict = IngredientParser::new()
        .clear_units()
        .with_units(&["clove", "cloves"]);
    println!("{}", strict.from_str("2 cloves garlic")); // clove still recognized
    println!("{}", strict.from_str("1 packet yeast")); // "packet" stays in the name
}
