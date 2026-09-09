//! Neutral light/dark native-tool theme for the maintainer workspaces.
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

/// One appearance's colors. Semantic accessors (`amount()`, `name()`,
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

const MOCHA: Palette = Palette {
    base: Color32::from_rgb(30, 31, 34),
    mantle: Color32::from_rgb(37, 38, 41),
    crust: Color32::from_rgb(24, 25, 28),
    surface0: Color32::from_rgb(42, 43, 47),
    surface1: Color32::from_rgb(60, 62, 67),
    surface2: Color32::from_rgb(74, 77, 83),
    overlay1: Color32::from_rgb(148, 153, 163),
    subtext0: Color32::from_rgb(173, 177, 187),
    text: Color32::from_rgb(235, 237, 241),
    blue: Color32::from_rgb(112, 177, 255),
    lavender: Color32::from_rgb(143, 188, 255),
    green: Color32::from_rgb(115, 207, 151),
    yellow: Color32::from_rgb(237, 198, 99),
    peach: Color32::from_rgb(225, 179, 130),
    red: Color32::from_rgb(255, 138, 138),
    dark: true,
};

const LATTE: Palette = Palette {
    base: Color32::from_rgb(249, 249, 251),
    mantle: Color32::from_rgb(239, 240, 243),
    crust: Color32::from_rgb(255, 255, 255),
    surface0: Color32::from_rgb(236, 238, 242),
    surface1: Color32::from_rgb(212, 215, 222),
    surface2: Color32::from_rgb(193, 200, 211),
    overlay1: Color32::from_rgb(102, 109, 123),
    subtext0: Color32::from_rgb(92, 99, 113),
    text: Color32::from_rgb(35, 39, 47),
    blue: Color32::from_rgb(25, 91, 173),
    lavender: Color32::from_rgb(34, 92, 173),
    green: Color32::from_rgb(27, 115, 65),
    yellow: Color32::from_rgb(130, 91, 12),
    peach: Color32::from_rgb(139, 83, 33),
    red: Color32::from_rgb(181, 42, 47),
    dark: false,
};

/// Appearance choice. Existing serialized names remain readable.
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
        style.spacing.item_spacing = egui::vec2(8.0, 5.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
        style.spacing.interact_size.y = 28.0;
        style
            .text_styles
            .insert(egui::TextStyle::Small, egui::FontId::proportional(12.0));
        style
            .text_styles
            .insert(egui::TextStyle::Body, egui::FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Button, egui::FontId::proportional(14.0));
        style
            .text_styles
            .insert(egui::TextStyle::Heading, egui::FontId::proportional(22.0));
        style
            .text_styles
            .insert(egui::TextStyle::Monospace, egui::FontId::monospace(13.0));
    });
}

fn visuals(p: &Palette) -> egui::Visuals {
    let mut v = if p.dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    let radius = CornerRadius::same(4);

    v.panel_fill = p.base;
    v.window_fill = p.mantle;
    v.extreme_bg_color = p.crust;
    v.faint_bg_color = p.surface0;
    v.override_text_color = Some(p.text);
    v.hyperlink_color = p.lavender;
    v.error_fg_color = p.red;
    v.window_corner_radius = radius;
    v.selection.bg_fill = Color32::from_rgba_unmultiplied(p.blue.r(), p.blue.g(), p.blue.b(), 28);
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

fn card_with<R>(
    ui: &mut egui::Ui,
    inner_margin: egui::Margin,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> R {
    let p = palette();
    egui::Frame::group(ui.style())
        .fill(p.surface0)
        .stroke(Stroke::new(1.0, p.surface1))
        .corner_radius(CornerRadius::same(3))
        .inner_margin(inner_margin)
        .show(ui, add_contents)
        .inner
}

/// Inset structural panes distinguish navigation and inspection from evidence.
pub fn workspace_frame() -> egui::Frame {
    egui::Frame::new().fill(palette().base).inner_margin(16)
}

pub fn sidebar_frame() -> egui::Frame {
    egui::Frame::new().fill(palette().mantle).inner_margin(12)
}

pub fn evidence_frame() -> egui::Frame {
    egui::Frame::new().fill(palette().crust).inner_margin(16)
}

/// Retain the user's comparison split as the window or inspector changes width.
pub fn comparison_panel(ui: &egui::Ui) -> egui::Panel {
    let id = egui::Id::new("review-comparison-source");
    let width = ui.available_width();
    let previous = ui.data_mut(|data| data.get_persisted::<f32>(id.with("available")));
    if let Some(previous) = previous.filter(|previous| *previous > 0.0)
        && (previous - width).abs() > 1.0
        && let Some(mut state) = egui::containers::panel::PanelState::load(ui.ctx(), id)
    {
        let fraction = (state.size().x / previous).clamp(0.3, 0.65);
        state.outer_rect.max.x = state.outer_rect.min.x + width * fraction;
        ui.data_mut(|data| data.insert_persisted(id, state));
    }
    ui.data_mut(|data| data.insert_persisted(id.with("available"), width));
    egui::Panel::left(id)
        .default_size(width * 0.45)
        .size_range((width * 0.3)..=(width * 0.65))
}

pub fn pane_heading(ui: &mut egui::Ui, title: &str) {
    ui.label(egui::RichText::new(title).size(17.0).strong());
    ui.add_space(10.0);
}

pub fn primary_button(ui: &mut egui::Ui, title: &str) -> egui::Response {
    let p = palette();
    let ink = if p.dark { p.crust } else { Color32::WHITE };
    ui.add(
        egui::Button::new(egui::RichText::new(title).color(ink).strong())
            .fill(p.blue)
            .min_size(egui::vec2(0.0, 32.0)),
    )
}

/// Material Symbols codepoints for the app's affordances. `apply()` registers
/// the icon font as a proportional-family fallback, so these render inline in
/// normal text labels (e.g. `format!("{} Recipe", icon::RECIPE)`).
pub mod icon {
    use egui_material_icons::icons;

    pub const CLOSE: &str = icons::ICON_CLOSE.codepoint;
    pub const TEST: &str = icons::ICON_SCIENCE.codepoint;
    pub const COOKBOOK: &str = icons::ICON_AUTO_STORIES.codepoint;
    pub const YIELD: &str = icons::ICON_SCALE.codepoint;
    pub const SERVINGS: &str = icons::ICON_RESTAURANT.codepoint;
    pub const TIME: &str = icons::ICON_SCHEDULE.codepoint;
    pub const EQUIPMENT: &str = icons::ICON_HANDYMAN.codepoint;
    pub const NOTE: &str = icons::ICON_STICKY_NOTE_2.codepoint;
    pub const OPEN: &str = icons::ICON_OPEN_IN_NEW.codepoint;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(rgb: [f64; 3]) -> f64 {
        rgb.into_iter()
            .zip([0.2126, 0.7152, 0.0722])
            .map(|(c, weight)| {
                let c = c / 255.0;
                (if c <= 0.04045 {
                    c / 12.92
                } else {
                    ((c + 0.055) / 1.055).powf(2.4)
                }) * weight
            })
            .sum()
    }

    #[test]
    fn semantic_text_remains_readable_on_selected_rows() {
        for p in [&MOCHA, &LATTE] {
            let selection = visuals(p).selection.bg_fill;
            let alpha = f64::from(selection.a()) / 255.0;
            let foreground = [p.blue.r(), p.blue.g(), p.blue.b()];
            let background = [p.base.r(), p.base.g(), p.base.b()];
            let composite = std::array::from_fn(|i| {
                f64::from(foreground[i]) * alpha + f64::from(background[i]) * (1.0 - alpha)
            });
            let bg = luminance(composite);
            for color in [
                p.text, p.blue, p.green, p.peach, p.yellow, p.red, p.subtext0,
            ] {
                let fg = luminance([color.r(), color.g(), color.b()].map(f64::from));
                let contrast = (fg.max(bg) + 0.05) / (fg.min(bg) + 0.05);
                assert!(
                    contrast >= 4.5,
                    "selected text contrast {contrast:.2} for {color:?}"
                );
            }
        }
    }
}
