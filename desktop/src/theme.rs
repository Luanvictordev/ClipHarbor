//! ClipHarbor Desktop design system — Untitled UI foundations.
//!
//! Primitives and semantic colors, type scale, spacing, radii and the small set
//! of controls the window needs. All visuals live here so `main.rs` stays about
//! state and layout. The app ships the light theme to match the web interface.

use egui::{
    Color32, FontFamily, FontId, Margin, Response, RichText, Rounding, Sense, Stroke, TextStyle,
    Ui, Visuals,
};

/* --------------------------------------------------------------------------
   Semantic palette (Untitled UI light)
   -------------------------------------------------------------------------- */

pub const BG: Color32 = Color32::from_rgb(0xF9, 0xFA, 0xFB);
pub const SURFACE: Color32 = Color32::from_rgb(0xFF, 0xFF, 0xFF);
pub const SURFACE_2: Color32 = Color32::from_rgb(0xF9, 0xFA, 0xFB);
pub const SURFACE_MUTED: Color32 = Color32::from_rgb(0xF2, 0xF4, 0xF7);
pub const BORDER: Color32 = Color32::from_rgb(0xEA, 0xEC, 0xF0);
pub const BORDER_STRONG: Color32 = Color32::from_rgb(0xD0, 0xD5, 0xDD);
pub const TEXT: Color32 = Color32::from_rgb(0x10, 0x18, 0x28);
pub const MUTED: Color32 = Color32::from_rgb(0x47, 0x54, 0x67);
pub const FAINT: Color32 = Color32::from_rgb(0x66, 0x70, 0x85);
pub const PLACEHOLDER: Color32 = Color32::from_rgb(0x98, 0xA2, 0xB3);

pub const ACCENT: Color32 = Color32::from_rgb(0x7F, 0x56, 0xD9);
pub const ACCENT_HOVER: Color32 = Color32::from_rgb(0x69, 0x41, 0xC6);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(0xF9, 0xF5, 0xFF);
pub const ACCENT_BORDER: Color32 = Color32::from_rgb(0xE9, 0xD7, 0xFE);

pub const OK: Color32 = Color32::from_rgb(0x02, 0x7A, 0x48);
pub const OK_SOFT: Color32 = Color32::from_rgb(0xEC, 0xFD, 0xF3);
pub const OK_BORDER: Color32 = Color32::from_rgb(0xA6, 0xF4, 0xC5);

pub const WARN: Color32 = Color32::from_rgb(0xB5, 0x47, 0x08);
pub const WARN_SOFT: Color32 = Color32::from_rgb(0xFF, 0xFA, 0xEB);
pub const WARN_BORDER: Color32 = Color32::from_rgb(0xFE, 0xDF, 0x89);

pub const ERR: Color32 = Color32::from_rgb(0xB4, 0x23, 0x18);
pub const ERR_SOFT: Color32 = Color32::from_rgb(0xFE, 0xF3, 0xF2);
pub const ERR_BORDER: Color32 = Color32::from_rgb(0xFE, 0xCD, 0xCA);

const RADIUS: f32 = 8.0;
const CARD_RADIUS: f32 = 12.0;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Info,
    Success,
    Warning,
    Error,
}

fn tint(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

/* --------------------------------------------------------------------------
   Base style
   -------------------------------------------------------------------------- */

pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);

    let mut visuals = Visuals::light();
    visuals.panel_fill = BG;
    visuals.window_fill = SURFACE;
    visuals.faint_bg_color = SURFACE_MUTED;
    visuals.extreme_bg_color = SURFACE;
    visuals.code_bg_color = SURFACE_MUTED;
    visuals.override_text_color = Some(TEXT);
    visuals.hyperlink_color = ACCENT;
    visuals.warn_fg_color = WARN;
    visuals.error_fg_color = ERR;
    visuals.selection.bg_fill = tint(ACCENT, 46);
    visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    visuals.window_rounding = Rounding::same(CARD_RADIUS);
    visuals.window_stroke = Stroke::new(1.0_f32, BORDER);
    visuals.menu_rounding = Rounding::same(RADIUS);

    visuals.widgets.noninteractive.bg_fill = SURFACE;
    visuals.widgets.noninteractive.weak_bg_fill = SURFACE;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, MUTED);
    visuals.widgets.noninteractive.rounding = Rounding::same(RADIUS);

    visuals.widgets.inactive.bg_fill = SURFACE;
    visuals.widgets.inactive.weak_bg_fill = SURFACE;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER_STRONG);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.inactive.rounding = Rounding::same(RADIUS);

    visuals.widgets.hovered.bg_fill = SURFACE_2;
    visuals.widgets.hovered.weak_bg_fill = SURFACE_2;
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, BORDER_STRONG);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.hovered.rounding = Rounding::same(RADIUS);

    visuals.widgets.active.bg_fill = SURFACE_MUTED;
    visuals.widgets.active.weak_bg_fill = SURFACE_MUTED;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, TEXT);
    visuals.widgets.active.rounding = Rounding::same(RADIUS);

    visuals.widgets.open.bg_fill = SURFACE;
    visuals.widgets.open.bg_stroke = Stroke::new(1.0_f32, BORDER_STRONG);
    visuals.widgets.open.rounding = Rounding::same(RADIUS);

    let mut style = (*ctx.style()).clone();
    style.text_styles = [
        (
            TextStyle::Heading,
            FontId::new(24.0, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(14.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Small,
            FontId::new(12.0, FontFamily::Proportional),
        ),
        (
            TextStyle::Monospace,
            FontId::new(12.0, FontFamily::Monospace),
        ),
    ]
    .into();
    style.spacing.item_spacing = egui::vec2(10.0, 10.0);
    style.spacing.button_padding = egui::vec2(14.0, 8.0);
    style.spacing.interact_size.y = 36.0;
    style.visuals = visuals;
    ctx.set_style(style);
}

/// Prefer the native Windows UI face so the desktop build matches its platform.
#[cfg(windows)]
fn install_fonts(ctx: &egui::Context) {
    for path in [
        "C:/Windows/Fonts/segoeui.ttf",
        "C:/Windows/Fonts/SegoeUI.ttf",
    ] {
        if let Ok(bytes) = std::fs::read(path) {
            let mut fonts = egui::FontDefinitions::default();
            fonts.font_data.insert(
                "segoe-ui".to_owned(),
                egui::FontData::from_owned(bytes).into(),
            );
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .insert(0, "segoe-ui".to_owned());
            ctx.set_fonts(fonts);
            return;
        }
    }
}

#[cfg(not(windows))]
fn install_fonts(_ctx: &egui::Context) {}

/* --------------------------------------------------------------------------
   Typography
   -------------------------------------------------------------------------- */

pub fn heading(ui: &mut Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .size(24.0)
            .strong()
            .color(TEXT),
    );
}

pub fn title(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).size(16.0).strong().color(TEXT));
}

pub fn label(ui: &mut Ui, text: &str, color: Color32, size: f32) {
    ui.label(RichText::new(text).color(color).size(size));
}

pub fn muted(ui: &mut Ui, text: &str) {
    label(ui, text, MUTED, 13.0);
}

/// Small section label, Untitled UI's "Text sm / medium" role.
pub fn section(ui: &mut Ui, text: &str) {
    ui.add_space(2.0);
    ui.label(RichText::new(text).color(FAINT).size(11.5).strong());
    ui.add_space(2.0);
}

/* --------------------------------------------------------------------------
   Brand mark
   -------------------------------------------------------------------------- */

pub fn brand_mark(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(36.0, 36.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, Rounding::same(10.0), ACCENT);
    let center = rect.center();
    let stroke = Stroke::new(2.0_f32, Color32::WHITE);
    painter.line_segment(
        [
            egui::pos2(center.x, center.y - 8.0),
            egui::pos2(center.x, center.y + 4.0),
        ],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(center.x - 4.5, center.y - 0.5),
            egui::pos2(center.x, center.y + 4.0),
        ],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(center.x + 4.5, center.y - 0.5),
            egui::pos2(center.x, center.y + 4.0),
        ],
        stroke,
    );
    painter.line_segment(
        [
            egui::pos2(center.x - 6.5, center.y + 9.0),
            egui::pos2(center.x + 6.5, center.y + 9.0),
        ],
        stroke,
    );
}

/* --------------------------------------------------------------------------
   Surfaces and notices
   -------------------------------------------------------------------------- */

/// A card with Untitled UI's 12px radius, hairline border and white surface.
pub fn card(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    egui::Frame::none()
        .fill(SURFACE)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .rounding(Rounding::same(CARD_RADIUS))
        .inner_margin(Margin::symmetric(20.0, 14.0))
        .shadow(egui::epaint::Shadow {
            offset: egui::vec2(0.0, 1.0),
            blur: 2.0,
            spread: 0.0,
            color: Color32::from_black_alpha(12),
        })
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

/// Inline alert used for warnings, results and failure details.
pub fn notice(ui: &mut Ui, text: &str, tone: Tone) {
    let (fill, border, color) = match tone {
        Tone::Info => (ACCENT_SOFT, ACCENT_BORDER, ACCENT_HOVER),
        Tone::Success => (OK_SOFT, OK_BORDER, OK),
        Tone::Warning => (WARN_SOFT, WARN_BORDER, WARN),
        Tone::Error => (ERR_SOFT, ERR_BORDER, ERR),
    };

    egui::Frame::none()
        .fill(fill)
        .stroke(Stroke::new(1.0_f32, border))
        .rounding(Rounding::same(RADIUS))
        .inner_margin(Margin::symmetric(12.0, 9.0))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing.x = 7.0;
                ui.label(RichText::new("●").size(8.0).color(color));
                ui.label(RichText::new(text).size(12.5).color(color));
            });
        });
}

/* --------------------------------------------------------------------------
   Buttons
   -------------------------------------------------------------------------- */

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

/// A hand-painted button so hover, focus and disabled states match the system.
pub fn button_sized(ui: &mut Ui, text: &str, kind: Kind, width: Option<f32>) -> Response {
    let font = FontId::new(14.0, FontFamily::Proportional);
    let galley = ui.fonts(|fonts| fonts.layout_no_wrap(text.to_owned(), font, Color32::WHITE));
    let enabled = ui.is_enabled();
    let height = 38.0;
    let width = width.unwrap_or(galley.size().x + 30.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), Sense::click());

    let hovered = response.hovered() && enabled;
    let (fill, stroke, text_color) = match kind {
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
                SURFACE_MUTED
            } else {
                Color32::TRANSPARENT
            },
            Stroke::NONE,
            MUTED,
        ),
        Kind::Danger => (
            if hovered {
                tint(ERR, 30)
            } else {
                ERR_SOFT
            },
            Stroke::new(1.0_f32, ERR_BORDER),
            ERR,
        ),
    };

    let (fill, stroke, text_color) = if enabled {
        (fill, stroke, text_color)
    } else {
        (
            SURFACE_MUTED,
            Stroke::new(1.0_f32, BORDER),
            PLACEHOLDER,
        )
    };

    let painter = ui.painter();
    if fill != Color32::TRANSPARENT {
        painter.rect_filled(rect, Rounding::same(RADIUS), fill);
    }
    if stroke != Stroke::NONE {
        painter.rect_stroke(rect, Rounding::same(RADIUS), stroke);
    }
    if response.has_focus() {
        painter.rect_stroke(
            rect.expand(2.0),
            Rounding::same(RADIUS + 2.0),
            Stroke::new(2.0_f32, ACCENT),
        );
    }
    painter.galley(rect.center() - galley.size() / 2.0, galley, text_color);

    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

/* --------------------------------------------------------------------------
   Progress and status
   -------------------------------------------------------------------------- */

/// 8px progress track with a rounded brand fill.
pub fn progress(ui: &mut Ui, fraction: f32) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 8.0), Sense::hover());
    let radius = Rounding::same(4.0);
    let painter = ui.painter();
    painter.rect_filled(rect, radius, SURFACE_MUTED);
    let clamped = fraction.clamp(0.0, 1.0);
    if clamped > 0.0 {
        let filled = egui::Rect::from_min_size(
            rect.min,
            egui::vec2((rect.width() * clamped).max(8.0), rect.height()),
        );
        painter.rect_filled(filled, radius, ACCENT);
    }
}

/// Soft status pill with a colored dot, so state is never color-only.
pub fn pill(ui: &mut Ui, text: &str, color: Color32) {
    let font = FontId::new(11.5, FontFamily::Proportional);
    let galley = ui.fonts(|fonts| fonts.layout_no_wrap(text.to_owned(), font, color));
    let pad_x = 10.0;
    let dot = 6.0;
    let gap = 6.0;
    let size = egui::vec2(
        galley.size().x + pad_x * 2.0 + dot + gap,
        galley.size().y + 9.0,
    );
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, Rounding::same(20.0), tint(color, 22));
    painter.rect_stroke(
        rect,
        Rounding::same(20.0),
        Stroke::new(1.0_f32, tint(color, 55)),
    );
    let center_y = rect.center().y;
    painter.circle_filled(
        egui::pos2(rect.left() + pad_x + dot / 2.0, center_y),
        dot / 2.0,
        color,
    );
    painter.galley(
        egui::pos2(rect.left() + pad_x + dot + gap, center_y - galley.size().y / 2.0),
        galley,
        color,
    );
}

/* --------------------------------------------------------------------------
   Segmented control
   -------------------------------------------------------------------------- */

/// Compact selector used for MP3/MP4, painted as a grouped control.
pub fn segmented(ui: &mut Ui, options: &[(&str, bool)]) -> Option<usize> {
    let mut clicked = None;
    egui::Frame::none()
        .fill(SURFACE_2)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .rounding(Rounding::same(10.0))
        .inner_margin(Margin::same(3.0))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 3.0;
                for (index, (text, selected)) in options.iter().enumerate() {
                    if segment(ui, text, *selected).clicked() {
                        clicked = Some(index);
                    }
                }
            });
        });
    clicked
}

fn segment(ui: &mut Ui, text: &str, selected: bool) -> Response {
    let font = FontId::new(12.5, FontFamily::Proportional);
    let color = if selected { TEXT } else { MUTED };
    let galley = ui.fonts(|fonts| fonts.layout_no_wrap(text.to_owned(), font, color));
    let size = egui::vec2(galley.size().x + 26.0, 30.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hovered = response.hovered();

    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, Rounding::same(7.0), SURFACE);
        painter.rect_stroke(
            rect,
            Rounding::same(7.0),
            Stroke::new(1.0_f32, BORDER_STRONG),
        );
    } else if hovered {
        painter.rect_filled(rect, Rounding::same(7.0), SURFACE_MUTED);
    }
    if response.has_focus() {
        painter.rect_stroke(
            rect.expand(1.0),
            Rounding::same(8.0),
            Stroke::new(2.0_f32, ACCENT),
        );
    }
    painter.galley(rect.center() - galley.size() / 2.0, galley, color);
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
