//! Catppuccin theme (Mocha dark / Latte light) for the food-app egui UI.
//!
//! egui ships stock defaults and has no built-in system-font support. This
//! module applies a cohesive palette, loads a macOS system font at runtime
//! (nothing is bundled into the binary — the bytes are read from the OS),
//! tunes spacing, and exposes the active palette via [`palette()`] so render
//! code stays on-palette. Fonts/spacing are applied once in `MyApp::new`
//! ([`apply`]); the flavor can be switched at runtime ([`set_theme`]).

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Stroke};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// One Catppuccin flavor's colors. Semantic accessors (`amount()`, `name()`,
/// …) map the app's render roles onto the flavor so call sites don't pick raw
/// colors themselves.
pub struct Palette {
    base: Color32,
    mantle: Color32,
    crust: Color32,
    surface0: Color32,
    surface1: Color32,
    surface2: Color32,
    overlay1: Color32,
    subtext0: Color32,
    text: Color32,
    blue: Color32,
    lavender: Color32,
    green: Color32,
    yellow: Color32,
    peach: Color32,
    red: Color32,
    /// Whether this flavor builds on egui's dark base visuals.
    dark: bool,
}

impl Palette {
    /// Ingredient amount text.
    pub fn amount(&self) -> Color32 {
        self.peach
    }
    /// Ingredient name text.
    pub fn name(&self) -> Color32 {
        self.blue
    }
    /// Ingredient modifier / secondary metadata text.
    pub fn modifier(&self) -> Color32 {
        self.subtext0
    }
    /// Trace node: a parser that matched fully.
    pub fn trace_ok(&self) -> Color32 {
        self.green
    }
    /// Trace node: a parser that failed.
    pub fn trace_fail(&self) -> Color32 {
        self.red
    }
    /// Trace node: matched but left unconsumed input.
    pub fn trace_incomplete(&self) -> Color32 {
        self.yellow
    }
    /// Recipe-reference graph: hub node accent.
    pub fn graph_node(&self) -> Color32 {
        self.blue
    }
}

// RGB of the published hex values: https://github.com/catppuccin/catppuccin
const MOCHA: Palette = Palette {
    base: Color32::from_rgb(30, 30, 46),        // #1e1e2e
    mantle: Color32::from_rgb(24, 24, 37),      // #181825
    crust: Color32::from_rgb(17, 17, 27),       // #11111b
    surface0: Color32::from_rgb(49, 50, 68),    // #313244
    surface1: Color32::from_rgb(69, 71, 90),    // #45475a
    surface2: Color32::from_rgb(88, 91, 112),   // #585b70
    overlay1: Color32::from_rgb(127, 132, 156), // #7f849c
    subtext0: Color32::from_rgb(166, 173, 200), // #a6adc8
    text: Color32::from_rgb(205, 214, 244),     // #cdd6f4
    blue: Color32::from_rgb(137, 180, 250),     // #89b4fa
    lavender: Color32::from_rgb(180, 190, 254), // #b4befe
    green: Color32::from_rgb(166, 227, 161),    // #a6e3a1
    yellow: Color32::from_rgb(249, 226, 175),   // #f9e2af
    peach: Color32::from_rgb(250, 179, 135),    // #fab387
    red: Color32::from_rgb(243, 139, 168),      // #f38ba8
    dark: true,
};

const LATTE: Palette = Palette {
    base: Color32::from_rgb(239, 241, 245),     // #eff1f5
    mantle: Color32::from_rgb(230, 233, 239),   // #e6e9ef
    crust: Color32::from_rgb(220, 224, 232),    // #dce0e8
    surface0: Color32::from_rgb(204, 208, 218), // #ccd0da
    surface1: Color32::from_rgb(188, 192, 204), // #bcc0cc
    surface2: Color32::from_rgb(172, 176, 190), // #acb0be
    overlay1: Color32::from_rgb(140, 143, 161), // #8c8fa1
    subtext0: Color32::from_rgb(108, 111, 133), // #6c6f85
    text: Color32::from_rgb(76, 79, 105),       // #4c4f69
    blue: Color32::from_rgb(30, 102, 245),      // #1e66f5
    lavender: Color32::from_rgb(114, 135, 253), // #7287fd
    green: Color32::from_rgb(64, 160, 43),      // #40a02b
    yellow: Color32::from_rgb(223, 142, 29),    // #df8e1d
    peach: Color32::from_rgb(254, 100, 11),     // #fe640b
    red: Color32::from_rgb(210, 15, 57),        // #d20f39
    dark: false,
};

/// Which Catppuccin flavor is active. Persisted across sessions.
#[derive(Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum ThemeChoice {
    #[default]
    Mocha,
    Latte,
}

/// Active-flavor flag backing [`palette()`]. An atomic (not ctx data) so the
/// palette is reachable from plain render helpers without threading a context.
static IS_LATTE: AtomicBool = AtomicBool::new(false);

/// The active flavor's palette. Branchy const refs — no locks, no per-frame cost.
pub fn palette() -> &'static Palette {
    if IS_LATTE.load(Ordering::Relaxed) {
        &LATTE
    } else {
        &MOCHA
    }
}

/// Switch the active flavor and rebuild egui's visuals from it. Cheap enough
/// to call from a toggle button.
pub fn set_theme(ctx: &egui::Context, choice: ThemeChoice) {
    IS_LATTE.store(choice == ThemeChoice::Latte, Ordering::Relaxed);
    ctx.set_visuals(visuals(palette()));
}

/// Apply the theme (palette, system font, spacing). Call once at startup;
/// flavor switches afterwards go through [`set_theme`].
pub fn apply(ctx: &egui::Context, choice: ThemeChoice) {
    set_theme(ctx, choice);
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

fn visuals(p: &Palette) -> egui::Visuals {
    let mut v = if p.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    let radius = CornerRadius::same(6);

    v.panel_fill = p.base;
    v.window_fill = p.mantle;
    v.extreme_bg_color = p.crust;
    v.faint_bg_color = p.surface0;
    v.override_text_color = Some(p.text);
    v.hyperlink_color = p.lavender;
    v.error_fg_color = p.red;
    v.window_corner_radius = radius;
    v.selection.bg_fill = Color32::from_rgba_unmultiplied(p.blue.r(), p.blue.g(), p.blue.b(), 90);
    v.selection.stroke = Stroke::new(1.0, p.lavender);

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.base;
    w.noninteractive.weak_bg_fill = p.base;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.surface0);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.subtext0);
    w.noninteractive.corner_radius = radius;

    w.inactive.bg_fill = p.surface0;
    w.inactive.weak_bg_fill = p.surface0;
    w.inactive.bg_stroke = Stroke::new(1.0, p.surface1);
    w.hovered.bg_fill = p.surface1;
    w.hovered.weak_bg_fill = p.surface1;
    w.hovered.bg_stroke = Stroke::new(1.0, p.overlay1);
    w.active.bg_fill = p.surface2;
    w.active.weak_bg_fill = p.surface2;
    w.active.bg_stroke = Stroke::new(1.0, p.lavender);
    w.open.bg_fill = p.surface1;
    w.open.weak_bg_fill = p.surface1;
    w.open.bg_stroke = Stroke::new(1.0, p.overlay1);

    for wv in [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
        wv.corner_radius = radius;
        wv.fg_stroke = Stroke::new(1.0, p.text);
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
    let p = palette();
    egui::Frame::group(ui.style())
        .fill(p.surface0)
        .stroke(Stroke::new(1.0, p.surface1))
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
    pub const CORPUS: &str = icons::ICON_FACT_CHECK.codepoint;
    pub const YIELD: &str = icons::ICON_SCALE.codepoint;
    pub const SERVINGS: &str = icons::ICON_RESTAURANT.codepoint;
    pub const TIME: &str = icons::ICON_SCHEDULE.codepoint;
    pub const EQUIPMENT: &str = icons::ICON_HANDYMAN.codepoint;
    pub const NOTE: &str = icons::ICON_STICKY_NOTE_2.codepoint;
    pub const OPEN: &str = icons::ICON_OPEN_IN_NEW.codepoint;
    pub const DARK_MODE: &str = icons::ICON_DARK_MODE.codepoint;
    pub const LIGHT_MODE: &str = icons::ICON_LIGHT_MODE.codepoint;
}
