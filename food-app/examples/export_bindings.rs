//! Regenerate or verify the frontend contract (`ui/src/api/generated.ts`).
fn main() -> Result<(), String> {
    let path = food_app::backend::bindings_path();
    let expected = food_app::backend::bindings_source();
    if std::env::args().any(|arg| arg == "--check") {
        let actual = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
        if actual != expected {
            return Err(
                "Frontend bindings are stale. Run cargo run -p food-app --example export_bindings"
                    .into(),
            );
        }
        Ok(())
    } else {
        std::fs::write(&path, expected).map_err(|error| error.to_string())
    }
}
