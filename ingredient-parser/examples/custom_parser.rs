//! Custom parser configuration — extend the default unit vocabulary.
//!
//! Run with: `cargo run -p ingredient --example custom_parser`

use ingredient::IngredientParser;

fn main() {
    let parser = IngredientParser::new().with_units(&["handful", "handfuls"]);

    let ing = parser.from_str("2 handfuls nuts");
    println!("name     = {}", ing.name);
    println!("amounts  = {:?}", ing.amounts);
    println!("modifier = {:?}", ing.modifier);
}
