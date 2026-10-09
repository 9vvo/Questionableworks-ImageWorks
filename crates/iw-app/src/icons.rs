//! Small vector icons drawn with the painter, so the app ships no image
//! assets for its controls. All original designs.

use eframe::egui::{
    self, pos2, vec2, Color32, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, Ui,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Hand,
    Zoom,
    Rotate,
    Eye,
    EyeOff,
    Lock,
    Unlock,
    Folder,
    ChevronRight,
    ChevronDown,
    NewLayer,
    NewGroup,
    Trash,
}

pub fn paint(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let c = rect.center();
    let s = rect.width().min(rect.height()) / 16.0;
    let p = |x: f32, y: f32| c + vec2(x * s, y * s);
    let stroke = Stroke::new(1.3 * s.max(0.8), color);
    match icon {
        Icon::Hand => {
            // Palm and four fingers.
            painter.rect_stroke(
                Rect::from_min_max(p(-5.0, -1.0), p(5.0, 6.0)),
                2.0 * s,
                stroke,
                egui::StrokeKind::Middle,
            );
            for (x, top) in [(-4.0, -5.0), (-1.5, -7.0), (1.0, -7.0), (3.5, -5.5)] {
                painter.line_segment([p(x, -1.0), p(x, top)], stroke);
            }
            painter.line_segment([p(-5.0, 2.0), p(-7.0, -1.0)], stroke);
        }
        Icon::Zoom => {
            painter.circle_stroke(p(-1.5, -1.5), 4.5 * s, stroke);
            painter.line_segment([p(1.8, 1.8), p(6.0, 6.0)], Stroke::new(2.2 * s, color));
        }
        Icon::Rotate => {
            let points: Vec<Pos2> = (0..=20)
                .map(|i| -40.0f32.to_radians() + i as f32 * 13.0f32.to_radians())
                .map(|a| p(5.5 * a.cos(), 5.5 * a.sin()))
                .collect();
            let end = *points.last().unwrap();
            painter.add(Shape::line(points, stroke));
            painter.add(Shape::convex_polygon(
                vec![
                    end + vec2(-3.0 * s, -s),
                    end + vec2(2.0 * s, -2.5 * s),
                    end + vec2(0.5 * s, 2.5 * s),
                ],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Eye | Icon::EyeOff => {
            let top: Vec<Pos2> = (0..=16)
                .map(|i| i as f32 / 16.0)
                .map(|t| p(-6.5 + 13.0 * t, -4.0 * (t * std::f32::consts::PI).sin()))
                .collect();
            let bottom: Vec<Pos2> = top.iter().map(|q| pos2(q.x, 2.0 * c.y - q.y)).collect();
            painter.add(Shape::line(top, stroke));
            painter.add(Shape::line(bottom, stroke));
            painter.circle_filled(c, 1.8 * s, color);
            if icon == Icon::EyeOff {
                painter.line_segment([p(-6.0, 5.0), p(6.0, -5.0)], stroke);
            }
        }
        Icon::Lock | Icon::Unlock => {
            painter.rect_filled(
                Rect::from_min_max(p(-4.5, -1.0), p(4.5, 6.0)),
                1.0 * s,
                color,
            );
            // An open lock leaves the right leg of the shackle out.
            let arc: Vec<Pos2> = (0..=12)
                .map(|i| std::f32::consts::PI + i as f32 * std::f32::consts::PI / 12.0)
                .map(|a| p(3.0 * a.cos(), 3.0 * a.sin() - 3.0))
                .collect();
            painter.add(Shape::line(arc, stroke));
            painter.line_segment([p(-3.0, -3.0), p(-3.0, -1.0)], stroke);
            if icon == Icon::Lock {
                painter.line_segment([p(3.0, -3.0), p(3.0, -1.0)], stroke);
            }
        }
        Icon::Folder => {
            let body = vec![
                p(-6.5, -4.0),
                p(-2.0, -4.0),
                p(-0.5, -2.5),
                p(6.5, -2.5),
                p(6.5, 5.0),
                p(-6.5, 5.0),
            ];
            painter.add(Shape::closed_line(body, stroke));
        }
        Icon::ChevronRight => {
            painter.add(Shape::line(
                vec![p(-2.0, -4.0), p(2.5, 0.0), p(-2.0, 4.0)],
                stroke,
            ));
        }
        Icon::ChevronDown => {
            painter.add(Shape::line(
                vec![p(-4.0, -2.0), p(0.0, 2.5), p(4.0, -2.0)],
                stroke,
            ));
        }
        Icon::NewLayer => {
            painter.rect_stroke(
                Rect::from_min_max(p(-6.0, -6.0), p(4.0, 4.0)),
                1.0 * s,
                stroke,
                egui::StrokeKind::Middle,
            );
            painter.line_segment([p(3.0, 6.5), p(8.0, 6.5)], stroke);
            painter.line_segment([p(5.5, 4.0), p(5.5, 9.0)], stroke);
        }
        Icon::NewGroup => {
            let body = vec![
                p(-6.5, -4.0),
                p(-2.0, -4.0),
                p(-0.5, -2.5),
                p(5.0, -2.5),
                p(5.0, 4.0),
                p(-6.5, 4.0),
            ];
            painter.add(Shape::closed_line(body, stroke));
            painter.line_segment([p(3.0, 6.5), p(8.0, 6.5)], stroke);
            painter.line_segment([p(5.5, 4.0), p(5.5, 9.0)], stroke);
        }
        Icon::Trash => {
            painter.line_segment([p(-6.0, -4.5), p(6.0, -4.5)], stroke);
            painter.line_segment([p(-2.0, -6.5), p(2.0, -6.5)], stroke);
            painter.add(Shape::closed_line(
                vec![p(-4.5, -4.5), p(4.5, -4.5), p(3.5, 6.5), p(-3.5, 6.5)],
                stroke,
            ));
        }
    }
}

/// A square icon button. `selected` draws it highlighted, as for the
/// current tool.
pub fn button(ui: &mut Ui, icon: Icon, size: f32, selected: bool, tooltip: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let visuals = ui.style().interact_selectable(&response, selected);
    if selected || response.hovered() {
        ui.painter().rect_filled(rect, 3.0, visuals.weak_bg_fill);
    }
    let color = if selected {
        crate::theme::ACCENT
    } else {
        visuals.fg_stroke.color
    };
    paint(ui.painter(), rect.shrink(size * 0.2), icon, color);
    response.on_hover_text(tooltip)
}
