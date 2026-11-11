//! A compact design system: near-black surfaces, one accent, thin borders.
//!
//! egui's defaults are serviceable but look like a debug panel. Everything
//! visual lives here so the window code stays about layout and state.

use egui::{
    Color32, FontFamily, FontId, Margin, Response, RichText, Rounding, Sense, Stroke, TextStyle,
    Ui, Visuals,
};

pub const BG: Color32 = Color32::from_rgb(0x0A, 0x0B, 0x0D);
pub const SURFACE: Color32 = Color32::from_rgb(0x11, 0x13, 0x16);
pub const SURFACE_2: Color32 = Color32::from_rgb(0x17, 0x1A, 0x1F);
pub const BORDER: Color32 = Color32::from_rgb(0x22, 0x25, 0x2B);
pub const BORDER_STRONG: Color32 = Color32::from_rgb(0x2E, 0x33, 0x3B);
pub const TEXT: Color32 = Color32::from_rgb(0xE8, 0xEA, 0xED);
pub const MUTED: Color32 = Color32::from_rgb(0x8B, 0x92, 0x9E);
pub const FAINT: Color32 = Color32::from_rgb(0x5C, 0x62, 0x6E);
pub const ACCENT: Color32 = Color32::from_rgb(0x5E, 0x6A, 0xD2);
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0x70, 0x7B, 0xE4);
pub const OK: Color32 = Color32::from_rgb(0x4C, 0xB7, 0x82);
pub const WARN: Color32 = Color32::from_rgb(0xE0, 0xA8, 0x4C);
pub const ERR: Color32 = Color32::from_rgb(0xE5, 0x6B, 0x6B);

const RADIUS: f32 = 8.0;

pub fn apply(ctx: &egui::Context) {
    let mut visuals = Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = BG;
    visuals.faint_bg_color = SURFACE_2;
    visuals.extreme_bg_color = SURFACE_2;
    visuals.override_text_color = Some(TEXT);
    visuals.selection.bg_fill = ACCENT.gamma_multiply(0.45);
    visuals.selection.stroke = Stroke::new(1.0_f32, TEXT);

    visuals.widgets.noninteractive.bg_fill = SURFACE;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, MUTED);
    visuals.widgets.noninteractive.rounding = Rounding::same(RADIUS);

    visuals.widgets.inactive.bg_fill = SURFACE_2;
    visuals.widgets.inactive.weak_bg_fill = SURFACE_2;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.inactive.rounding = Rounding::same(RADIUS);

    visuals.widgets.hovered.bg_fill = Color32::from_rgb(0x1D, 0x21, 0x27);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x1D, 0x21, 0x27);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, BORDER_STRONG);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.hovered.rounding = Rounding::same(RADIUS);

    visuals.widgets.active.bg_fill = Color32::from_rgb(0x23, 0x27, 0x2E);
    visuals.widgets.active.weak_bg_fill = Color32::from_rgb(0x23, 0x27, 0x2E);
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.active.rounding = Rounding::same(RADIUS);

    visuals.widgets.open.bg_fill = SURFACE_2;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0_f32, BORDER_STRONG);
    visuals.widgets.open.rounding = Rounding::same(RADIUS);

    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (
            TextStyle::Heading,
            FontId::new(21.0, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(13.5, FontFamily::Proportional),
        ),
        (
            TextStyle::Small,
            FontId::new(11.5, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(12.0, FontFamily::Monospace),
        ),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(12.0, 6.0);
    style.spacing.interact_size.y = 26.0;
    style.visuals = visuals;
    ctx.set_style(style);
}

/// A bordered panel that groups one part of the window.
pub fn card(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    egui::Frame::none()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .rounding(Rounding::same(12.0))
        .inner_margin(Margin::symmetric(16.0, 14.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

pub fn label(ui: &mut Ui, text: &str, color: Color32, size: f32) {
    ui.label(RichText::new(text).color(color).size(size));
}

pub fn muted(ui: &mut Ui, text: &str) {
    label(ui, text, MUTED, 12.0);
}

/// Section label in the small caps style dashboards use.
pub fn section(ui: &mut Ui, text: &str) {
    ui.add_space(2.0);
    label(ui, &text.to_uppercase(), FAINT, 10.5);
    ui.add_space(2.0);
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Primary,
    Secondary,
    Ghost,
    Danger,
}

pub fn button(ui: &mut Ui, text: &str, kind: Kind) -> Response {
    button_sized(ui, text, kind, None)
}

/// A button painted by hand so hover, focus and disabled states match the theme.
pub fn button_sized(ui: &mut Ui, text: &str, kind: Kind, width: Option<f32>) -> Response {
    let font = FontId::new(13.5, FontFamily::Proportional);
    let galley = ui.fonts(|fonts| fonts.layout_no_wrap(text.to_owned(), font, Color32::WHITE));
    let enabled = ui.is_enabled();
    let height = 30.0;
    let width = width.unwrap_or(galley.size().x + 28.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), Sense::click());

    let hovered = response.hovered() && enabled;
    let (mut fill, mut stroke, mut text_color) = match kind {
        Kind::Primary => (
            if hovered { ACCENT_HOVER } else { ACCENT },
            Stroke::NONE,
            Color32::WHITE,
        ),
        Kind::Secondary => (
            if hovered { SURFACE_2 } else { SURFACE },
            Stroke::new(1.0_f32, BORDER_STRONG),
            TEXT,
        ),
        Kind::Ghost => (
            if hovered {
                SURFACE_2
            } else {
                Color32::TRANSPARENT
            },
            Stroke::new(1.0_f32, BORDER),
            MUTED,
        ),
        Kind::Danger => (
            if hovered {
                ERR.gamma_multiply(0.22)
            } else {
                Color32::TRANSPARENT
            },
            Stroke::new(1.0_f32, ERR.gamma_multiply(0.7)),
            ERR,
        ),
    };
    if !enabled {
        fill = fill.gamma_multiply(0.5);
        stroke.color = stroke.color.gamma_multiply(0.5);
        text_color = text_color.gamma_multiply(0.5);
    }

    let painter = ui.painter();
    if fill != Color32::TRANSPARENT {
        painter.rect_filled(rect, Rounding::same(RADIUS), fill);
    }
    if stroke != Stroke::NONE {
        painter.rect_stroke(rect, Rounding::same(RADIUS), stroke);
    }
    painter.galley(rect.center() - galley.size() / 2.0, galley, text_color);

    response
}

/// Thin rounded progress track; the fill keeps a visible stub at 0%.
pub fn progress(ui: &mut Ui, fraction: f32) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 6.0), Sense::hover());
    let radius = Rounding::same(3.0);
    let painter = ui.painter();
    painter.rect_filled(rect, radius, SURFACE_2);
    let clamped = fraction.clamp(0.0, 1.0);
    if clamped > 0.0 {
        let filled = egui::Rect::from_min_size(
            rect.min,
            egui::vec2((rect.width() * clamped).max(6.0), rect.height()),
        );
        painter.rect_filled(filled, radius, ACCENT);
    }
}

pub fn pill(ui: &mut Ui, text: &str, color: Color32) {
    let font = FontId::new(11.5, FontFamily::Proportional);
    let galley = ui.fonts(|fonts| fonts.layout_no_wrap(text.to_owned(), font, color));
    let size = galley.size() + egui::vec2(20.0, 9.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, Rounding::same(20.0), color.gamma_multiply(0.14));
    painter.galley(rect.center() - galley.size() / 2.0, galley, color);
}

/// Two-state selector used for MP3/MP4.
pub fn segmented(ui: &mut Ui, options: &[(&str, bool)]) -> Option<usize> {
    let mut clicked = None;
    for (index, (text, selected)) in options.iter().enumerate() {
        let kind = if *selected {
            Kind::Primary
        } else {
            Kind::Secondary
        };
        if button_sized(ui, text, kind, Some(74.0)).clicked() {
            clicked = Some(index);
        }
    }
    clicked
}
