#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

fn main() -> eframe::Result<()> {
    tracing_subscriber::fmt::init();
    let options = eframe::NativeOptions {
        // initial_window_size: Some([1280.0, 1024.0].into()),
        ..Default::default()
    };
    eframe::run_native(
        "recipe viewer",
        options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::<food_ui::MyApp>::default())
        }),
    )
}
