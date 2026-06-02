//! Catppuccin Mocha theme for the food-app egui UI.
//!
//! egui ships stock defaults and has no built-in system-font support. This
//! module applies a cohesive dark palette, loads a macOS system font at
//! runtime (nothing is bundled into the binary — the bytes are read from the
//! OS), tunes spacing, and exposes the app's accent text colors as named
//! constants so render code stays on-palette. Applied once in `MyApp::new`.

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Stroke};
use std::sync::Arc;

// --- Catppuccin Mocha palette ---------------------------------------------
// RGB of the published hex values: https://github.com/catppuccin/catppuccin
const BASE: Color32 = Color32::from_rgb(30, 30, 46); // #1e1e2e
const MANTLE: Color32 = Color32::from_rgb(24, 24, 37); // #181825
const CRUST: Color32 = Color32::from_rgb(17, 17, 27); // #11111b
const SURFACE0: Color32 = Color32::from_rgb(49, 50, 68); // #313244
const SURFACE1: Color32 = Color32::from_rgb(69, 71, 90); // #45475a
const SURFACE2: Color32 = Color32::from_rgb(88, 91, 112); // #585b70
const OVERLAY1: Color32 = Color32::from_rgb(127, 132, 156); // #7f849c
const SUBTEXT0: Color32 = Color32::from_rgb(166, 173, 200); // #a6adc8
const TEXT: Color32 = Color32::from_rgb(205, 214, 244); // #cdd6f4
const BLUE: Color32 = Color32::from_rgb(137, 180, 250); // #89b4fa
const LAVENDER: Color32 = Color32::from_rgb(180, 190, 254); // #b4befe
const GREEN: Color32 = Color32::from_rgb(166, 227, 161); // #a6e3a1
const YELLOW: Color32 = Color32::from_rgb(249, 226, 175); // #f9e2af
const PEACH: Color32 = Color32::from_rgb(250, 179, 135); // #fab387
const RED: Color32 = Color32::from_rgb(243, 139, 168); // #f38ba8

// --- Public accent colors used by the tabs --------------------------------
/// Ingredient amount text.
pub const AMOUNT: Color32 = PEACH;
/// Ingredient name text.
pub const NAME: Color32 = BLUE;
/// Ingredient modifier / secondary metadata text.
pub const MODIFIER: Color32 = SUBTEXT0;
/// Trace node: a parser that matched fully.
pub const TRACE_OK: Color32 = GREEN;
/// Trace node: a parser that failed.
pub const TRACE_FAIL: Color32 = RED;
/// Trace node: matched but left unconsumed input.
pub const TRACE_INCOMPLETE: Color32 = YELLOW;
/// Recipe-reference graph: hub node accent.
pub const GRAPH_NODE: Color32 = BLUE;

/// Apply the theme (palette, system font, spacing). Call once at startup.
pub fn apply(ctx: &egui::Context) {
    ctx.set_visuals(visuals());
    if let Some(fonts) = system_fonts() {
        ctx.set_fonts(fonts);
    }
    // Registers the Material Symbols font as a fallback in the proportional
    // family, so `icon::*` codepoints render inline in normal text labels.
    egui_material_icons::initialize(ctx);
    ctx.global_style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
    });
}

fn visuals() -> egui::Visuals {
    let mut v = egui::Visuals::dark();
    let radius = CornerRadius::same(6);

    v.panel_fill = BASE;
    v.window_fill = MANTLE;
    v.extreme_bg_color = CRUST;
    v.faint_bg_color = SURFACE0;
    v.override_text_color = Some(TEXT);
    v.hyperlink_color = LAVENDER;
    v.error_fg_color = RED;
    v.window_corner_radius = radius;
    v.selection.bg_fill = Color32::from_rgba_unmultiplied(137, 180, 250, 90);
    v.selection.stroke = Stroke::new(1.0, LAVENDER);

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = BASE;
    w.noninteractive.weak_bg_fill = BASE;
    w.noninteractive.bg_stroke = Stroke::new(1.0, SURFACE0);
    w.noninteractive.fg_stroke = Stroke::new(1.0, SUBTEXT0);
    w.noninteractive.corner_radius = radius;

    w.inactive.bg_fill = SURFACE0;
    w.inactive.weak_bg_fill = SURFACE0;
    w.inactive.bg_stroke = Stroke::new(1.0, SURFACE1);
    w.hovered.bg_fill = SURFACE1;
    w.hovered.weak_bg_fill = SURFACE1;
    w.hovered.bg_stroke = Stroke::new(1.0, OVERLAY1);
    w.active.bg_fill = SURFACE2;
    w.active.weak_bg_fill = SURFACE2;
    w.active.bg_stroke = Stroke::new(1.0, LAVENDER);
    w.open.bg_fill = SURFACE1;
    w.open.weak_bg_fill = SURFACE1;
    w.open.bg_stroke = Stroke::new(1.0, OVERLAY1);

    for wv in [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        wv.corner_radius = radius;
        wv.fg_stroke = Stroke::new(1.0, TEXT);
    }

    v
}

/// Build a `FontDefinitions` that prefers a macOS system font, falling back to
/// egui's embedded fonts. Returns `None` (keep egui defaults) when no system
/// font is readable — e.g. on non-macOS hosts or the wasm build, where these
/// paths don't exist.
fn system_fonts() -> Option<FontDefinitions> {
    // (path, ttc face index). SFNS.ttf is a single-face variable font; the
    // .ttc fallbacks are collections, so they need an explicit index.
    const UI_CANDIDATES: &[(&str, u32)] = &[
        ("/System/Library/Fonts/SFNS.ttf", 0),
        ("/System/Library/Fonts/HelveticaNeue.ttc", 0),
        ("/System/Library/Fonts/Supplemental/Arial.ttf", 0),
    ];
    const MONO_CANDIDATES: &[(&str, u32)] = &[
        ("/System/Library/Fonts/SFNSMono.ttf", 0),
        ("/System/Library/Fonts/Menlo.ttc", 0),
    ];

    let ui = load_first(UI_CANDIDATES);
    let mono = load_first(MONO_CANDIDATES);
    if ui.is_none() && mono.is_none() {
        return None;
    }

    let mut fonts = FontDefinitions::default();
    if let Some(data) = ui {
        fonts
            .font_data
            .insert("system-ui".to_owned(), Arc::new(data));
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "system-ui".to_owned());
    }
    if let Some(data) = mono {
        fonts
            .font_data
            .insert("system-mono".to_owned(), Arc::new(data));
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, "system-mono".to_owned());
    }
    Some(fonts)
}

/// Read the first candidate font that exists on disk.
fn load_first(candidates: &[(&str, u32)]) -> Option<FontData> {
    candidates.iter().find_map(|(path, index)| {
        std::fs::read(path).ok().map(|bytes| FontData {
            index: *index,
            ..FontData::from_owned(bytes)
        })
    })
}

/// Wrap content in a rounded, subtly-filled card so panels read as deliberate
/// surfaces rather than bare labels. Roomy padding suits multi-line content
/// like instruction steps.
pub fn card<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    card_with(ui, egui::Margin::same(8), add_contents)
}

/// A tighter [`card`] for single-line content (e.g. ingredient rows), so the
/// surface hugs the text instead of reading as an oversized pill.
pub fn card_compact<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    card_with(ui, egui::Margin::symmetric(8, 2), add_contents)
}

fn card_with<R>(
    ui: &mut egui::Ui,
    inner_margin: egui::Margin,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    egui::Frame::group(ui.style())
        .fill(SURFACE0)
        .stroke(Stroke::new(1.0, SURFACE1))
        .corner_radius(CornerRadius::same(6))
        .inner_margin(inner_margin)
        .show(ui, add_contents)
        .inner
}

/// Material Symbols codepoints for the app's affordances. `apply()` registers
/// the icon font as a proportional-family fallback, so these render inline in
/// normal text labels (e.g. `format!("{} Recipe", icon::RECIPE)`).
pub mod icon {
    use egui_material_icons::icons;

    pub const TEST: &str = icons::ICON_SCIENCE.codepoint;
    pub const RECIPE: &str = icons::ICON_MENU_BOOK.codepoint;
    pub const DEBUG: &str = icons::ICON_SEARCH.codepoint;
    pub const COOKBOOK: &str = icons::ICON_AUTO_STORIES.codepoint;
    pub const YIELD: &str = icons::ICON_SCALE.codepoint;
    pub const SERVINGS: &str = icons::ICON_RESTAURANT.codepoint;
    pub const TIME: &str = icons::ICON_SCHEDULE.codepoint;
    pub const EQUIPMENT: &str = icons::ICON_HANDYMAN.codepoint;
    pub const NOTE: &str = icons::ICON_STICKY_NOTE_2.codepoint;
    pub const OPEN: &str = icons::ICON_OPEN_IN_NEW.codepoint;
}
