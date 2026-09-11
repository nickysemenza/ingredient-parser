//! Desktop command services and the macOS Tauri application shell.
pub mod backend;

#[cfg(target_os = "macos")]
mod desktop;
#[cfg(target_os = "macos")]
pub use desktop::run;
