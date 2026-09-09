fn main() {
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();
    #[cfg(target_os = "macos")]
    if let Err(error) = food_app::run() {
        eprintln!("Could not start Ingredient Parser: {error}");
        std::process::exit(1);
    }
    #[cfg(not(target_os = "macos"))]
    {
        eprintln!(
            "The desktop application currently supports macOS. Use food-cli on this platform."
        );
        std::process::exit(1);
    }
}
