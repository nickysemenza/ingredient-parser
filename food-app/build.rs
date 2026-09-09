fn main() {
    #[cfg(target_os = "macos")]
    tauri_build::build();
}
