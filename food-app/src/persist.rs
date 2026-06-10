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
    pub theme: crate::theme::ThemeChoice,
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
            theme: app.theme,
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
        app.theme = self.theme;
        app.url = self.url;
        app.test.input = self.test_input;
        app.cookbook.path = self.cookbook_path;
        app.cookbook.library_dir = self.library_dir;
        app.cookbook.cookbooks_only = self.cookbooks_only;
        app.cookbook.use_ai_fallback = self.use_ai_fallback;
        app.cookbook.library_grid = self.library_grid;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// capture → ron → deserialize → apply_to round-trips every persisted
    /// field. ron because that's what eframe's native storage writes.
    #[test]
    fn round_trip_through_ron() {
        let mut app = MyApp {
            current_tab: Tab::Test,
            theme: crate::theme::ThemeChoice::Latte,
            url: "https://example.com/recipe".to_string(),
            ..Default::default()
        };
        app.test.input = "2 cups flour\n1 egg".to_string();
        app.cookbook.path = "/books/pok-pok.epub".to_string();
        app.cookbook.library_dir = Some(std::path::PathBuf::from("/books"));
        app.cookbook.cookbooks_only = false;
        app.cookbook.use_ai_fallback = true;
        app.cookbook.library_grid = false;

        let ron = ron::to_string(&PersistedState::capture(&app)).unwrap();
        let restored: PersistedState = ron::from_str(&ron).unwrap();
        let mut fresh = MyApp::default();
        restored.apply_to(&mut fresh);

        assert!(fresh.current_tab == Tab::Test);
        assert!(fresh.theme == crate::theme::ThemeChoice::Latte);
        assert_eq!(fresh.url, app.url);
        assert_eq!(fresh.test.input, app.test.input);
        assert_eq!(fresh.cookbook.path, app.cookbook.path);
        assert_eq!(fresh.cookbook.library_dir, app.cookbook.library_dir);
        assert!(!fresh.cookbook.cookbooks_only);
        assert!(fresh.cookbook.use_ai_fallback);
        assert!(!fresh.cookbook.library_grid);
    }

    /// An older/empty snapshot must fall back to the app's defaults (per-field
    /// `#[serde(default)]`), not zero values — an empty URL would otherwise
    /// clobber the sample recipe.
    #[test]
    fn missing_fields_fall_back_to_app_defaults() {
        let restored: PersistedState = ron::from_str("()").unwrap();
        let defaults = MyApp::default();
        assert_eq!(restored.url, defaults.url);
        assert_eq!(restored.test_input, defaults.test.input);
        assert!(restored.cookbooks_only == defaults.cookbook.cookbooks_only);
    }
}
