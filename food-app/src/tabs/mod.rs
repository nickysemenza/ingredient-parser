mod debug;
mod recipe;
mod test;

pub use debug::show_debug_tab;
pub use recipe::{show_parsed, show_raw};
pub use test::show_test_tab;

// Cookbook (EPUB) tab — native only (recipe-epub pulls file/network/tokio deps).
#[cfg(not(target_arch = "wasm32"))]
mod cookbook;
#[cfg(not(target_arch = "wasm32"))]
pub use cookbook::CookbookTab;
