//! The application's look: a dense dark theme with a warm accent.

use eframe::egui::{self, Color32, CornerRadius, FontId, Margin, Stroke, TextStyle};

pub const ACCENT: Color32 = Color32::from_rgb(0xE8, 0xA3, 0x3D);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(0x6B, 0x4E, 0x22);
pub const CANVAS_BACKGROUND: Color32 = Color32::from_rgb(0x1B, 0x1B, 0x1D);
pub const PANEL: Color32 = Color32::from_rgb(0x2A, 0x2A, 0x2D);
pub const PANEL_DARK: Color32 = Color32::from_rgb(0x23, 0x23, 0x26);
pub const TEXT: Color32 = Color32::from_rgb(0xDA, 0xDA, 0xDC);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0x8E, 0x8E, 0x93);
pub const CHECKER_LIGHT: Color32 = Color32::from_rgb(0xCC, 0xCC, 0xCC);
pub const CHECKER_DARK: Color32 = Color32::from_rgb(0x99, 0x99, 0x99);

pub fn apply(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.panel_fill = PANEL;
    visuals.window_fill = PANEL;
    visuals.extreme_bg_color = PANEL_DARK;
    visuals.faint_bg_color = Color32::from_rgb(0x30, 0x30, 0x34);
    visuals.override_text_color = Some(TEXT);
    visuals.selection.bg_fill = ACCENT_DIM;
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);
    visuals.hyperlink_color = ACCENT;
    let radius = CornerRadius::same(3);
    for w in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        w.corner_radius = radius;
    }
    visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(0x3A, 0x3A, 0x3E);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x46, 0x46, 0x4B);
    visuals.widgets.active.weak_bg_fill = ACCENT_DIM;
    visuals.window_corner_radius = CornerRadius::same(6);
    ctx.set_visuals(visuals);

    ctx.all_styles_mut(|style| {
        style.spacing.item_spacing = egui::vec2(6.0, 4.0);
        style.spacing.button_padding = egui::vec2(6.0, 2.0);
        style.spacing.interact_size.y = 20.0;
        style.spacing.window_margin = Margin::same(10);
        style
            .text_styles
            .insert(TextStyle::Body, FontId::proportional(13.0));
        style
            .text_styles
            .insert(TextStyle::Button, FontId::proportional(13.0));
        style
            .text_styles
            .insert(TextStyle::Small, FontId::proportional(11.0));
        style
            .text_styles
            .insert(TextStyle::Monospace, FontId::monospace(12.0));
    });
}
