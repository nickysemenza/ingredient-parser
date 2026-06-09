#![warn(clippy::all, rust_2018_idioms)]
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use food_app::MyApp;

fn main() -> eframe::Result<()> {
    // Load AI gateway creds (AI_GATEWAY_API_KEY, CLOUDFLARE_AI_GATEWAY_BASE_URL)
    // from a repo-root .env. Missing file is fine; real exported vars take precedence.
    let _ = dotenvy::dotenv();
    tracing_subscriber::fmt::init();

    let native_options = eframe::NativeOptions::default();
    eframe::run_native(
        "ingredient-parser",
        native_options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::new(MyApp::new(cc)))
        }),
    )
}
