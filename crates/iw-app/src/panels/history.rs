//! The History panel: every step since the document was opened, with the
//! undone ones greyed out. Clicking a row moves the document to that state.

use crate::theme;
use eframe::egui::{self, RichText, Sense, Ui};
use iw_engine::history::Session;

/// Shows the panel. Returns the history position to jump to, if a row was
/// clicked: 0 is the state the document was opened or created in.
pub fn show(ui: &mut Ui, session: &Session, origin: &str) -> Option<usize> {
    let mut jump = None;
    let states = session.states();
    let position = session.position();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            let rows = std::iter::once((origin.to_string(), true))
                .chain(states.into_iter().map(|s| (s.label, s.applied)));
            for (index, (label, applied)) in rows.enumerate() {
                let current = index == position;
                let (rect, response) =
                    ui.allocate_exact_size(egui::vec2(ui.available_width(), 22.0), Sense::click());
                if current {
                    ui.painter().rect_filled(rect, 2.0, theme::ACCENT_DIM);
                } else if response.hovered() {
                    ui.painter()
                        .rect_filled(rect, 2.0, ui.visuals().widgets.hovered.weak_bg_fill);
                }
                let color = if applied {
                    theme::TEXT
                } else {
                    theme::TEXT_DIM
                };
                let text = if index == 0 {
                    RichText::new(label).italics()
                } else {
                    RichText::new(label)
                };
                ui.painter().text(
                    rect.left_center() + egui::vec2(8.0, 0.0),
                    egui::Align2::LEFT_CENTER,
                    text.text(),
                    egui::FontId::proportional(13.0),
                    color,
                );
                if response.clicked() && !current {
                    jump = Some(index);
                }
            }
        });
    jump
}
