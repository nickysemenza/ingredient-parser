//! Write one fixture to disk for manual reader/UI checks.
//!
//! ```sh
//! cargo run -p cookbook-fixtures --example write_fixture -- split_spine /tmp/split.epub
//! ```

const NAMES: [&str; 4] = [
    "cookbook",
    "split_spine",
    "epub3_nav_pagebreaks",
    "typographic_variations",
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let name = args
        .next()
        .ok_or_else(|| format!("provide a fixture name: one of {}", NAMES.join(", ")))?;
    let path = args.next().ok_or("provide an output EPUB path")?;
    let name = name.to_string_lossy().replace('-', "_");
    let bytes = match name.as_str() {
        "cookbook" | "cookbook_epub" => cookbook_fixtures::cookbook_epub()?,
        "split_spine" => cookbook_fixtures::split_spine()?,
        "epub3_nav_pagebreaks" => cookbook_fixtures::epub3_nav_pagebreaks()?,
        "typographic_variations" => cookbook_fixtures::typographic_variations()?,
        other => {
            return Err(
                format!("unknown fixture {other:?}; try one of {}", NAMES.join(", ")).into(),
            );
        }
    };
    std::fs::write(&path, bytes)?;
    println!("wrote {} to {}", name, path.to_string_lossy());
    Ok(())
}
