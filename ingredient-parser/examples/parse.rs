//! Basic ingredient parsing.
//!
//! Run with: `cargo run -p ingredient --example parse`

use ingredient::from_str;

fn main() {
    let lines = [
        "2 cups all-purpose flour, sifted",
        "1¼ cups / 155.5g sugar",
        "Juice of 1 lemon",
        "3 large eggs",
        "1 cup packed brown sugar",
        "salt to taste",
        "(½ cup chopped walnuts)",
    ];

    for line in lines {
        let ing = from_str(line);
        let amounts: Vec<String> = ing.amounts.iter().map(ToString::to_string).collect();
        println!("{line:?}");
        println!("  name     = {}", ing.name);
        println!("  amounts  = {amounts:?}");
        println!("  modifier = {:?}", ing.modifier);
        println!("  optional = {}", ing.optional);
        println!();
    }
}
