//! Session persistence: a small serde snapshot of the app's restorable state.
//!
//! Only plain inputs and view toggles are persisted — never the tab structs
//! themselves, which carry promises, traces, and other per-run state. Saved by
//! `eframe` (every ~30s and on exit) under [`eframe::APP_KEY`]; window geometry
//! and egui widget memory are persisted separately by eframe's defaults.

use crate::{MyApp, Tab};

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(crate) struct PersistedState {
    pub current_tab: Tab,
    pub url: String,
    pub test_input: String,
    pub cookbook_path: String,
    pub library_dir: Option<std::path::PathBuf>,
    pub cookbooks_only: bool,
    pub use_ai_fallback: bool,
    pub library_grid: bool,
}

impl Default for PersistedState {
    // Missing fields in an older snapshot fall back to the app's defaults, not
    // to zero values (an empty URL would otherwise clobber the sample recipe).
    fn default() -> Self {
        Self::capture(&MyApp::default())
    }
}

impl PersistedState {
    pub fn capture(app: &MyApp) -> Self {
        Self {
            current_tab: app.current_tab,
            url: app.url.clone(),
            test_input: app.test.input.clone(),
            cookbook_path: app.cookbook.path.clone(),
            library_dir: app.cookbook.library_dir.clone(),
            cookbooks_only: app.cookbook.cookbooks_only,
            use_ai_fallback: app.cookbook.use_ai_fallback,
            library_grid: app.cookbook.library_grid,
        }
    }

    /// Restore inputs and toggles onto a freshly-defaulted app. The cookbook
    /// path and library dir only prefill their controls — nothing auto-loads
    /// or auto-scans (a load runs LLM extraction). The recipe URL *does*
    /// re-fetch via the existing first-frame fetch on the Recipe/Debug tabs.
    pub fn apply_to(self, app: &mut MyApp) {
        app.current_tab = self.current_tab;
        app.url = self.url;
        app.test.input = self.test_input;
        app.cookbook.path = self.cookbook_path;
        app.cookbook.library_dir = self.library_dir;
        app.cookbook.cookbooks_only = self.cookbooks_only;
        app.cookbook.use_ai_fallback = self.use_ai_fallback;
        app.cookbook.library_grid = self.library_grid;
    }
}
