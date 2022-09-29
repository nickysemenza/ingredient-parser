#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

fn main() {
    let options = eframe::NativeOptions {
        // initial_window_size: Some([1280.0, 1024.0].into()),
        ..Default::default()
    };
    eframe::run_native(
        "Download and show an image with eframe/egui",
        options,
        Box::new(|_cc| Box::new(food_ui::MyApp::default())),
    );
}
