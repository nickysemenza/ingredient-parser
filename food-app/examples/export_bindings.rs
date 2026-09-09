//! Regenerate or verify the frontend contract from application-owned Rust DTOs.
fn main() -> Result<(), String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/src/generated.ts");
    if std::env::args().any(|arg| arg == "--check") {
        let actual = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
        if actual != food_app::backend::bindings_source() {
            return Err(
                "Frontend bindings are stale. Run cargo run -p food-app --example export_bindings"
                    .into(),
            );
        }
        Ok(())
    } else {
        food_app::backend::export_bindings(&path)
    }
}
