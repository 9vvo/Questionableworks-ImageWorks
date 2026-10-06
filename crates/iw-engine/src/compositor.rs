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
//! Groups are either pass-through (a folder) or isolated (composited alone,
//! then blended as one layer). Masks, clipping and fill opacity arrive with
//! later milestones and belong here.

use crate::blend::{blend_rgb, BlendMode};
use crate::geom::Rect;
use crate::pixel::{self, Channel, Pixel};
use crate::raster::Raster;
use crate::tile::{Tile, TileCoord, TILE_SIZE};
use std::collections::BTreeSet;

/// A raster layer in the stack handed to the compositor.
#[derive(Clone, Copy, Debug)]
pub struct Layer<'a, C: Channel> {
    pub raster: &'a Raster<C>,
    /// Where the raster's origin sits in the document.
    pub offset: (i32, i32),
    /// `0.0..=1.0`; values outside are clamped.
    pub opacity: f32,
    pub blend: BlendMode,
    pub visible: bool,
}

impl<'a, C: Channel> Layer<'a, C> {
    /// A visible, fully opaque, Normal layer at the origin.
    pub fn new(raster: &'a Raster<C>) -> Self {
        Self {
            raster,
            offset: (0, 0),
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

    pub fn with_offset(mut self, x: i32, y: i32) -> Self {
        self.offset = (x, y);
        self
    }
}

/// How a group combines with what lies beneath it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GroupBlend {
    /// The group is only a folder: its layers blend with the layers below
    /// the group as if they were not grouped.
    PassThrough,
    /// The group's layers are composited on their own first, and the result
    /// is blended as a single layer with this mode.
    Isolated(BlendMode),
}

/// A group of nodes, bottom first.
#[derive(Clone, Debug)]
pub struct Group<'a, C: Channel> {
    pub children: Vec<Node<'a, C>>,
    pub opacity: f32,
    pub blend: GroupBlend,
    pub visible: bool,
}

/// One entry in the stack: a layer or a group.
#[derive(Clone, Debug)]
pub enum Node<'a, C: Channel> {
    Layer(Layer<'a, C>),
    Group(Group<'a, C>),
}

impl<'a, C: Channel> From<Layer<'a, C>> for Node<'a, C> {
    fn from(layer: Layer<'a, C>) -> Self {
        Node::Layer(layer)
    }
}

impl<'a, C: Channel> From<Group<'a, C>> for Node<'a, C> {
    fn from(group: Group<'a, C>) -> Self {
        Node::Group(group)
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

const TILE_AREA: usize = (TILE_SIZE * TILE_SIZE) as usize;
const TILE: i32 = TILE_SIZE as i32;

/// The result so far for one tile.
enum Acc<'a, C: Channel> {
    Empty,
    /// Still an untouched copy of one opaque Normal layer's tile, so it can
    /// be shared instead of converted.
    Shared(&'a Tile<Pixel<C>>),
    Buffer(Vec<[f32; 4]>),
}

impl<C: Channel> Acc<'_, C> {
    fn to_floats(&self) -> Vec<[f32; 4]> {
        match self {
            Acc::Empty => vec![[0.0; 4]; TILE_AREA],
            Acc::Shared(tile) => tile.pixels().iter().map(|p| pixel::to_f32(*p)).collect(),
            Acc::Buffer(buffer) => buffer.clone(),
        }
    }

    fn buffer(&mut self) -> &mut Vec<[f32; 4]> {
        if !matches!(self, Acc::Buffer(_)) {
            *self = Acc::Buffer(self.to_floats());
        }
        match self {
            Acc::Buffer(buffer) => buffer,
            _ => unreachable!("just converted to a buffer"),
        }
    }

    fn into_tile(self) -> Option<Tile<Pixel<C>>> {
        match self {
            Acc::Empty => None,
            Acc::Shared(tile) => Some(tile.clone()),
            Acc::Buffer(buffer) => {
                let mut out = Tile::new();
                for (dst, src) in out.pixels_mut().iter_mut().zip(&buffer) {
                    *dst = pixel::from_f32(*src);
                }
                Some(out)
            }
        }
    }
}

/// Source pixels for one destination tile.
enum Source<'a, C: Channel> {
    Tile(&'a Tile<Pixel<C>>),
    Pixels(Vec<Pixel<C>>),
    Floats(Vec<[f32; 4]>),
}

/// The part of `layer` that lands on destination tile `coord`, if any.
fn layer_source<'a, C: Channel>(layer: &Layer<'a, C>, coord: TileCoord) -> Option<Source<'a, C>> {
    let (dx, dy) = layer.offset;
    if dx % TILE == 0 && dy % TILE == 0 {
        let source = TileCoord::new(
            coord.tx.wrapping_sub(dx / TILE),
            coord.ty.wrapping_sub(dy / TILE),
        );
        return layer.raster.tile(source).map(Source::Tile);
    }
    // Not tile-aligned: the destination tile straddles up to four source tiles.
    let (ox, oy) = coord.origin();
    let (sx, sy) = (ox.wrapping_sub(dx), oy.wrapping_sub(dy));
    let (first, _) = TileCoord::of_pixel(sx, sy);
    let any = [(0, 0), (1, 0), (0, 1), (1, 1)].iter().any(|(i, j)| {
        layer
            .raster
            .tile(TileCoord::new(
                first.tx.wrapping_add(*i),
                first.ty.wrapping_add(*j),
            ))
            .is_some()
    });
    any.then(|| {
        Source::Pixels(
            layer
                .raster
                .read_rect(Rect::new(sx, sy, TILE_SIZE, TILE_SIZE)),
        )
    })
}

fn blend_source<'a, C: Channel>(
    acc: &mut Acc<'a, C>,
    source: Source<'a, C>,
    mode: BlendMode,
    opacity: f32,
    coord: TileCoord,
) {
    if let (Acc::Empty, Source::Tile(tile), BlendMode::Normal, true) =
        (&*acc, &source, mode, opacity >= 1.0)
    {
        *acc = Acc::Shared(tile);
        return;
    }
    let (ox, oy) = coord.origin();
    let opacity = opacity.clamp(0.0, 1.0);
    let size = TILE_SIZE as usize;
    let buffer = acc.buffer();
    let mut apply = |i: usize, src: [f32; 4]| {
        let x = ox.wrapping_add((i % size) as i32);
        let y = oy.wrapping_add((i / size) as i32);
        buffer[i] = blend_pixel(mode, buffer[i], src, opacity, x, y);
    };
    match &source {
        Source::Tile(tile) => tile
            .pixels()
            .iter()
            .enumerate()
            .for_each(|(i, p)| apply(i, pixel::to_f32(*p))),
        Source::Pixels(pixels) => pixels
            .iter()
            .enumerate()
            .for_each(|(i, p)| apply(i, pixel::to_f32(*p))),
        Source::Floats(floats) => floats.iter().enumerate().for_each(|(i, p)| apply(i, *p)),
    }
}

/// Composites `nodes` onto `acc`. Returns whether anything was drawn.
fn composite_nodes<'a, C: Channel>(
    nodes: &[Node<'a, C>],
    coord: TileCoord,
    acc: &mut Acc<'a, C>,
) -> bool {
    let mut drew = false;
    for node in nodes {
        match node {
            Node::Layer(layer) => {
                if !layer.visible || layer.opacity <= 0.0 {
                    continue;
                }
                if let Some(source) = layer_source(layer, coord) {
                    blend_source(acc, source, layer.blend, layer.opacity, coord);
                    drew = true;
                }
            }
            Node::Group(group) => {
                if !group.visible || group.opacity <= 0.0 {
                    continue;
                }
                match group.blend {
                    GroupBlend::PassThrough if group.opacity >= 1.0 => {
                        drew |= composite_nodes(&group.children, coord, acc);
                    }
                    GroupBlend::PassThrough => {
                        // Fade between the result without and with the group.
                        let before = acc.to_floats();
                        if composite_nodes(&group.children, coord, acc) {
                            let t = group.opacity.clamp(0.0, 1.0);
                            for (after, before) in acc.buffer().iter_mut().zip(&before) {
                                for i in 0..4 {
                                    after[i] = before[i] + (after[i] - before[i]) * t;
                                }
                            }
                            drew = true;
                        }
                    }
                    GroupBlend::Isolated(mode) => {
                        let mut inner = Acc::Empty;
                        composite_nodes(&group.children, coord, &mut inner);
                        let source = match inner {
                            Acc::Empty => continue,
                            Acc::Shared(tile) => Source::Tile(tile),
                            Acc::Buffer(buffer) => Source::Floats(buffer),
                        };
                        blend_source(acc, source, mode, group.opacity, coord);
                        drew = true;
                    }
                }
            }
        }
    }
    drew
}

/// Composites the stack (bottom first) for one tile.
///
/// Returns `None` if nothing has content there.
pub fn composite_tile<C: Channel>(
    nodes: &[Node<'_, C>],
    coord: TileCoord,
) -> Option<Tile<Pixel<C>>> {
    let mut acc = Acc::Empty;
    composite_nodes(nodes, coord, &mut acc);
    acc.into_tile()
}

/// Composites the given tiles of the stack into a new raster.
pub fn composite_tiles<C: Channel>(
    nodes: &[Node<'_, C>],
    coords: impl IntoIterator<Item = TileCoord>,
) -> Raster<C> {
    let mut out = Raster::new();
    for coord in coords {
        if let Some(tile) = composite_tile(nodes, coord) {
            out.insert_tile(coord, tile);
        }
    }
    out
}

/// Every destination tile that any visible layer in the stack can touch.
pub fn covered_tiles<C: Channel>(nodes: &[Node<'_, C>]) -> BTreeSet<TileCoord> {
    fn walk<C: Channel>(nodes: &[Node<'_, C>], out: &mut BTreeSet<TileCoord>) {
        for node in nodes {
            match node {
                Node::Layer(layer) if layer.visible && layer.opacity > 0.0 => {
                    let (dx, dy) = layer.offset;
                    for coord in layer.raster.tile_coords() {
                        let (x, y) = coord.origin();
                        let (x0, y0) = (x.wrapping_add(dx), y.wrapping_add(dy));
                        let (first, _) = TileCoord::of_pixel(x0, y0);
                        let (last, _) = TileCoord::of_pixel(
                            x0.wrapping_add(TILE - 1),
                            y0.wrapping_add(TILE - 1),
                        );
                        for ty in first.ty..=last.ty {
                            for tx in first.tx..=last.tx {
                                out.insert(TileCoord::new(tx, ty));
                            }
                        }
                    }
                }
                Node::Group(group) if group.visible && group.opacity > 0.0 => {
                    walk(&group.children, out)
                }
                _ => {}
            }
        }
    }
    let mut out = BTreeSet::new();
    walk(nodes, &mut out);
    out
}

/// Composites every tile the stack has content in.
pub fn composite<C: Channel>(nodes: &[Node<'_, C>]) -> Raster<C> {
    composite_tiles(nodes, covered_tiles(nodes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pixel::from_straight;

    use crate::geom::Rect;

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
        let out = composite_tile(&[Layer::new(&r).into()], coord).unwrap();
        assert!(out.shares_data_with(r.tile(coord).unwrap()));
    }

    #[test]
    fn opaque_top_layer_wins_exactly() {
        let bottom: Raster<u8> = solid([0.2, 0.4, 0.6, 1.0]);
        let top: Raster<u8> = solid([0.9, 0.1, 0.5, 1.0]);
        let out = composite(&[Layer::new(&bottom).into(), Layer::new(&top).into()]);
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
            Layer::new(&bottom).into(),
            hidden.into(),
            Layer::new(&top).with_opacity(0.0).into(),
        ]);
        assert_eq!(out, bottom);
    }

    #[test]
    fn empty_stack_composites_to_nothing() {
        let empty: Raster<u8> = Raster::new();
        assert!(composite(&[Layer::new(&empty).into()]).is_empty());
        assert!(composite_tile::<u8>(&[], TileCoord::new(0, 0)).is_none());
    }

    #[test]
    fn layers_on_different_tiles_both_appear() {
        let mut a: Raster<u8> = Raster::new();
        a.set_pixel(5, 5, [10, 20, 30, 255]);
        let mut b: Raster<u8> = Raster::new();
        b.set_pixel(-5, 600, [40, 50, 60, 255]);
        let out = composite(&[
            Layer::new(&a).into(),
            Layer::new(&b).with_blend(BlendMode::Multiply).into(),
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
            Layer::new(&bottom).into(),
            Layer::new(&top).with_blend(BlendMode::Multiply).into(),
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
                Layer::new(&bottom).into(),
                Layer::new(&top).with_blend(mode).with_opacity(0.9).into(),
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
        let out = composite(&[
            Layer::new(&under).into(),
            Layer::new(&hdr).with_opacity(0.5).into(),
        ]);
        assert_px(out.pixel(0, 0), [2.05, 1.05, 0.55, 1.0]);
    }

    fn group<'a>(children: Vec<Node<'a, u8>>, blend: GroupBlend, opacity: f32) -> Node<'a, u8> {
        Group {
            children,
            opacity,
            blend,
            visible: true,
        }
        .into()
    }

    #[test]
    fn tile_aligned_offset_shares_the_source_tile() {
        let r: Raster<u8> = solid([0.2, 0.4, 0.6, 1.0]);
        let nodes = [Layer::new(&r).with_offset(512, -256).into()];
        let out = composite(&nodes);
        assert_eq!(out.pixel(513, -255), r.pixel(1, 1));
        assert_eq!(out.pixel(1, 1), [0; 4]);
        let dest = TileCoord::new(2, -1);
        assert!(out
            .tile(dest)
            .unwrap()
            .shares_data_with(r.tile(TileCoord::new(0, 0)).unwrap()));
    }

    #[test]
    fn unaligned_offset_moves_pixels_across_tile_boundaries() {
        // A 4x4 block at the origin, moved so it straddles four tiles.
        let r: Raster<u8> = solid([0.2, 0.4, 0.6, 1.0]);
        let nodes = [Layer::new(&r).with_offset(254, 254).into()];
        assert_eq!(covered_tiles(&nodes).len(), 4);
        let out = composite(&nodes);
        let p = r.pixel(0, 0);
        for (x, y) in [(254, 254), (257, 254), (254, 257), (257, 257)] {
            assert_eq!(out.pixel(x, y), p, "at {x},{y}");
        }
        for (x, y) in [(253, 254), (258, 257), (0, 0), (254, 258)] {
            assert_eq!(out.pixel(x, y), [0; 4], "at {x},{y}");
        }
        // Negative offsets too.
        let out = composite(&[Layer::new(&r).with_offset(-3, -1).into()]);
        assert_eq!(out.pixel(-3, -1), p);
        assert_eq!(out.pixel(0, 2), p);
        assert_eq!(out.pixel(1, 0), [0; 4]);
    }

    #[test]
    fn isolated_normal_group_of_one_layer_equals_the_layer() {
        let bottom: Raster<u8> = solid([0.2, 0.4, 0.6, 1.0]);
        let top: Raster<u8> = solid([0.9, 0.1, 0.5, 0.5]);
        let flat = composite(&[Layer::new(&bottom).into(), Layer::new(&top).into()]);
        let grouped = composite(&[
            Layer::new(&bottom).into(),
            group(
                vec![Layer::new(&top).into()],
                GroupBlend::Isolated(BlendMode::Normal),
                1.0,
            ),
        ]);
        assert_eq!(grouped, flat);
    }

    #[test]
    fn pass_through_lets_blend_modes_reach_below_the_group_and_isolation_stops_them() {
        // 0.4 grey under a 0.6 grey Multiply layer that sits inside a group.
        let bottom: Raster<u8> = solid([0.4, 0.4, 0.4, 1.0]);
        let top: Raster<u8> = solid([0.6, 0.6, 0.6, 1.0]);
        let multiply = || vec![Layer::new(&top).with_blend(BlendMode::Multiply).into()];

        let pass = composite(&[
            Layer::new(&bottom).into(),
            group(multiply(), GroupBlend::PassThrough, 1.0),
        ]);
        assert_eq!(pass.pixel(0, 0), [61, 61, 61, 255]); // 0.24

        // Isolated: inside the group there is nothing to multiply with, so
        // the layer is plain 0.6 grey, then drawn Normal over the backdrop.
        let isolated = composite(&[
            Layer::new(&bottom).into(),
            group(multiply(), GroupBlend::Isolated(BlendMode::Normal), 1.0),
        ]);
        assert_eq!(isolated.pixel(0, 0), [153, 153, 153, 255]);
    }

    #[test]
    fn group_opacity_fades_the_whole_group() {
        let bottom: Raster<u8> = solid([0.0, 0.0, 0.0, 1.0]);
        let top: Raster<u8> = solid([1.0, 1.0, 1.0, 1.0]);
        let children = || vec![Layer::new(&top).into()];
        // Half of white over black is mid grey, either way.
        let pass = composite(&[
            Layer::new(&bottom).into(),
            group(children(), GroupBlend::PassThrough, 0.5),
        ]);
        let isolated = composite(&[
            Layer::new(&bottom).into(),
            group(children(), GroupBlend::Isolated(BlendMode::Normal), 0.5),
        ]);
        assert_eq!(pass.pixel(0, 0), [128, 128, 128, 255]);
        assert_eq!(isolated.pixel(0, 0), [128, 128, 128, 255]);
    }

    #[test]
    fn isolated_group_blend_mode_applies_to_the_group_result() {
        let bottom: Raster<u8> = solid([0.4, 0.4, 0.4, 1.0]);
        let a: Raster<u8> = solid([0.2, 0.2, 0.2, 1.0]);
        let b: Raster<u8> = solid([0.6, 0.6, 0.6, 1.0]);
        // Inside the group b covers a, so the group is 0.6 grey; multiplied
        // with the 0.4 backdrop that is 0.24.
        let out = composite(&[
            Layer::new(&bottom).into(),
            group(
                vec![Layer::new(&a).into(), Layer::new(&b).into()],
                GroupBlend::Isolated(BlendMode::Multiply),
                1.0,
            ),
        ]);
        assert_eq!(out.pixel(0, 0), [61, 61, 61, 255]);
    }

    #[test]
    fn hidden_and_empty_groups_draw_nothing() {
        let bottom: Raster<u8> = solid([0.2, 0.4, 0.6, 1.0]);
        let top: Raster<u8> = solid([0.9, 0.1, 0.5, 1.0]);
        let hidden: Node<u8> = Group {
            children: vec![Layer::new(&top).into()],
            opacity: 1.0,
            blend: GroupBlend::PassThrough,
            visible: false,
        }
        .into();
        let out = composite(&[
            Layer::new(&bottom).into(),
            hidden,
            group(vec![], GroupBlend::Isolated(BlendMode::Multiply), 1.0),
            group(vec![], GroupBlend::PassThrough, 0.5),
        ]);
        assert_eq!(out, bottom);
        // Untouched, so the backdrop tile is still shared.
        let coord = TileCoord::new(0, 0);
        assert!(out
            .tile(coord)
            .unwrap()
            .shares_data_with(bottom.tile(coord).unwrap()));
    }

    #[test]
    fn nested_groups_composite_inside_out() {
        let bottom: Raster<u8> = solid([0.4, 0.4, 0.4, 1.0]);
        let top: Raster<u8> = solid([0.6, 0.6, 0.6, 1.0]);
        let inner = group(
            vec![Layer::new(&top).with_blend(BlendMode::Multiply).into()],
            GroupBlend::PassThrough,
            1.0,
        );
        let outer = group(vec![inner], GroupBlend::PassThrough, 1.0);
        let out = composite(&[Layer::new(&bottom).into(), outer]);
        assert_eq!(out.pixel(0, 0), [61, 61, 61, 255]);
    }
}
