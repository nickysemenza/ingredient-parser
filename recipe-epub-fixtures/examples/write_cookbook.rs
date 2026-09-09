//! Write the same fixture used by downstream tests for manual reader/UI checks.
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args_os()
        .nth(1)
        .ok_or("provide an output EPUB path")?;
    std::fs::write(path, recipe_epub_fixtures::cookbook_epub()?)?;
    Ok(())
}
