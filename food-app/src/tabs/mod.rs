mod cookbook;
mod corpus;
mod debug;
mod recipe;
mod test;

pub use cookbook::CookbookTab;
pub use corpus::{CorpusAction, CorpusTab};
pub use debug::show_debug_tab;
pub use recipe::{show_parsed, show_raw};
pub use test::TestTab;

use eframe::egui;

/// Arrow-key navigation for a selectable list: ArrowUp/ArrowDown move the
/// selection (ArrowDown with nothing selected picks the first row). Inactive
/// while any widget has focus, so arrows keep working inside text fields.
/// Returns `true` when the selection changed — callers should scroll the
/// selected row into view.
pub(crate) fn arrow_nav(ui: &egui::Ui, selected: &mut Option<usize>, len: usize) -> bool {
    if len == 0 || ui.ctx().memory(|m| m.focused().is_some()) {
        return false;
    }
    let (up, down) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::ArrowUp),
            i.key_pressed(egui::Key::ArrowDown),
        )
    });
    let next = match *selected {
        Some(i) if down && i + 1 < len => Some(i + 1),
        Some(i) if up && i > 0 => Some(i - 1),
        None if down => Some(0),
        _ => return false,
    };
    *selected = next;
    true
}
