//! Parsing measurements embedded in recipe prose (instructions), and
//! highlighting known ingredient names.
//!
//! Run with: `cargo run -p ingredient --example rich_text`

use ingredient::rich_text::{Chunk, RichParser};

fn main() {
    // Pass any iterable of string-likes — no `.to_string()` needed.
    let parser = RichParser::new(["flour", "butter"]);

    let text = "Cream 2 sticks butter, then beat in 1 1/2 cups flour for 3 minutes.";
    let Ok(chunks) = parser.parse(text) else {
        eprintln!("failed to parse: {text:?}");
        return;
    };

    // Reconstruct the line, tagging each recognized piece.
    for chunk in chunks {
        match chunk {
            Chunk::Text(t) => print!("{t}"),
            Chunk::Measure(measures) => {
                let rendered: Vec<String> = measures.iter().map(ToString::to_string).collect();
                print!("[{}]", rendered.join(", "));
            }
            Chunk::Ing(name) => print!("<{name}>"),
        }
    }
    println!();
}
