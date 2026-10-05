//! CPU reference compositor.
//!
//! The one place where layers are combined (architecture rule 3). It works
//! tile by tile, so a caller can recomposite only what changed. The stack is
//! accumulated in `f32` and converted to the channel type once at the end,
//! so rounding does not build up layer by layer.
//!
//! For each layer over the accumulated backdrop, with premultiplied colours
//! `cs`, `cb`, alphas `as`, `ab` (layer opacity folded into the source) and
//! blend function `B` on straight colour:
//!
//! ```text
//! co = (1 - ab) * cs + (1 - as) * cb + as * ab * B(Cb, Cs)
//! ao = as + ab - as * ab
//! ```
//!
//! Masks, clipping groups and fill opacity arrive with later milestones and
//! belong here.

use crate::blend::{blend_rgb, BlendMode};
use crate::pixel::{self, Channel};
use crate::raster::Raster;
use crate::tile::{Tile, TileCoord, TILE_SIZE};
use std::collections::BTreeSet;

/// One entry in the layer stack handed to the compositor.
#[derive(Clone, Copy, Debug)]
pub struct Layer<'a, C: Channel> {
    pub raster: &'a Raster<C>,
    /// `0.0..=1.0`; values outside are clamped.
    pub opacity: f32,
    pub blend: BlendMode,
    pub visible: bool,
}

impl<'a, C: Channel> Layer<'a, C> {
    /// A visible, fully opaque, Normal layer.
    pub fn new(raster: &'a Raster<C>) -> Self {
        Self {
            raster,
            opacity: 1.0,
            blend: BlendMode::Normal,
            visible: true,
        }
    }

    pub fn with_blend(mut self, blend: BlendMode) -> Self {
        self.blend = blend;
        self
    }

    pub fn with_opacity(mut self, opacity: f32) -> Self {
        self.opacity = opacity;
        self
    }

    fn contributes(&self) -> bool {
        self.visible && self.opacity > 0.0
    }
}

/// Combines one source pixel with the backdrop. Both are premultiplied,
/// normalised RGBA; `(x, y)` is the document position, used only by Dissolve.
#[inline]
pub fn blend_pixel(
    mode: BlendMode,
    backdrop: [f32; 4],
    source: [f32; 4],
    opacity: f32,
    x: i32,
    y: i32,
) -> [f32; 4] {
    let sa = (source[3] * opacity).clamp(0.0, 1.0);
    if sa <= 0.0 {
        return backdrop;
    }

    if mode == BlendMode::Dissolve {
        // Coverage instead of transparency: the pixel is either fully the
        // source colour or not drawn at all.
        return if dissolve_threshold(x, y) < sa {
            let inv = 1.0 / source[3];
            [source[0] * inv, source[1] * inv, source[2] * inv, 1.0]
        } else {
            backdrop
        };
    }

    let ba = backdrop[3].clamp(0.0, 1.0);
    let s = [
        source[0] * opacity,
        source[1] * opacity,
        source[2] * opacity,
    ];
    let alpha = sa + ba - sa * ba;

    if mode == BlendMode::Normal || ba <= 0.0 {
        // With no backdrop there is nothing to blend against, so every mode
        // reduces to source-over.
        let k = 1.0 - sa;
        return [
            s[0] + backdrop[0] * k,
            s[1] + backdrop[1] * k,
            s[2] + backdrop[2] * k,
            alpha,
        ];
    }

    // Straight colours for the blend function. Clamped: the functions are
    // defined on 0..=1 (float documents may hold values above 1).
    let inv_sa = 1.0 / sa;
    let inv_ba = 1.0 / ba;
    let cs = s.map(|v| (v * inv_sa).clamp(0.0, 1.0));
    let cb = [backdrop[0], backdrop[1], backdrop[2]].map(|v| (v * inv_ba).clamp(0.0, 1.0));
    let blended = blend_rgb(mode, cb, cs);

    let mut out = [0.0, 0.0, 0.0, alpha];
    for i in 0..3 {
        out[i] = (1.0 - ba) * s[i] + (1.0 - sa) * backdrop[i] + sa * ba * blended[i];
    }
    out
}

/// A fixed pseudo-random value in `0.0..1.0` for each document pixel.
///
/// Position-only, so the dissolve pattern is stable across recomposites and
/// does not swim when a layer is edited.
#[inline]
pub fn dissolve_threshold(x: i32, y: i32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (y as u32).wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    h = h.wrapping_mul(0x297A_2D39);
    h ^= h >> 15;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

/// Composites the stack (bottom layer first) for one tile.
///
/// Returns `None` if no layer has content there.
pub fn composite_tile<C: Channel>(layers: &[Layer<'_, C>], coord: TileCoord) -> Option<Tile<C>> {
    // While the result is still an untouched copy of one opaque Normal
    // layer's tile, keep sharing that tile instead of converting it.
    let mut shared: Option<&Tile<C>> = None;
    let mut acc: Option<Vec<[f32; 4]>> = None;
    let (ox, oy) = coord.origin();

    for layer in layers.iter().filter(|l| l.contributes()) {
        let Some(tile) = layer.raster.tile(coord) else {
            continue;
        };

        if acc.is_none()
            && shared.is_none()
            && layer.blend == BlendMode::Normal
            && layer.opacity >= 1.0
        {
            shared = Some(tile);
            continue;
        }

        let buffer = acc.get_or_insert_with(|| match shared.take() {
            Some(base) => base.pixels().iter().map(|p| pixel::to_f32(*p)).collect(),
            None => vec![[0.0; 4]; tile.pixels().len()],
        });
        let opacity = layer.opacity.clamp(0.0, 1.0);
        let size = TILE_SIZE as usize;
        for (i, (dst, src)) in buffer.iter_mut().zip(tile.pixels()).enumerate() {
            let x = ox + (i % size) as i32;
            let y = oy + (i / size) as i32;
            *dst = blend_pixel(layer.blend, *dst, pixel::to_f32(*src), opacity, x, y);
        }
    }

    match (acc, shared) {
        (Some(buffer), _) => {
            let mut out = Tile::new();
            for (dst, src) in out.pixels_mut().iter_mut().zip(&buffer) {
                *dst = pixel::from_f32(*src);
            }
            Some(out)
        }
        (None, Some(tile)) => Some(tile.clone()),
        (None, None) => None,
    }
}

/// Composites the given tiles of the stack into a new raster.
pub fn composite_tiles<C: Channel>(
    layers: &[Layer<'_, C>],
    coords: impl IntoIterator<Item = TileCoord>,
) -> Raster<C> {
    let mut out = Raster::new();
    for coord in coords {
        if let Some(tile) = composite_tile(layers, coord) {
            out.insert_tile(coord, tile);
        }
    }
    out
}

/// Composites every tile any contributing layer has content in.
pub fn composite<C: Channel>(layers: &[Layer<'_, C>]) -> Raster<C> {
    let coords: BTreeSet<TileCoord> = layers
        .iter()
        .filter(|l| l.contributes())
        .flat_map(|l| l.raster.tile_coords())
        .collect();
    composite_tiles(layers, coords)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Rect;
    use crate::pixel::{from_straight, Pixel};

    fn solid<C: Channel>(rgba: [f32; 4]) -> Raster<C> {
        let mut r = Raster::new();
        r.write_rect(Rect::new(0, 0, 4, 4), &[from_straight(rgba); 16]);
        r
    }

    fn assert_px(actual: [f32; 4], expected: [f32; 4]) {
        for i in 0..4 {
            assert!(
                (actual[i] - expected[i]).abs() < 1e-5,
                "got {actual:?}, expected {expected:?}"
            );
        }
    }

    #[test]
    fn half_transparent_normal_is_the_average() {
        let out = blend_pixel(
            BlendMode::Normal,
            [0.2, 0.4, 0.6, 1.0],
            [0.5, 0.0, 0.0, 0.5],
            1.0,
            0,
            0,
        );
        assert_px(out, [0.6, 0.2, 0.3, 1.0]);
    }

    #[test]
    fn opacity_scales_the_source() {
        let full = blend_pixel(
            BlendMode::Normal,
            [0.2, 0.4, 0.6, 1.0],
            [0.5, 0.0, 0.0, 0.5],
            1.0,
            0,
            0,
        );
        let via_opacity = blend_pixel(
            BlendMode::Normal,
            [0.2, 0.4, 0.6, 1.0],
            [1.0, 0.0, 0.0, 1.0],
            0.5,
            0,
            0,
        );
        assert_px(via_opacity, full);
    }

    #[test]
    fn two_half_transparent_pixels_accumulate_alpha() {
        // Straight red at 50% over straight blue at 50%.
        let out = blend_pixel(
            BlendMode::Normal,
            [0.0, 0.0, 0.5, 0.5],
            [0.5, 0.0, 0.0, 0.5],
            1.0,
            0,
            0,
        );
        assert_px(out, [0.5, 0.0, 0.25, 0.75]);
    }

    #[test]
    fn multiply_respects_partial_alpha_on_both_sides() {
        // Straight Cs = 0.5 at as = 0.5, straight Cb = 0.8 at ab = 0.5.
        // co = (1-ab)*cs + (1-as)*cb + as*ab*Cb*Cs
        //    = 0.5*0.25 + 0.5*0.4 + 0.25*0.4 = 0.425, ao = 0.75
        let out = blend_pixel(
            BlendMode::Multiply,
            [0.4, 0.4, 0.4, 0.5],
            [0.25, 0.25, 0.25, 0.5],
            1.0,
            0,
            0,
        );
        assert_px(out, [0.425, 0.425, 0.425, 0.75]);
    }

    #[test]
    fn every_mode_over_nothing_is_the_source() {
        let source = [0.3, 0.2, 0.1, 0.6];
        for mode in BlendMode::ALL {
            if mode == BlendMode::Dissolve {
                continue;
            }
            assert_px(blend_pixel(mode, [0.0; 4], source, 1.0, 0, 0), source);
        }
    }

    #[test]
    fn every_mode_leaves_the_backdrop_alone_under_a_transparent_source() {
        let backdrop = [0.3, 0.2, 0.1, 0.6];
        for mode in BlendMode::ALL {
            assert_eq!(blend_pixel(mode, backdrop, [0.0; 4], 1.0, 3, 4), backdrop);
            assert_eq!(
                blend_pixel(mode, backdrop, [0.5, 0.5, 0.5, 1.0], 0.0, 3, 4),
                backdrop
            );
        }
    }

    #[test]
    fn dissolve_is_all_or_nothing_and_tracks_opacity() {
        let backdrop = [0.0, 0.0, 1.0, 1.0];
        let source = [0.5, 0.0, 0.0, 0.5]; // straight red at 50%
        let mut drawn = 0;
        let total = 200 * 200;
        for y in 0..200 {
            for x in 0..200 {
                let out = blend_pixel(BlendMode::Dissolve, backdrop, source, 1.0, x, y);
                if out == [1.0, 0.0, 0.0, 1.0] {
                    drawn += 1;
                } else {
                    assert_eq!(out, backdrop);
                }
            }
        }
        // 50% coverage, within sampling noise for 40,000 pixels.
        let fraction = drawn as f32 / total as f32;
        assert!((fraction - 0.5).abs() < 0.02, "coverage was {fraction}");

        // Fully opaque always draws.
        assert_eq!(
            blend_pixel(
                BlendMode::Dissolve,
                backdrop,
                [0.0, 1.0, 0.0, 1.0],
                1.0,
                7,
                9
            ),
            [0.0, 1.0, 0.0, 1.0]
        );
    }

    #[test]
    fn dissolve_pattern_is_stable() {
        assert_eq!(dissolve_threshold(10, 20), dissolve_threshold(10, 20));
        assert_ne!(dissolve_threshold(10, 20), dissolve_threshold(20, 10));
    }

    #[test]
    fn single_opaque_layer_is_shared_not_copied() {
        let r: Raster<u8> = solid([0.2, 0.4, 0.6, 1.0]);
        let coord = TileCoord::new(0, 0);
        let out = composite_tile(&[Layer::new(&r)], coord).unwrap();
        assert!(out.shares_data_with(r.tile(coord).unwrap()));
    }

    #[test]
    fn opaque_top_layer_wins_exactly() {
        let bottom: Raster<u8> = solid([0.2, 0.4, 0.6, 1.0]);
        let top: Raster<u8> = solid([0.9, 0.1, 0.5, 1.0]);
        let out = composite(&[Layer::new(&bottom), Layer::new(&top)]);
        assert_eq!(out.pixel(1, 1), top.pixel(1, 1));
        // Outside the painted 4x4 area both are transparent.
        assert_eq!(out.pixel(10, 10), [0; 4]);
    }

    #[test]
    fn hidden_and_zero_opacity_layers_are_ignored() {
        let bottom: Raster<u8> = solid([0.2, 0.4, 0.6, 1.0]);
        let top: Raster<u8> = solid([0.9, 0.1, 0.5, 1.0]);
        let mut hidden = Layer::new(&top);
        hidden.visible = false;
        let out = composite(&[
            Layer::new(&bottom),
            hidden,
            Layer::new(&top).with_opacity(0.0),
        ]);
        assert_eq!(out, bottom);
    }

    #[test]
    fn empty_stack_composites_to_nothing() {
        let empty: Raster<u8> = Raster::new();
        assert!(composite(&[Layer::new(&empty)]).is_empty());
        assert!(composite_tile::<u8>(&[], TileCoord::new(0, 0)).is_none());
    }

    #[test]
    fn layers_on_different_tiles_both_appear() {
        let mut a: Raster<u8> = Raster::new();
        a.set_pixel(5, 5, [10, 20, 30, 255]);
        let mut b: Raster<u8> = Raster::new();
        b.set_pixel(-5, 600, [40, 50, 60, 255]);
        let out = composite(&[
            Layer::new(&a),
            Layer::new(&b).with_blend(BlendMode::Multiply),
        ]);
        assert_eq!(out.tile_count(), 2);
        assert_eq!(out.pixel(5, 5), [10, 20, 30, 255]);
        assert_eq!(out.pixel(-5, 600), [40, 50, 60, 255]);
    }

    #[test]
    fn multiply_of_8_bit_layers() {
        // 0.4 * 0.6 = 0.24 -> 61 in 8-bit (0.24 * 255 = 61.2).
        let bottom: Raster<u8> = solid([0.4, 0.4, 0.4, 1.0]);
        let top: Raster<u8> = solid([0.6, 0.6, 0.6, 1.0]);
        let out = composite(&[
            Layer::new(&bottom),
            Layer::new(&top).with_blend(BlendMode::Multiply),
        ]);
        assert_eq!(out.pixel(0, 0), [61, 61, 61, 255]);
    }

    /// The same stack at each bit depth must agree to within the coarser
    /// depth's quantisation.
    #[test]
    fn bit_depths_agree() {
        fn run<C: Channel>(mode: BlendMode) -> [f32; 4] {
            let bottom: Raster<C> = solid([0.25, 0.5, 0.75, 0.8]);
            let top: Raster<C> = solid([0.9, 0.3, 0.6, 0.7]);
            let out = composite(&[
                Layer::new(&bottom),
                Layer::new(&top).with_blend(mode).with_opacity(0.9),
            ]);
            let p: Pixel<C> = out.pixel(2, 2);
            pixel::to_f32(p)
        }
        for mode in BlendMode::ALL {
            if mode == BlendMode::Dissolve {
                continue;
            }
            let (a, b, c) = (run::<u8>(mode), run::<u16>(mode), run::<f32>(mode));
            for i in 0..4 {
                // Inputs are quantised differently per depth, so allow a few
                // 8-bit steps (steep modes amplify the input difference).
                assert!(
                    (a[i] - c[i]).abs() < 4.0 / 255.0,
                    "{mode:?} u8 {a:?} vs f32 {c:?}"
                );
                assert!(
                    (b[i] - c[i]).abs() < 4.0 / 65535.0 + 1e-4,
                    "{mode:?} u16 {b:?} vs f32 {c:?}"
                );
            }
        }
    }

    #[test]
    fn float_normal_passes_values_above_one() {
        let mut hdr: Raster<f32> = Raster::new();
        hdr.set_pixel(0, 0, [4.0, 2.0, 1.0, 1.0]);
        let mut under: Raster<f32> = Raster::new();
        under.set_pixel(0, 0, [0.1, 0.1, 0.1, 1.0]);
        let out = composite(&[Layer::new(&under), Layer::new(&hdr).with_opacity(0.5)]);
        assert_px(out.pixel(0, 0), [2.05, 1.05, 0.55, 1.0]);
    }
}
