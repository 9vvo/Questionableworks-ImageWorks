//! The document canvas: drawing composited tiles with the view
//! transform, and the view tools (hand, zoom, rotate).
//!
//! Tiles are composited off the UI thread by [`crate::renderer`] and kept
//! here as GPU textures. Panning, zooming and rotating only move those
//! textures; nothing is recomposited (architecture rule 5).

use crate::renderer::{DocKey, Rendered};
use crate::theme;
use crate::view::View;
use eframe::egui::{
    self, pos2, vec2, Color32, CursorIcon, Mesh, Pos2, Rect, Sense, Shape, Stroke, TextureHandle,
    TextureId, TextureOptions, Ui,
};
use iw_engine::document::Document;
use iw_engine::tile::{TileCoord, TILE_SIZE};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Hand,
    Zoom,
    Rotate,
}

impl Tool {
    pub fn name(self) -> &'static str {
        match self {
            Tool::Hand => "Hand",
            Tool::Zoom => "Zoom",
            Tool::Rotate => "Rotate View",
        }
    }
}

/// Pixels inside one tile are drawn crisp when zoomed in and smoothed,
/// with mipmaps, when zoomed out.
const TILE_TEXTURE: TextureOptions = TextureOptions {
    magnification: egui::TextureFilter::Nearest,
    minification: egui::TextureFilter::Linear,
    wrap_mode: egui::TextureWrapMode::ClampToEdge,
    mipmap_mode: Some(egui::TextureFilter::Linear),
};

/// Screen size of one square of the transparency checkerboard.
const CHECKER_CELL: f32 = 8.0;

pub fn checker_texture(ctx: &egui::Context) -> TextureHandle {
    let (l, d) = (theme::CHECKER_LIGHT, theme::CHECKER_DARK);
    let image = egui::ColorImage::new([2, 2], vec![l, d, d, l]);
    let options = TextureOptions {
        magnification: egui::TextureFilter::Nearest,
        minification: egui::TextureFilter::Nearest,
        wrap_mode: egui::TextureWrapMode::Repeat,
        mipmap_mode: None,
    };
    ctx.load_texture("checkerboard", image, options)
}

/// What the canvas reports back to the app each frame.
#[derive(Default)]
pub struct CanvasEvents {
    /// Where the pointer is, in document pixels.
    pub cursor: Option<Pos2>,
    /// The viewport changed size or was shown for the first time.
    pub viewport: Option<Rect>,
}

/// Per-document canvas state.
pub struct Canvas {
    key: DocKey,
    pub view: Option<View>,
    textures: HashMap<TileCoord, TextureHandle>,
    rotate_drag: Option<(f32, f32)>,
    last_viewport: Option<Rect>,
}

impl Canvas {
    pub fn new(key: DocKey) -> Self {
        Self {
            key,
            view: None,
            textures: HashMap::new(),
            rotate_drag: None,
            last_viewport: None,
        }
    }

    pub fn viewport(&self) -> Option<Rect> {
        self.last_viewport
    }

    /// Installs a finished tile, or removes it if the tile is now empty.
    pub fn apply(&mut self, ctx: &egui::Context, result: Rendered) {
        match result.image {
            Some(image) => match self.textures.get_mut(&result.coord) {
                Some(texture) => texture.set(image, TILE_TEXTURE),
                None => {
                    let name = format!("doc{}-{}-{}", self.key, result.coord.tx, result.coord.ty);
                    self.textures
                        .insert(result.coord, ctx.load_texture(name, image, TILE_TEXTURE));
                }
            },
            None => {
                self.textures.remove(&result.coord);
            }
        }
    }

    /// Coordinates of every tile currently on the GPU.
    pub fn tile_coords(&self) -> impl Iterator<Item = TileCoord> + '_ {
        self.textures.keys().copied()
    }

    /// The tile under the middle of the viewport, to composite first.
    pub fn focus_tile(&self) -> Option<TileCoord> {
        let view = self.view?;
        let doc = view.center;
        Some(TileCoord::of_pixel(doc.x.floor() as i32, doc.y.floor() as i32).0)
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        doc: &Document,
        tool: Tool,
        checker: &TextureHandle,
    ) -> CanvasEvents {
        let mut events = CanvasEvents::default();
        let rect = ui.available_rect_before_wrap();
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        if self.last_viewport != Some(rect) {
            self.last_viewport = Some(rect);
            events.viewport = Some(rect);
        }
        let view = self
            .view
            .get_or_insert_with(|| View::fit(doc.width(), doc.height(), rect));

        // ----- Input -----
        let (space, alt, shift, scroll, zoom_delta, pointer) = ui.input(|i| {
            (
                i.key_down(egui::Key::Space),
                i.modifiers.alt,
                i.modifiers.shift,
                i.smooth_scroll_delta,
                i.zoom_delta(),
                i.pointer.hover_pos(),
            )
        });
        let typing = ui.ctx().egui_wants_keyboard_input();
        let middle = ui.input(|i| i.pointer.middle_down());
        let effective = if (space && !typing) || middle {
            Tool::Hand
        } else {
            tool
        };

        if response.hovered() {
            if zoom_delta != 1.0 {
                if let Some(p) = pointer {
                    view.zoom_about(p, view.zoom * zoom_delta, rect);
                }
            } else if scroll != egui::Vec2::ZERO {
                view.pan_by(scroll);
            }
        }

        match effective {
            Tool::Hand => {
                if response.dragged() {
                    view.pan_by(response.drag_delta());
                }
                ui.ctx().set_cursor_icon(if response.dragged() {
                    CursorIcon::Grabbing
                } else {
                    CursorIcon::Grab
                });
            }
            Tool::Zoom => {
                if response.clicked() {
                    if let Some(p) = response.interact_pointer_pos() {
                        let target = if alt { view.step_out() } else { view.step_in() };
                        view.zoom_about(p, target, rect);
                    }
                }
                if response.hovered() {
                    ui.ctx().set_cursor_icon(if alt {
                        CursorIcon::ZoomOut
                    } else {
                        CursorIcon::ZoomIn
                    });
                }
            }
            Tool::Rotate => {
                let angle_of = |p: Pos2| (p - rect.center()).angle();
                if response.drag_started() {
                    if let Some(p) = response.interact_pointer_pos() {
                        self.rotate_drag = Some((view.rotation, angle_of(p)));
                    }
                }
                if let (Some((base, start)), Some(p)) =
                    (self.rotate_drag, response.interact_pointer_pos())
                {
                    if response.dragged() {
                        let mut angle = base + angle_of(p) - start;
                        if shift {
                            let step = 15f32.to_radians();
                            angle = (angle / step).round() * step;
                        }
                        view.set_rotation(angle);
                    }
                }
                if response.drag_stopped() {
                    self.rotate_drag = None;
                }
                if response.hovered() {
                    ui.ctx().set_cursor_icon(CursorIcon::AllScroll);
                }
            }
        }

        if let Some(p) = response.hover_pos() {
            events.cursor = Some(view.screen_to_doc(p, rect));
        }

        // ----- Drawing -----
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, theme::CANVAS_BACKGROUND);
        let view = *view;
        let canvas = Rect::from_min_size(
            pos2(0.0, 0.0),
            vec2(doc.width() as f32, doc.height() as f32),
        );
        let corners = [
            canvas.left_top(),
            canvas.right_top(),
            canvas.right_bottom(),
            canvas.left_bottom(),
        ]
        .map(|p| view.doc_to_screen(p, rect));

        // Transparency checkerboard, fixed to the screen.
        let mut mesh = Mesh::with_texture(checker.id());
        for p in corners {
            mesh.vertices.push(egui::epaint::Vertex {
                pos: p,
                uv: pos2(p.x / (2.0 * CHECKER_CELL), p.y / (2.0 * CHECKER_CELL)),
                color: Color32::WHITE,
            });
        }
        mesh.add_triangle(0, 1, 2);
        mesh.add_triangle(0, 2, 3);
        painter.add(Shape::mesh(mesh));

        // Composited tiles that are on the canvas and on screen.
        let visible = view.visible_doc_rect(rect).intersect(canvas);
        let size = TILE_SIZE as f32;
        if visible.is_positive() {
            for (coord, texture) in &self.textures {
                let (x, y) = coord.origin();
                let tile = Rect::from_min_size(pos2(x as f32, y as f32), vec2(size, size));
                let part = tile.intersect(canvas);
                if !part.is_positive() || !part.intersects(visible) {
                    continue;
                }
                painter.add(Shape::mesh(tile_mesh(
                    texture.id(),
                    tile,
                    part,
                    &view,
                    rect,
                )));
            }
        }

        // Canvas edge.
        painter.add(Shape::closed_line(
            corners.to_vec(),
            Stroke::new(1.0, Color32::from_black_alpha(160)),
        ));
        events
    }
}

/// A quad showing the `part` of `tile` (both in document pixels).
fn tile_mesh(texture: TextureId, tile: Rect, part: Rect, view: &View, viewport: Rect) -> Mesh {
    let mut mesh = Mesh::with_texture(texture);
    let uv = |p: Pos2| {
        pos2(
            (p.x - tile.min.x) / tile.width(),
            (p.y - tile.min.y) / tile.height(),
        )
    };
    for p in [
        part.left_top(),
        part.right_top(),
        part.right_bottom(),
        part.left_bottom(),
    ] {
        mesh.vertices.push(egui::epaint::Vertex {
            pos: view.doc_to_screen(p, viewport),
            uv: uv(p),
            color: Color32::WHITE,
        });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    mesh
}
