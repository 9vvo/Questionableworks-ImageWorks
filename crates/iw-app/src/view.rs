//! The canvas view: how document pixels map to screen points.
//!
//! Pure geometry, no UI, so it can be tested directly. A view is a zoom,
//! a rotation, an optional horizontal mirror, and the document point shown
//! at the centre of the viewport. Changing the view never touches the
//! document and never recomposites anything (architecture rule 5).

use eframe::egui::{pos2, vec2, Pos2, Rect, Vec2};

/// Zoom levels the zoom-in and zoom-out steps snap to, as in most editors.
pub const ZOOM_STEPS: [f32; 25] = [
    1.0 / 64.0,
    1.0 / 48.0,
    1.0 / 32.0,
    1.0 / 24.0,
    1.0 / 16.0,
    1.0 / 12.0,
    1.0 / 8.0,
    1.0 / 6.0,
    1.0 / 4.0,
    1.0 / 3.0,
    0.5,
    2.0 / 3.0,
    1.0,
    2.0,
    3.0,
    4.0,
    5.0,
    6.0,
    7.0,
    8.0,
    12.0,
    16.0,
    20.0,
    24.0,
    32.0,
];
pub const MIN_ZOOM: f32 = ZOOM_STEPS[0];
pub const MAX_ZOOM: f32 = ZOOM_STEPS[ZOOM_STEPS.len() - 1];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// Screen points per document pixel.
    pub zoom: f32,
    /// Clockwise, in radians.
    pub rotation: f32,
    /// Flips the document left to right, as seen on screen.
    pub mirror: bool,
    /// The document point shown at the centre of the viewport.
    pub center: Pos2,
}

fn rotate(v: Vec2, angle: f32) -> Vec2 {
    let (sin, cos) = angle.sin_cos();
    vec2(v.x * cos - v.y * sin, v.x * sin + v.y * cos)
}

impl View {
    /// A view that fits a `width` x `height` document in `viewport` with
    /// a margin, never enlarging past 100%.
    pub fn fit(width: u32, height: u32, viewport: Rect) -> Self {
        let mut view = Self {
            zoom: 1.0,
            rotation: 0.0,
            mirror: false,
            center: pos2(0.0, 0.0),
        };
        view.fit_to(width, height, viewport, false);
        view
    }

    /// Fits the document, keeping the current rotation and mirror. With
    /// `allow_enlarge` a small document is zoomed in to fill the space.
    pub fn fit_to(&mut self, width: u32, height: u32, viewport: Rect, allow_enlarge: bool) {
        let (w, h) = (width as f32, height as f32);
        // Size of the rotated document's bounding box at zoom 1.
        let (sin, cos) = self.rotation.sin_cos();
        let bw = (w * cos).abs() + (h * sin).abs();
        let bh = (w * sin).abs() + (h * cos).abs();
        let margin = 0.92;
        let mut zoom = (viewport.width() / bw).min(viewport.height() / bh) * margin;
        if !allow_enlarge {
            zoom = zoom.min(1.0);
        }
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        self.center = pos2(w / 2.0, h / 2.0);
    }

    /// Document space to screen space, without the translation.
    fn linear(&self, mut v: Vec2) -> Vec2 {
        if self.mirror {
            v.x = -v.x;
        }
        rotate(v, self.rotation) * self.zoom
    }

    /// Inverse of [`View::linear`].
    fn inverse_linear(&self, v: Vec2) -> Vec2 {
        let mut d = rotate(v / self.zoom, -self.rotation);
        if self.mirror {
            d.x = -d.x;
        }
        d
    }

    pub fn doc_to_screen(&self, doc: Pos2, viewport: Rect) -> Pos2 {
        viewport.center() + self.linear(doc - self.center)
    }

    pub fn screen_to_doc(&self, screen: Pos2, viewport: Rect) -> Pos2 {
        self.center + self.inverse_linear(screen - viewport.center())
    }

    /// Moves the view so the document follows a drag of `delta` on screen.
    pub fn pan_by(&mut self, delta: Vec2) {
        self.center -= self.inverse_linear(delta);
    }

    /// Changes the zoom, keeping the document point under `anchor` (a
    /// screen point) where it is.
    pub fn zoom_about(&mut self, anchor: Pos2, new_zoom: f32, viewport: Rect) {
        let before = self.screen_to_doc(anchor, viewport);
        self.zoom = new_zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        let after = self.screen_to_doc(anchor, viewport);
        self.center += before - after;
    }

    /// The next preset zoom level up from the current zoom.
    pub fn step_in(&self) -> f32 {
        ZOOM_STEPS
            .iter()
            .copied()
            .find(|z| *z > self.zoom * 1.001)
            .unwrap_or(MAX_ZOOM)
    }

    /// The next preset zoom level down.
    pub fn step_out(&self) -> f32 {
        ZOOM_STEPS
            .iter()
            .rev()
            .copied()
            .find(|z| *z < self.zoom / 1.001)
            .unwrap_or(MIN_ZOOM)
    }

    /// Rotates about the viewport centre, which keeps `center` fixed.
    pub fn set_rotation(&mut self, radians: f32) {
        let full = std::f32::consts::TAU;
        self.rotation = radians.rem_euclid(full);
    }

    pub fn rotation_degrees(&self) -> f32 {
        let d = self.rotation.to_degrees();
        if d > 180.0 {
            d - 360.0
        } else {
            d
        }
    }

    /// Flips the view left to right about the viewport centre.
    pub fn toggle_mirror(&mut self) {
        self.mirror = !self.mirror;
        // Mirroring after rotating should look like flipping the picture on
        // screen, so the on-screen rotation direction is reversed too.
        self.rotation = (-self.rotation).rem_euclid(std::f32::consts::TAU);
    }

    /// The document rectangle covering everything visible in `viewport`.
    pub fn visible_doc_rect(&self, viewport: Rect) -> Rect {
        let corners = [
            viewport.left_top(),
            viewport.right_top(),
            viewport.left_bottom(),
            viewport.right_bottom(),
        ];
        let mut rect = Rect::NOTHING;
        for corner in corners {
            rect.extend_with(self.screen_to_doc(corner, viewport));
        }
        rect
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn viewport() -> Rect {
        Rect::from_min_size(pos2(100.0, 50.0), vec2(800.0, 600.0))
    }

    fn close(a: Pos2, b: Pos2) -> bool {
        (a - b).length() < 1e-3
    }

    fn views() -> Vec<View> {
        let mut out = Vec::new();
        for zoom in [0.1, 1.0, 3.5] {
            for rotation in [0.0, 0.3, std::f32::consts::FRAC_PI_2, 4.0] {
                for mirror in [false, true] {
                    out.push(View {
                        zoom,
                        rotation,
                        mirror,
                        center: pos2(37.0, -12.5),
                    });
                }
            }
        }
        out
    }

    #[test]
    fn screen_and_document_coordinates_round_trip() {
        let vp = viewport();
        for view in views() {
            for p in [pos2(0.0, 0.0), pos2(123.4, 567.8), pos2(-50.0, 9000.0)] {
                assert!(
                    close(view.screen_to_doc(view.doc_to_screen(p, vp), vp), p),
                    "{view:?} {p:?}"
                );
            }
            // The centre point is always in the middle of the viewport.
            assert!(close(view.doc_to_screen(view.center, vp), vp.center()));
        }
    }

    #[test]
    fn zoom_about_keeps_the_anchor_still() {
        let vp = viewport();
        for mut view in views() {
            let anchor = pos2(250.0, 400.0);
            let under = view.screen_to_doc(anchor, vp);
            view.zoom_about(anchor, view.zoom * 2.7, vp);
            assert!(close(view.doc_to_screen(under, vp), anchor), "{view:?}");
        }
    }

    #[test]
    fn panning_moves_the_document_with_the_pointer() {
        let vp = viewport();
        for mut view in views() {
            let p = pos2(10.0, 20.0);
            let before = view.doc_to_screen(p, vp);
            view.pan_by(vec2(33.0, -7.0));
            assert!(
                close(view.doc_to_screen(p, vp), before + vec2(33.0, -7.0)),
                "{view:?}"
            );
        }
    }

    #[test]
    fn zoom_steps_go_through_the_presets_and_stop_at_the_ends() {
        let mut view = View {
            zoom: 1.0,
            rotation: 0.0,
            mirror: false,
            center: pos2(0.0, 0.0),
        };
        assert_eq!(view.step_in(), 2.0);
        assert_eq!(view.step_out(), 2.0 / 3.0);
        view.zoom = 1.4; // between presets
        assert_eq!(view.step_in(), 2.0);
        assert_eq!(view.step_out(), 1.0);
        view.zoom = MAX_ZOOM;
        assert_eq!(view.step_in(), MAX_ZOOM);
        view.zoom = MIN_ZOOM;
        assert_eq!(view.step_out(), MIN_ZOOM);
    }

    #[test]
    fn fit_fills_the_viewport_without_enlarging_by_default() {
        let vp = viewport();
        let view = View::fit(4000, 1000, vp);
        assert!((view.zoom - 800.0 / 4000.0 * 0.92).abs() < 1e-4);
        assert_eq!(view.center, pos2(2000.0, 500.0));
        assert_eq!(View::fit(10, 10, vp).zoom, 1.0, "small images open at 100%");

        // Fitting a rotated document uses its rotated bounding box.
        let mut rotated = View::fit(4000, 1000, vp);
        rotated.set_rotation(std::f32::consts::FRAC_PI_2);
        rotated.fit_to(4000, 1000, vp, true);
        assert!((rotated.zoom - 600.0 / 4000.0 * 0.92).abs() < 1e-3);
    }

    #[test]
    fn rotation_and_mirror() {
        let vp = viewport();
        let mut view = View {
            zoom: 1.0,
            rotation: 0.0,
            mirror: false,
            center: pos2(0.0, 0.0),
        };
        view.set_rotation(std::f32::consts::FRAC_PI_2);
        // Rotated 90 degrees clockwise, a point to the right of the centre
        // appears below it.
        assert!(close(
            view.doc_to_screen(pos2(10.0, 0.0), vp),
            vp.center() + vec2(0.0, 10.0)
        ));
        assert!((view.rotation_degrees() - 90.0).abs() < 1e-3);
        view.set_rotation(-std::f32::consts::FRAC_PI_2);
        assert!((view.rotation_degrees() + 90.0).abs() < 1e-3);

        let mut view = View {
            zoom: 1.0,
            rotation: 0.0,
            mirror: false,
            center: pos2(0.0, 0.0),
        };
        view.toggle_mirror();
        assert!(close(
            view.doc_to_screen(pos2(10.0, 0.0), vp),
            vp.center() - vec2(10.0, 0.0)
        ));
        view.toggle_mirror();
        assert!(!view.mirror);
    }

    #[test]
    fn the_visible_rectangle_covers_the_viewport() {
        let vp = viewport();
        for view in views() {
            let visible = view.visible_doc_rect(vp);
            for corner in [vp.left_top(), vp.right_bottom(), vp.center()] {
                let doc = view.screen_to_doc(corner, vp);
                assert!(visible.expand(1e-3).contains(doc), "{view:?}");
            }
        }
    }
}
